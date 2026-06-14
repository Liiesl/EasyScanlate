mod app;
mod settings;

use iced::{Size, Theme};

fn main() -> iced::Result {
    iced::application(app::boot, app::update, app::view)
        .title("Scanlateit")
        .window_size(Size::new(1400.0, 900.0))
        .theme(Theme::Dark)
        .subscription(app::subscription)
        .run()
}