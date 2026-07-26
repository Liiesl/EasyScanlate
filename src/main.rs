mod app;

use iced::Size;
use lucide_icons::LUCIDE_FONT_BYTES;
use neverliie_iced_widgets::title_bar::{NativeFrame, NativeFrameConfig};

fn main() -> iced::Result {
    scanlateit_settings::init();

    // Single-window custom frame: fixed chrome — not scaled with ui_font_size.
    let frame = NativeFrame::new(
        NativeFrameConfig::platform_default()
            .corner_radius(8.0)
            .frame_border(true)
            .outer_padding(0.0)
            .title_bar_height(32.0)
            .caption_button_width(46.0)
            .show_title(false),
    );

    let settings = frame.window_settings(iced::window::Settings {
        size: Size::new(1400.0, 900.0),
        ..iced::window::Settings::default()
    });

    iced::application(
        {
            let frame = frame.clone();
            move || {
                let (app, task) = app::boot(frame.clone());
                (app, iced::Task::batch([task, frame.clone().install_latest().discard()]))
            }
        },
        app::update,
        app::view,
    )
    .window(settings)
    .font(LUCIDE_FONT_BYTES)
    .title("Scanlateit")
    .theme(|app: &app::App| app.theme())
    .subscription(app::subscription)
    .run()
}
