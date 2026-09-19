//! Terminal and event loop: `select!` over the keyboard, actions and a tick
//! that is only armed while it is needed.

use std::io::stdout;

use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event,
    EventStream, KeyboardEnhancementFlags, MouseButton, MouseEventKind,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use futures_util::StreamExt;
use ratatui::DefaultTerminal;
use tokio::sync::mpsc;

use crate::app::{Action, App, TICK};
use crate::view::view;

#[derive(Clone, Copy)]
struct Extras {
    kitty: bool,
    mouse: bool,
}

impl Extras {
    fn enable(mouse: bool) -> Self {
        let _ = execute!(stdout(), EnableBracketedPaste);
        let kitty = crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false)
            && execute!(
                stdout(),
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
            )
            .is_ok();
        let mouse = mouse && execute!(stdout(), EnableMouseCapture).is_ok();
        tracing::debug!(kitty, mouse, "terminal ready");
        Self { kitty, mouse }
    }

    fn disable(self) {
        if self.mouse {
            let _ = execute!(stdout(), DisableMouseCapture);
        }
        if self.kitty {
            let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
        }
        let _ = execute!(stdout(), DisableBracketedPaste);
    }
}

pub async fn run(app: &mut App) -> anyhow::Result<()> {
    let mut terminal = ratatui::init();
    let extras = Extras::enable(app.cfg.general.mouse);
    // ratatui::init already restores the terminal on a panic; our own setup
    // has to be undone first.
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        extras.disable();
        prev(info);
    }));
    let result = event_loop(app, &mut terminal).await;
    extras.disable();
    ratatui::restore();
    result
}

async fn event_loop(app: &mut App, terminal: &mut DefaultTerminal) -> anyhow::Result<()> {
    let mut events = EventStream::new();
    let (tx, mut rx) = mpsc::unbounded_channel::<Action>();
    let mut tick = tokio::time::interval(TICK);
    #[cfg(unix)]
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;

    app.bootstrap(&tx);

    loop {
        terminal.draw(|frame| view(app, frame))?;

        #[cfg(unix)]
        let term_signal = sigterm.recv();
        #[cfg(not(unix))]
        let term_signal = std::future::pending::<Option<()>>();

        tokio::select! {
            ev = events.next() => match ev {
                Some(Ok(ev)) => handle_terminal_event(app, ev, &tx),
                Some(Err(e)) => tracing::warn!(error = %e, "evento de terminal"),
                None => break,
            },
            Some(action) = rx.recv() => {
                app.update(action, &tx);
                // batch the deltas that arrived together before drawing
                while let Ok(more) = rx.try_recv() {
                    app.update(more, &tx);
                }
            }
            _ = tick.tick(), if app.needs_tick() => app.update(Action::Tick, &tx),
            _ = term_signal => break,
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}

fn handle_terminal_event(app: &mut App, ev: Event, tx: &mpsc::UnboundedSender<Action>) {
    match ev {
        Event::Key(k) => app.update(Action::Key(k), tx),
        Event::Paste(s) => app.update(Action::Paste(s), tx),
        Event::Resize(_, _) => app.update(Action::Resize, tx),
        Event::Mouse(m) => match m.kind {
            MouseEventKind::ScrollUp => app.update(Action::ScrollBy(-3), tx),
            MouseEventKind::ScrollDown => app.update(Action::ScrollBy(3), tx),
            MouseEventKind::Down(MouseButton::Left) => {
                app.update(Action::MouseDown(m.column, m.row), tx)
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                app.update(Action::MouseDrag(m.column, m.row), tx)
            }
            MouseEventKind::Up(MouseButton::Left) => {
                app.update(Action::MouseUp(m.column, m.row), tx)
            }
            _ => {}
        },
        _ => {}
    }
}
