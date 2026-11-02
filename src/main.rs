mod app;
mod keys;
mod launcher;
mod model;
mod store;
mod ui;

use std::io;
use std::time::Duration;

use ratatui::crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind};
use ratatui::crossterm::terminal::SetTitle;
use ratatui::DefaultTerminal;

use crate::app::App;

const HELP: &str = "\
discord-fake-game-launcher (ratatui)

USAGE:
    discord-fake-game-launcher [--os win32|linux|darwin]

OPTIONS:
    --os <os>        Which executable list to use (default: this machine's OS)
    -h, --help       Show this help
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "-h" || a == "--help") {
        print!("{HELP}");
        return;
    }

    let os = match args.iter().position(|a| a == "--os") {
        Some(pos) => match args.get(pos + 1) {
            Some(v) => v.to_lowercase(),
            None => {
                eprintln!("--os needs a value (win32, linux, darwin)");
                std::process::exit(2);
            }
        },
        None => model::host_os().to_string(),
    };

    let mut app = App::new(os);

    let mut terminal = ratatui::init();
    let _ = ratatui::crossterm::execute!(
        io::stdout(),
        EnableMouseCapture,
        SetTitle("Discord Fake Game Launcher")
    );
    let result = run(&mut terminal, &mut app);
    let _ = ratatui::crossterm::execute!(io::stdout(), DisableMouseCapture);
    ratatui::restore();

    app.shutdown();

    if let Err(e) = result {
        eprintln!("error: {}", store::io_text(&e));
        std::process::exit(1);
    }
}

fn run(terminal: &mut DefaultTerminal, app: &mut App) -> io::Result<()> {
    while !app.should_quit {
        terminal.draw(|f| ui::draw(f, app))?;

        let wait = if app.syncing { 80 } else { 150 };
        if event::poll(Duration::from_millis(wait))? {
            loop {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => app.handle_key(key),
                    Event::Mouse(m) => app.handle_mouse(m),
                    _ => {}
                }
                if app.should_quit || !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }
        app.tick();
    }
    Ok(())
}
