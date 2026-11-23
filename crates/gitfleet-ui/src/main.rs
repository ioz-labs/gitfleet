#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod graph;
mod icons;
mod preview;
mod syntax;
mod terminal;
mod theme;
mod views;

use app::GitFleetApp;

fn main() -> iced::Result {
    gitfleet_app::updater::cleanup_stale_old_binary();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let mut builder = iced::daemon(GitFleetApp::title, GitFleetApp::update, GitFleetApp::view)
        .theme(GitFleetApp::theme)
        .default_font(theme::UI_FONT)
        .subscription(GitFleetApp::subscription);

    for font_bytes in theme::ALL_BUNDLED_FONTS {
        builder = builder.font(font_bytes);
    }

    builder.run_with(GitFleetApp::new)
}
