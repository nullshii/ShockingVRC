pub mod app;
pub mod event;
pub mod preview;
pub mod tabs;
pub mod theme;
pub mod tutorial;
pub mod update_overlay;
pub mod ui;

use std::io::{self, Stdout};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use tokio::sync::broadcast;

use shocking_vrc_core::cli::CliStatus;

use crate::app_state::AppState;
use crate::tui_logger::LogBuffer;

use app::App;
use event::InputEvent;

type Backend = CrosstermBackend<Stdout>;

fn init_terminal() -> io::Result<Terminal<Backend>> {
        enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

fn restore_terminal(terminal: &mut Terminal<Backend>) -> io::Result<()> {
        disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
        Ok(())
    }

fn restore_on_panic() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
}

fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_on_panic();
        original(info);
    }));
}

pub async fn run(
    state: Arc<AppState>,
    mut status_rx: broadcast::Receiver<CliStatus>,
    log_buffer: LogBuffer,
    skip_update_check: bool,
) -> io::Result<Option<(PathBuf, Vec<String>)>> {
    install_panic_hook();
    let mut terminal = init_terminal()?;

    let mut app = App::new(state, log_buffer);
    app.refresh_all().await;
    if !skip_update_check {
        app.spawn_update_check();
    }

    let mut input_rx = event::spawn_input_thread();
    let mut tick = tokio::time::interval(Duration::from_millis(33));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let result = loop {
        app.poll_presets_load();
        app.poll_update_check();
        app.poll_update_download();
        app.check_update_download_stall();
        if let Err(e) = terminal.draw(|f| ui::draw(f, &mut app)) {
            break Err(e);
        }
        if app.should_quit {
            break Ok(app.pending_self_update.take());
        }

        tokio::select! {
            maybe_ev = input_rx.recv() => {
                match maybe_ev {
                    Some(InputEvent::Key(k)) => app.handle_key(k).await,
                    Some(InputEvent::Mouse(m)) => app.handle_mouse(m).await,
                    Some(InputEvent::Resize) => {}
                    None => app.should_quit = true,
                }
            }
            st = status_rx.recv() => {
                match st {
                    Ok(s) => {
                        app.status = s;
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => {}
                }
            }
            _ = tick.tick() => {
                app.poll_presets_load();
                app.poll_update_check();
                app.poll_update_download();
                app.check_update_download_stall();
                app.maybe_refresh().await;
            }
        }
    };

    restore_terminal(&mut terminal)?;
    result
}
