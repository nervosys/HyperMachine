//! `hm-tui`: a terminal dashboard over the same two APIs `hm-gui` and `hm
//! sandbox vm` already use -- `hv2-api`'s `/vms` for Type 2 VMs and a
//! sandboxd node's `/sandboxes` for sandbox VMs. It is read-mostly: list
//! both, select a row, start/stop or delete it. Nothing here talks to a
//! hypervisor directly; both backends are reached over HTTP, so this runs
//! anywhere `hm serve` or a sandboxd node is reachable, including over SSH
//! with no GUI at all.

mod api;
mod app;
mod ui;

use anyhow::Result;
use app::App;
use clap::Parser;
use crossterm::{
    event::{self, Event as CEvent, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::Duration;

#[derive(Parser)]
#[command(
    name = "hm-tui",
    about = "HyperMachine terminal dashboard: Type 2 VMs and sandbox VMs, Docker-Desktop style"
)]
struct Args {
    /// hv2-api's REST URL, from `hm serve` (the Type 2 VM list and lifecycle)
    #[arg(long)]
    api_url: Option<String>,

    /// A sandboxd node's URL (the sandbox VM list)
    #[arg(long)]
    sandbox_url: Option<String>,
}

impl Args {
    /// `--api-url`, else `HV2_API_URL`, else `hm serve`'s own default.
    /// `clap`'s `env` attribute needs a feature this workspace doesn't turn
    /// on for every other binary that uses it, so this reads the variable
    /// by hand instead of adding one more thing that differs crate to crate.
    fn resolved_api_url(&self) -> String {
        self.api_url.clone().unwrap_or_else(|| {
            std::env::var("HV2_API_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string())
        })
    }

    fn resolved_sandbox_url(&self) -> String {
        self.sandbox_url.clone().unwrap_or_else(|| {
            std::env::var("HV2_SANDBOX_URL").unwrap_or_else(|_| "http://127.0.0.1:3980".to_string())
        })
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let handle = runtime.handle().clone();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(args.resolved_api_url(), args.resolved_sandbox_url(), handle);
    let result = run(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> Result<()> {
    loop {
        app.drain_events();
        app.maybe_refresh();

        terminal.draw(|frame| ui::draw(frame, app))?;

        // A short poll keeps the UI live for auto-refresh without busy-looping:
        // no key press still wakes up often enough that a refreshed list
        // appears within a fraction of a second of arriving.
        if event::poll(Duration::from_millis(150))? {
            if let CEvent::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
                    KeyCode::Tab | KeyCode::Left | KeyCode::Right => app.switch_tab(),
                    KeyCode::Up | KeyCode::Char('k') => app.move_selection(-1),
                    KeyCode::Down | KeyCode::Char('j') => app.move_selection(1),
                    KeyCode::Char('r') => app.refresh(),
                    KeyCode::Char('s') => app.toggle_selected_vm(),
                    KeyCode::Char('d') => app.request_delete_selected(),
                    _ => {}
                }
            }
        }

        if app.should_quit {
            return Ok(());
        }
    }
}
