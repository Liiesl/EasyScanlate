mod app;

use iced::{Size, Theme};

fn main() -> iced::Result {
    scanlateit_settings::init();
    iced::application(app::boot, app::update, app::view)
        .title("Scanlateit")
        .window_size(Size::new(1400.0, 900.0))
        .theme(|_: &app::App| {
            if scanlateit_settings::get(|s| s.aurora_is_dark) {
                Theme::Dark
            } else {
                Theme::Light
            }
        })
        .subscription(app::subscription)
        .run()
}
