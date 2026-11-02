use ratatui::layout::{Alignment, Constraint, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, List, ListItem, ListState, Padding, Paragraph, Scrollbar, ScrollbarOrientation,
    ScrollbarState,
};
use ratatui::Frame;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::app::{App, StatusKind, Tab};
use crate::launcher;

const ACCENT: Color = Color::Rgb(240, 148, 186);
const TEXT: Color = Color::Rgb(224, 224, 230);
const DIM: Color = Color::Rgb(136, 136, 148);
const BORDER: Color = Color::Rgb(62, 62, 74);
const INPUT_BG: Color = Color::Rgb(34, 34, 42);
const SELECTED_BG: Color = Color::Rgb(50, 42, 52);
const GREEN: Color = Color::Rgb(152, 214, 160);
const YELLOW: Color = Color::Rgb(240, 210, 150);
const RED: Color = Color::Rgb(235, 110, 110);

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

fn spinner() -> &'static str {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    SPINNER[(ms / 100) as usize % SPINNER.len()]
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area().inner(Margin {
        horizontal: 2,
        vertical: 0,
    });

    let show_status =
        app.status_kind == StatusKind::Error && !app.status.trim().is_empty();

    let [_top, header, _g1, search, _g2, body, _g3, status, hints, _bottom] =
        Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(if show_status { 2 } else { 0 }),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(area);

    draw_header(f, app, header);
    draw_search(f, app, search);

    let [left, right] =
        Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)]).areas(body);
    draw_list(f, app, left);
    draw_details(f, app, right);
    draw_list_scrollbar(f, app);

    if show_status {
        draw_status(f, app, status);
    }
    draw_hints(f, app, hints);
}

fn fmt_num(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(' ');
        }
        out.push(ch);
    }
    out
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let left = Line::from(Span::styled(
        "Discord Fake Game Launcher",
        Style::new().fg(TEXT).add_modifier(Modifier::BOLD),
    ));
    let left_w = left.width();
    f.render_widget(Paragraph::new(left), area);

    let running = (!app.running.is_empty()).then(|| {
        Span::styled(
            format!("● {} running", app.running.len()),
            Style::new().fg(GREEN),
        )
    });

    let mut stats: Vec<Span<'static>> = Vec::new();
    if app.syncing {
        stats.push(Span::styled(spinner(), Style::new().fg(ACCENT)));
        stats.push(Span::styled(" syncing · ", Style::new().fg(DIM)));
    }
    stats.push(Span::styled(
        format!("{} · {} games", app.os, fmt_num(app.db.len())),
        Style::new().fg(DIM),
    ));

    let free = (area.width as usize).saturating_sub(left_w + 2);
    let mut candidates: Vec<Vec<Span<'static>>> = Vec::new();
    if let Some(r) = running {
        let mut full = vec![r.clone(), Span::raw("   ")];
        full.extend(stats.clone());
        candidates.push(full);
        candidates.push(vec![r]);
    }
    candidates.push(stats);

    if let Some(spans) = candidates
        .into_iter()
        .find(|c| Line::from(c.clone()).width() <= free)
    {
        f.render_widget(
            Paragraph::new(Line::from(spans)).alignment(Alignment::Right),
            area,
        );
    }
}

fn draw_search(f: &mut Frame, app: &App, area: Rect) {
    let bar = Style::new().bg(INPUT_BG);

    let mut spans = vec![Span::styled(
        " › ",
        Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
    )];
    if app.filter.is_empty() {
        spans.push(Span::styled(
            "start typing a game name…",
            Style::new().fg(DIM),
        ));
    } else {
        spans.push(Span::styled(app.filter.clone(), Style::new().fg(TEXT)));
    }
    f.render_widget(Paragraph::new(Line::from(spans)).style(bar), area);

    let found = match app.tab {
        Tab::Browse => app.filtered.len(),
        Tab::Library => app.lib_filtered.len(),
    };
    let found_text = format!("{} found ", fmt_num(found));
    let used = 3 + if app.filter.is_empty() {
        "start typing a game name…".chars().count()
    } else {
        app.filter.chars().count()
    };

    if used + 2 + found_text.chars().count() <= area.width as usize {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(found_text, Style::new().fg(DIM))))
                .alignment(Alignment::Right)
                .style(bar),
            area,
        );
    }
}

fn tab_spans(label: &str, count: usize, active: bool) -> Vec<Span<'static>> {
    if active {
        vec![
            Span::styled(
                label.to_string(),
                Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {}", fmt_num(count)), Style::new().fg(ACCENT)),
        ]
    } else {
        vec![
            Span::styled(label.to_string(), Style::new().fg(DIM)),
            Span::styled(format!(" {}", fmt_num(count)), Style::new().fg(DIM)),
        ]
    }
}

fn draw_tabs(f: &mut Frame, app: &mut App, area: Rect) {
    app.tabs_y = area.y;
    app.tab_hits.clear();

    let browse = tab_spans("Browse", app.filtered.len(), app.tab == Tab::Browse);
    let library = tab_spans("Library", app.library.len(), app.tab == Tab::Library);
    let browse_w = Line::from(browse.clone()).width() as u16;
    let library_w = Line::from(library.clone()).width() as u16;
    let x0 = area.x + 1;
    app.tab_hits.push((x0, x0 + browse_w, Tab::Browse));
    let x1 = x0 + browse_w + 3;
    app.tab_hits.push((x1, x1 + library_w, Tab::Library));

    let mut spans: Vec<Span<'static>> = vec![Span::raw(" ")];
    spans.extend(browse);
    spans.push(Span::raw("   "));
    spans.extend(library);
    let tabs_w = Line::from(spans.clone()).width();
    f.render_widget(Paragraph::new(Line::from(spans)), area);

    if app.tab == Tab::Browse {
        let (long, short) = if app.show_all {
            ("all games · Ctrl+N: only with exe ", "all · Ctrl+N ")
        } else {
            ("known exe only · Ctrl+N: show all ", "exe only · Ctrl+N ")
        };

        let free = (area.width as usize).saturating_sub(tabs_w + 2);
        let note = [long, short]
            .into_iter()
            .find(|n| n.chars().count() <= free);
        if let Some(note) = note {
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(note, Style::new().fg(TEXT))))
                    .alignment(Alignment::Right),
                area,
            );
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

fn row(name: &str, mark: Span<'static>, dim_name: bool, avail: usize) -> ListItem<'static> {
    let name_style = Style::new().fg(if dim_name { DIM } else { TEXT });
    let name = truncate(name, avail.saturating_sub(1 + mark.width()).max(1));
    ListItem::new(Line::from(vec![
        Span::raw(" "),
        Span::styled(name, name_style),
        mark,
    ]))
}

fn empty_state(f: &mut Frame, area: Rect, lines: Vec<String>) {
    let top = (area.height as usize).saturating_sub(lines.len()) / 2;
    let mut text: Vec<Line> = vec![Line::raw(""); top];
    for l in lines {
        text.push(Line::from(Span::styled(l, Style::new().fg(DIM))));
    }
    f.render_widget(Paragraph::new(text).alignment(Alignment::Center), area);
}

fn loading_state(f: &mut Frame, area: Rect) {
    let top = (area.height as usize).saturating_sub(1) / 2;
    let mut text: Vec<Line> = vec![Line::raw(""); top];
    text.push(Line::from(vec![
        Span::styled(spinner(), Style::new().fg(ACCENT)),
        Span::styled(" Loading the game list…", Style::new().fg(DIM)),
    ]));
    f.render_widget(Paragraph::new(text).alignment(Alignment::Center), area);
}

fn window(state: &mut ListState, len: usize, height: usize) -> (usize, usize) {
    let height = height.max(1);
    let sel = state.selected().unwrap_or(0).min(len.saturating_sub(1));
    let mut off = state.offset().min(len.saturating_sub(1));
    if sel < off {
        off = sel;
    } else if sel >= off + height {
        off = sel + 1 - height;
    }
    if off + height > len {
        off = len.saturating_sub(height);
    }
    *state.offset_mut() = off;
    (off, (off + height).min(len))
}

fn draw_list(f: &mut Frame, app: &mut App, area: Rect) {
    let [tabs, list_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(area);
    draw_tabs(f, app, tabs);
    app.list_area = list_area;

    let avail = (list_area.width as usize).saturating_sub(1);
    let selected = Style::new().bg(SELECTED_BG).add_modifier(Modifier::BOLD);

    match app.tab {
        Tab::Browse => {
            if app.filtered.is_empty() {
                if app.db.is_empty() && app.syncing {
                    loading_state(f, list_area);
                    return;
                }
                let lines = if app.db.is_empty() {
                    vec![
                        "No game list available.".to_string(),
                        "Restart the app to retry.".to_string(),
                    ]
                } else if app.filter.trim().is_empty() {
                    vec!["Nothing to show.".to_string()]
                } else {
                    let mut v = vec![format!("No games match “{}”", app.filter.trim())];
                    if !app.show_all {
                        v.push("Ctrl+N also searches games without a known exe.".to_string());
                    }
                    v
                };
                empty_state(f, list_area, lines);
                return;
            }

            let (start, end) = window(
                &mut app.browse_state,
                app.filtered.len(),
                list_area.height as usize,
            );
            let items: Vec<ListItem> = app.filtered[start..end]
                .iter()
                .filter_map(|&i| app.db.get(i))
                .map(|g| {
                    let running = app.is_running(&g.id, &g.exe);
                    let in_lib = app
                        .library
                        .iter()
                        .any(|l| l.app_id == g.id && l.exe == g.exe);
                    let mark = if running {
                        Span::styled(" ●", Style::new().fg(GREEN))
                    } else if in_lib {
                        Span::styled(" ✓", Style::new().fg(ACCENT))
                    } else {
                        Span::raw("")
                    };
                    row(&g.name, mark, g.uses_new_detection, avail)
                })
                .collect();

            let mut view = ListState::default();
            view.select(app.browse_state.selected().map(|s| s.saturating_sub(start)));
            let list = List::new(items).highlight_style(selected);
            f.render_stateful_widget(list, list_area, &mut view);
        }
        Tab::Library => {
            if app.lib_filtered.is_empty() {
                let lines = if app.library.is_empty() {
                    vec![
                        "Your library is empty.".to_string(),
                        "Launch a game in Browse or press Ctrl+A to add one.".to_string(),
                    ]
                } else {
                    vec![format!("No library games match “{}”", app.filter.trim())]
                };
                empty_state(f, list_area, lines);
                return;
            }

            let (start, end) = window(
                &mut app.lib_state,
                app.lib_filtered.len(),
                list_area.height as usize,
            );
            let items: Vec<ListItem> = app.lib_filtered[start..end]
                .iter()
                .filter_map(|&j| app.library.get(j))
                .map(|g| {
                    let mark = if app.running.contains_key(&g.key()) {
                        Span::styled(" ●", Style::new().fg(GREEN))
                    } else if g.is_favorite {
                        Span::styled(" ★", Style::new().fg(YELLOW))
                    } else {
                        Span::raw("")
                    };
                    row(&g.name, mark, false, avail)
                })
                .collect();

            let mut view = ListState::default();
            view.select(app.lib_state.selected().map(|s| s.saturating_sub(start)));
            let list = List::new(items).highlight_style(selected);
            f.render_stateful_widget(list, list_area, &mut view);
        }
    }
}

fn draw_list_scrollbar(f: &mut Frame, app: &App) {
    let list_area = app.list_area;
    let (total, offset) = match app.tab {
        Tab::Browse => (app.filtered.len(), app.browse_state.offset()),
        Tab::Library => (app.lib_filtered.len(), app.lib_state.offset()),
    };
    let h = list_area.height as usize;
    if h == 0 || total <= h {
        return;
    }
    let max_offset = total - h;
    let mut sb = ScrollbarState::new(max_offset + 1)
        .position(offset.min(max_offset))
        .viewport_content_length(h);
    let bar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some("│"))
        .thumb_symbol("┃")
        .track_style(Style::new().fg(BORDER))
        .thumb_style(Style::new().fg(ACCENT));
    let sb_x = (list_area.x + list_area.width).min(f.area().right().saturating_sub(1));
    let sb_area = Rect::new(sb_x, list_area.y, 1, list_area.height);
    f.render_stateful_widget(bar, sb_area, &mut sb);
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0usize;
    for word in text.split_whitespace() {
        let mut chars: Vec<char> = word.chars().collect();
        while chars.len() > width {
            if cur_w > 0 {
                lines.push(std::mem::take(&mut cur));
                cur_w = 0;
            }
            let rest = chars.split_off(width);
            lines.push(chars.iter().collect());
            chars = rest;
        }
        let w = chars.len();
        if cur_w == 0 {
            cur = chars.iter().collect();
            cur_w = w;
        } else if cur_w + 1 + w <= width {
            cur.push(' ');
            cur.extend(chars.iter());
            cur_w += 1 + w;
        } else {
            lines.push(std::mem::take(&mut cur));
            cur = chars.iter().collect();
            cur_w = w;
        }
    }
    if cur_w > 0 {
        lines.push(cur);
    }
    lines
}

fn field(lines: &mut Vec<Line<'static>>, label: &str, value: &str, width: usize) {
    lines.push(Line::from(Span::styled(
        label.to_string(),
        Style::new().fg(DIM),
    )));
    for l in wrap(value, width) {
        lines.push(Line::from(Span::styled(l, Style::new().fg(TEXT))));
    }
    lines.push(Line::raw(""));
}

fn build_details(app: &App, width: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    let Some(game) = app.current_game() else {
        lines.push(Line::from(Span::styled(
            "Select a game to see details.",
            Style::new().fg(DIM),
        )));
        return lines;
    };

    for l in wrap(&game.name, width) {
        lines.push(Line::from(Span::styled(
            l,
            Style::new().fg(TEXT).add_modifier(Modifier::BOLD),
        )));
    }

    let running = app.is_running(&game.app_id, &game.exe);
    let in_library = app
        .library
        .iter()
        .any(|l| l.app_id == game.app_id && l.exe == game.exe);

    let mut badges: Vec<Span<'static>> = Vec::new();
    if running {
        badges.push(Span::styled("● running", Style::new().fg(GREEN)));
    } else {
        badges.push(Span::styled("○ stopped", Style::new().fg(DIM)));
    }
    if game.is_favorite {
        badges.push(Span::styled("   ★ favorite", Style::new().fg(YELLOW)));
    } else if in_library {
        badges.push(Span::styled("   ✓ in library", Style::new().fg(ACCENT)));
    }
    lines.push(Line::from(badges));
    lines.push(Line::raw(""));

    field(&mut lines, "App ID", &game.app_id, width);
    field(&mut lines, "Executable", &game.exe, width);

    if let Some(db) = app.selected_db().filter(|_| app.tab == Tab::Browse) {
        if db.uses_new_detection {
            let note = format!(
                "Discord lists no {} executable for this game, so the name is a guess and it may not be detected.",
                app.os
            );
            for l in wrap(&note, width) {
                lines.push(Line::from(Span::styled(l, Style::new().fg(YELLOW))));
            }
            lines.push(Line::raw(""));
        }
    }

    if let Ok(p) = launcher::fake_exe_path(&game, &app.paths) {
        let shown = p
            .strip_prefix(&app.paths.dir)
            .unwrap_or(p.as_path())
            .display()
            .to_string();
        lines.push(Line::from(vec![
            Span::styled("Dummy file", Style::new().fg(DIM)),
            Span::styled(" · ", Style::new().fg(DIM)),
            Span::styled("Ctrl+O", Style::new().fg(ACCENT)),
            Span::styled(" open folder", Style::new().fg(DIM)),
        ]));
        for l in wrap(&shown, width) {
            lines.push(Line::from(Span::styled(l, Style::new().fg(TEXT))));
        }
        if !p.exists() {
            lines.push(Line::from(Span::styled(
                "(created on first launch)",
                Style::new().fg(DIM),
            )));
        }
        lines.push(Line::raw(""));
    }

    lines
}

fn draw_details(f: &mut Frame, app: &mut App, area: Rect) {
    let block = Block::new()
        .borders(Borders::LEFT)
        .border_style(Style::new().fg(BORDER))
        .padding(Padding::new(2, 0, 0, 0));
    let inner = block.inner(area);
    app.details_area = area;

    let key = format!(
        "{}|{}",
        matches!(app.tab, Tab::Library),
        app.current_game().map(|g| g.key()).unwrap_or_default()
    );
    if key != app.detail_key {
        app.detail_key = key;
        app.detail_scroll = 0;
    }

    let width = (inner.width as usize).saturating_sub(1);
    let height = inner.height as usize;

    let lines = build_details(app, width);
    let max_scroll = lines.len().saturating_sub(height);
    app.detail_scroll = app.detail_scroll.min(max_scroll);
    let start = app.detail_scroll;
    let visible: Vec<Line<'static>> = lines.into_iter().skip(start).take(height).collect();

    f.render_widget(block, area);
    f.render_widget(Paragraph::new(visible), inner);

    if inner.width > 0 && inner.height > 0 {
        let arrow = |text: &'static str, y: u16| {
            (
                Paragraph::new(Line::from(Span::styled(text, Style::new().fg(ACCENT))))
                    .alignment(Alignment::Right),
                Rect {
                    x: inner.x,
                    y,
                    width: inner.width,
                    height: 1,
                },
            )
        };
        if start > 0 {
            let (w, r) = arrow("↑", inner.y);
            f.render_widget(w, r);
        }
        if start < max_scroll {
            let (w, r) = arrow("↓", inner.y + inner.height - 1);
            f.render_widget(w, r);
        }
    }
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let (prefix, color) = match app.status_kind {
        StatusKind::Info => ("", DIM),
        StatusKind::Busy => ("⟳ ", YELLOW),
        StatusKind::Error => ("✗ ", RED),
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("{}{}", prefix, app.status),
            Style::new().fg(color),
        )))
        .alignment(Alignment::Center),
        area,
    );
}

type Hint = (&'static str, &'static str, &'static str, u8);

fn hints_width(keys: &[Hint], compact: bool) -> usize {
    let text: usize = keys
        .iter()
        .map(|(k, d, short, _)| {
            let d = if compact { short } else { d };
            k.chars().count() + 1 + d.chars().count()
        })
        .sum();
    let sep = if compact { 2 } else { 3 };
    text + sep * keys.len().saturating_sub(1)
}

fn draw_hints(f: &mut Frame, app: &App, area: Rect) {
    let mut keys: Vec<Hint> = vec![
        ("↑↓", "move", "move", 7),
        ("Enter", "launch", "run", 1),
        ("Ctrl+S", "stop", "stop", 3),
    ];
    match app.tab {
        Tab::Browse => keys.push(("Ctrl+A", "add to library", "add", 2)),
        Tab::Library => {
            keys.push(("Ctrl+F", "favorite", "fav", 2));
            keys.push(("Ctrl+D", "remove", "del", 5));
        }
    }
    keys.push(if app.tab == Tab::Browse {
        ("Tab", "library", "lib", 4)
    } else {
        ("Tab", "browse", "browse", 4)
    });
    if !app.filter.is_empty() {
        keys.push(("Esc", "clear", "clear", 8));
    }

    let max = area.width as usize;
    let compact = hints_width(&keys, false) > max;
    while hints_width(&keys, compact) > max && keys.len() > 1 {
        let worst = keys
            .iter()
            .enumerate()
            .max_by_key(|(_, h)| h.3)
            .map(|(i, _)| i)
            .unwrap_or(0);
        keys.remove(worst);
    }

    let sep = if compact { "  " } else { " · " };
    let mut spans: Vec<Span> = Vec::new();
    for (i, (k, d, short, _)) in keys.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(sep, Style::new().fg(BORDER)));
        }
        let d = if compact { short } else { d };
        spans.push(Span::styled(k.to_string(), Style::new().fg(ACCENT)));
        spans.push(Span::styled(format!(" {d}"), Style::new().fg(DIM)));
    }
    f.render_widget(
        Paragraph::new(Line::from(spans)).alignment(Alignment::Center),
        area,
    );
}
