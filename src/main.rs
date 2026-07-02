mod app;
mod settings;

use iced::{Size, Theme};

fn main() -> iced::Result {
    iced::application(app::boot, app::update, app::view)
        .title("Scanlateit")
        .window_size(Size::new(1400.0, 900.0))
        .theme(|app: &app::App| if app.aurora_is_dark { Theme::Dark } else { Theme::Light })
        .subscription(app::subscription)
        .run()
}