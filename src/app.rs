use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;

use crate::keys;
use crate::launcher;
use crate::model::{filter_indices, make_game_key, sort_library, to_db_games, DbGame, Game};
use crate::store::{self, Paths};

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
    pub syncing: bool,
    pub should_quit: bool,

    sync_rx: Option<Receiver<Result<bool, String>>>,
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
            syncing: false,
            should_quit: false,
            sync_rx: None,
        };

        app.refresh_library_view(None);

        let _ = app.reload_db();
        app.start_sync();
        app
    }

    fn info(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
        self.status_kind = StatusKind::Info;
    }

    fn busy(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
        self.status_kind = StatusKind::Busy;
    }

    fn error(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
        self.status_kind = StatusKind::Error;
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

    pub fn tick(&mut self) {
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
        let opener = match std::env::consts::OS {
            "windows" => "explorer",
            "macos" => "open",
            _ => "xdg-open",
        };
        let result = Command::new(opener)
            .arg(&self.paths.dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        if let Err(e) = result {
            self.error(format!("Could not open the folder: {}", store::io_text(&e)));
        }
    }

    fn clear_filter(&mut self) {
        if !self.filter.is_empty() {
            self.filter.clear();
            self.apply_filter();
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
