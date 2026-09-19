//! Terminal interface for moon. Works with `Arc<dyn Provider>`: it knows no
//! concrete provider.

pub mod app;
pub mod clipboard;
pub mod commands;
pub mod input;
pub mod logo;
pub mod markdown;
pub mod mentions;
pub mod picker;
pub mod run;
pub mod sysmon;
pub mod theme;
pub mod view;
pub mod wrap;

pub use app::{App, RunOptions};
pub use theme::Theme;

/// Starts the TUI and returns when it finishes.
pub async fn run(opts: RunOptions) -> anyhow::Result<()> {
    let mut app = App::new(opts);
    run::run(&mut app).await
}
