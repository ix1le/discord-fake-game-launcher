use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;
use ratatui::widgets::ListState;

use crate::keys;
use crate::launcher;
use crate::model::{filter_indices, make_game_key, sort_library, to_db_games, DbGame, Game};
use crate::store::{self, Paths, Update};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Browse,
    Library,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StatusKind {
    Info,
    Busy,
    Error,
}

const STATUS_TTL: Duration = Duration::from_secs(15);

pub struct App {
    pub tab: Tab,
    pub os: String,
    pub paths: Paths,

    pub filter: String,

    pub db: Vec<DbGame>,

    pub filtered: Vec<usize>,
    pub browse_state: ListState,

    pub show_all: bool,

    pub library: Vec<Game>,

    pub lib_filtered: Vec<usize>,
    pub lib_state: ListState,

    pub running: HashMap<String, Child>,
    pub status: String,
    pub status_kind: StatusKind,
    status_until: Option<Instant>,
    pub syncing: bool,
    pub should_quit: bool,

    pub list_area: Rect,
    pub details_area: Rect,
    pub detail_scroll: usize,
    pub detail_key: String,
    pub tabs_y: u16,
    pub tab_hits: Vec<(u16, u16, Tab)>,

    pub update: Option<Update>,
    pub update_hit: Option<(u16, u16, u16)>,

    sync_rx: Option<Receiver<Result<bool, String>>>,
    update_rx: Option<Receiver<Option<Update>>>,
}

impl App {
    pub fn new(os: String) -> Self {
        let paths = Paths::new();
        let mut library = store::load_library(&paths);
        sort_library(&mut library);

        let mut app = App {
            tab: Tab::Browse,
            os,
            paths,
            filter: String::new(),
            db: Vec::new(),
            filtered: Vec::new(),
            browse_state: ListState::default(),
            show_all: false,
            library,
            lib_filtered: Vec::new(),
            lib_state: ListState::default(),
            running: HashMap::new(),
            status: String::new(),
            status_kind: StatusKind::Info,
            status_until: None,
            syncing: false,
            should_quit: false,
            list_area: Rect::default(),
            details_area: Rect::default(),
            detail_scroll: 0,
            detail_key: String::new(),
            tabs_y: 0,
            tab_hits: Vec::new(),
            update: None,
            update_hit: None,
            sync_rx: None,
            update_rx: None,
        };

        app.refresh_library_view(None);

        let _ = app.reload_db();
        app.start_sync();
        app.start_update_check();
        app
    }

    fn info(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
        self.status_kind = StatusKind::Info;
        self.status_until = None;
    }

    fn busy(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
        self.status_kind = StatusKind::Busy;
        self.status_until = None;
    }

    fn error(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
        self.status_kind = StatusKind::Error;
        self.status_until = Some(Instant::now() + STATUS_TTL);
    }

    fn expire_status(&mut self) {
        if self.status_until.is_some_and(|t| Instant::now() >= t) {
            self.status.clear();
            self.status_kind = StatusKind::Info;
            self.status_until = None;
        }
    }

    fn reload_db(&mut self) -> Result<(), String> {
        let apps = store::load_gamelist(&self.paths)?;
        self.db = to_db_games(&apps, &self.os);
        self.apply_filter();
        Ok(())
    }

    fn apply_filter(&mut self) {
        let show_all = self.show_all;
        let db = &self.db;
        self.filtered = filter_indices(db, &self.filter)
            .into_iter()
            .filter(|&i| show_all || !db[i].uses_new_detection)
            .collect();
        self.browse_state.select(if self.filtered.is_empty() {
            None
        } else {
            Some(0)
        });

        self.lib_state.select(Some(0));
        self.refresh_library_view(None);
    }

    fn refresh_library_view(&mut self, keep: Option<String>) {
        let term = self.filter.trim().to_lowercase();
        self.lib_filtered = self
            .library
            .iter()
            .enumerate()
            .filter(|(_, g)| term.is_empty() || g.name.to_lowercase().contains(&term))
            .map(|(i, _)| i)
            .collect();

        let by_key = keep.and_then(|k| {
            self.lib_filtered
                .iter()
                .position(|&j| self.library[j].key() == k)
        });
        let sel = by_key.or_else(|| {
            if self.lib_filtered.is_empty() {
                None
            } else {
                let cur = self.lib_state.selected().unwrap_or(0);
                Some(cur.min(self.lib_filtered.len() - 1))
            }
        });
        self.lib_state.select(sel);
    }

    fn start_sync(&mut self) {
        if self.syncing {
            return;
        }
        self.syncing = true;
        self.busy("Syncing game list with Discord…");
        let (tx, rx) = mpsc::channel();
        let paths = self.paths.clone();
        thread::spawn(move || {
            let _ = tx.send(store::sync_gamelist(&paths));
        });
        self.sync_rx = Some(rx);
    }

    fn start_update_check(&mut self) {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(store::check_update());
        });
        self.update_rx = Some(rx);
    }

    pub fn tick(&mut self) {
        match self.update_rx.as_ref().map(|rx| rx.try_recv()) {
            Some(Ok(found)) => {
                self.update = found;
                self.update_rx = None;
            }
            Some(Err(TryRecvError::Disconnected)) => self.update_rx = None,
            Some(Err(TryRecvError::Empty)) | None => {}
        }

        let msg = self.sync_rx.as_ref().map(|rx| rx.try_recv());
        match msg {
            Some(Ok(result)) => {
                self.syncing = false;
                self.sync_rx = None;
                match result {
                    Ok(updated) => {
                        if updated || self.db.is_empty() {
                            match self.reload_db() {
                                Ok(()) => {
                                    let n = self.db.len();
                                    self.info(format!("Game list updated ({n} apps)"));
                                }
                                Err(e) => self.error(e),
                            }
                        } else {
                            let n = self.db.len();
                            self.info(format!("Game list is up to date ({n} apps)"));
                        }
                    }
                    Err(e) => {
                        if self.db.is_empty() {
                            self.error(format!("Sync failed and no local game list: {e}"));
                        } else {
                            self.error(format!("Sync failed, using the local list: {e}"));
                        }
                    }
                }
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.syncing = false;
                self.sync_rx = None;
            }
            Some(Err(TryRecvError::Empty)) | None => {}
        }
        self.reap();
        self.expire_status();
    }

    fn reap(&mut self) {
        let finished: Vec<String> = self
            .running
            .iter_mut()
            .filter_map(|(k, c)| match c.try_wait() {
                Ok(None) => None,
                _ => Some(k.clone()),
            })
            .collect();
        for k in finished {
            self.running.remove(&k);
            self.info("A fake game process exited.");
        }
    }

    pub fn shutdown(&mut self) {
        for (_, mut c) in self.running.drain() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    pub fn selected_db(&self) -> Option<&DbGame> {
        let i = self.browse_state.selected()?;
        self.db.get(*self.filtered.get(i)?)
    }

    pub fn current_game(&self) -> Option<Game> {
        match self.tab {
            Tab::Browse => self.selected_db().map(Game::from),
            Tab::Library => {
                let i = self.lib_state.selected()?;
                let j = *self.lib_filtered.get(i)?;
                self.library.get(j).cloned()
            }
        }
    }

    pub fn is_running(&self, app_id: &str, exe: &str) -> bool {
        self.running.contains_key(&make_game_key(app_id, exe))
    }

    fn move_selection(&mut self, delta: isize) {
        let (state, len) = match self.tab {
            Tab::Browse => (&mut self.browse_state, self.filtered.len()),
            Tab::Library => (&mut self.lib_state, self.lib_filtered.len()),
        };
        if len == 0 {
            state.select(None);
            return;
        }
        let cur = state.selected().unwrap_or(0) as isize;
        let next = (cur + delta).clamp(0, len as isize - 1);
        state.select(Some(next as usize));
    }

    fn launch(&mut self) {
        let Some(game) = self.current_game() else {
            return;
        };
        let key = game.key();
        if self.running.contains_key(&key) {
            self.info(format!("{} is already running.", game.name));
            return;
        }
        match launcher::launch(&game, &self.paths) {
            Ok((child, _exe)) => {
                self.running.insert(key, child);
                self.insert_into_library(game.clone());
                self.info(format!(
                    "Launched {} — check Discord's activity.",
                    game.name
                ));
            }
            Err(e) => self.error(format!("Failed to launch {}: {}", game.name, store::io_text(&e))),
        }
    }

    fn stop(&mut self) {
        let Some(game) = self.current_game() else {
            return;
        };
        match self.running.remove(&game.key()) {
            Some(mut c) => {
                let _ = c.kill();
                let _ = c.wait();
                self.info(format!("Stopped {}.", game.name));
            }
            None => self.info(format!("{} is not running.", game.name)),
        }
    }

    fn insert_into_library(&mut self, game: Game) -> bool {
        let key = game.key();
        if self.library.iter().any(|g| g.key() == key) {
            return false;
        }
        self.library.push(game);
        sort_library(&mut self.library);
        self.save_library();
        self.refresh_library_view(Some(key));
        true
    }

    fn add_to_library(&mut self) {
        if self.tab != Tab::Browse {
            return;
        }
        let Some(game) = self.current_game() else {
            return;
        };
        let key = game.key();
        if self.library.iter().any(|g| g.key() == key) {
            self.info(format!("{} is already in your library.", game.name));
            return;
        }
        if let Err(e) = launcher::ensure_fake_exe(&game, &self.paths) {
            self.error(format!(
                "Could not create the dummy file for {}: {}",
                game.name,
                store::io_text(&e)
            ));
            return;
        }
        let name = game.name.clone();
        self.insert_into_library(game);
        self.info(format!("Added {name} to your library."));
    }

    fn toggle_favorite(&mut self) {
        if self.tab != Tab::Library {
            return;
        }
        let Some(game) = self.current_game() else {
            return;
        };
        let key = game.key();
        if let Some(g) = self.library.iter_mut().find(|g| g.key() == key) {
            g.is_favorite = !g.is_favorite;
        }
        sort_library(&mut self.library);
        self.save_library();
        self.refresh_library_view(Some(key));
    }

    fn delete_from_library(&mut self) {
        if self.tab != Tab::Library {
            return;
        }
        let Some(game) = self.current_game() else {
            return;
        };
        let key = game.key();
        if let Some(mut c) = self.running.remove(&key) {
            let _ = c.kill();
            let _ = c.wait();
        }
        if let Err(e) = launcher::remove_fake_exe(&game, &self.paths) {
            self.error(format!(
                "Not removed, can't delete the dummy file: {}",
                store::io_text(&e)
            ));
            return;
        }
        self.library.retain(|g| g.key() != key);
        self.save_library();
        self.refresh_library_view(None);
        self.info(format!("Removed {} from your library.", game.name));
    }

    fn save_library(&mut self) {
        if let Err(e) = store::save_library(&self.paths, &self.library) {
            self.error(format!("Could not save library: {e}"));
        }
    }

    fn toggle_show_all(&mut self) {
        self.show_all = !self.show_all;
        self.apply_filter();
        if self.show_all {
            self.info("Showing all games, including ones with no executable for this OS.");
        } else {
            self.info("Showing only games with a known executable.");
        }
    }

    fn open_data_dir(&mut self) {
        if let Err(e) = self.paths.ensure_dir() {
            self.error(format!("Could not create the data folder: {}", store::io_text(&e)));
            return;
        }
        let dir = self.paths.dir.clone();
        self.open_target(dir.as_os_str(), "the folder");
    }

    fn open_update_page(&mut self) {
        if let Some(u) = self.update.clone() {
            self.open_target(std::ffi::OsStr::new(&u.url), "the release page");
        }
    }

    fn open_target(&mut self, target: &std::ffi::OsStr, what: &str) {
        let opener = match std::env::consts::OS {
            "windows" => "explorer",
            "macos" => "open",
            _ => "xdg-open",
        };
        let result = Command::new(opener)
            .arg(target)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        if let Err(e) = result {
            self.error(format!("Could not open {what}: {}", store::io_text(&e)));
        }
    }

    fn clear_filter(&mut self) {
        if !self.filter.is_empty() {
            self.filter.clear();
            self.apply_filter();
        }
    }

    fn scroll_details(&mut self, up: bool) {
        self.detail_scroll = if up {
            self.detail_scroll.saturating_sub(1)
        } else {
            self.detail_scroll + 1
        };
    }

    fn list_len(&self) -> usize {
        match self.tab {
            Tab::Browse => self.filtered.len(),
            Tab::Library => self.lib_filtered.len(),
        }
    }

    pub fn handle_mouse(&mut self, m: MouseEvent) {
        let inside = |a: Rect| {
            m.column >= a.x && m.column < a.x + a.width && m.row >= a.y && m.row < a.y + a.height
        };
        match m.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let up = m.kind == MouseEventKind::ScrollUp;
                if inside(self.details_area) {
                    self.scroll_details(up);
                } else {
                    self.move_selection(if up { -1 } else { 1 });
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some((x0, x1, y)) = self.update_hit {
                    if m.row == y && m.column >= x0 && m.column < x1 {
                        self.open_update_page();
                        return;
                    }
                }
                if m.row == self.tabs_y {
                    if let Some(&(_, _, tab)) = self
                        .tab_hits
                        .iter()
                        .find(|&&(x0, x1, _)| m.column >= x0 && m.column < x1)
                    {
                        self.tab = tab;
                    }
                    return;
                }
                if inside(self.list_area) {
                    let state = match self.tab {
                        Tab::Browse => &mut self.browse_state,
                        Tab::Library => &mut self.lib_state,
                    };
                    let idx = state.offset() + (m.row - self.list_area.y) as usize;
                    if idx < self.list_len() {
                        match self.tab {
                            Tab::Browse => self.browse_state.select(Some(idx)),
                            Tab::Library => self.lib_state.select(Some(idx)),
                        }
                    }
                }
            }
            _ => {}
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL) || keys::ctrl_held();
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        let command = ctrl && !alt;
        let text = ctrl == alt;

        match key.code {
            KeyCode::Char(c) if command => match keys::latin_letter(c) {
                'c' | 'q' => self.should_quit = true,
                's' => self.stop(),
                'a' => self.add_to_library(),
                'f' => self.toggle_favorite(),
                'd' => self.delete_from_library(),
                'n' => self.toggle_show_all(),
                'u' => self.clear_filter(),
                'o' => self.open_data_dir(),
                _ => {}
            },
            KeyCode::Char(c) if text => {
                self.filter.push(c);
                self.apply_filter();
            }
            KeyCode::Backspace => {
                if self.filter.pop().is_some() {
                    self.apply_filter();
                }
            }
            KeyCode::Esc => self.clear_filter(),
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Left | KeyCode::Right => {
                self.tab = match self.tab {
                    Tab::Browse => Tab::Library,
                    Tab::Library => Tab::Browse,
                };
            }
            KeyCode::Up if alt => self.scroll_details(true),
            KeyCode::Down if alt => self.scroll_details(false),
            KeyCode::Up => self.move_selection(-1),
            KeyCode::Down => self.move_selection(1),
            KeyCode::PageUp => self.move_selection(-10),
            KeyCode::PageDown => self.move_selection(10),
            KeyCode::Home => self.move_selection(-1_000_000),
            KeyCode::End => self.move_selection(1_000_000),
            KeyCode::Enter => self.launch(),
            KeyCode::Delete => self.delete_from_library(),
            _ => {}
        }
    }
}