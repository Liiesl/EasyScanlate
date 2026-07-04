mod app;

use iced::Size;
use neverliie_iced_widgets::title_bar::{NativeFrame, NativeFrameConfig};

fn main() -> iced::Result {
    scanlateit_settings::init();

    // Single-window custom frame: we own the title bar so the aurora can
    // show through. `outer_padding` is kept at 0 so the bar stays edge-to-
    // edge – the app's own `OUTER_PADDING` is applied to the content only.
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
    .title("Scanlateit")
    .theme(|app: &app::App| app.theme())
    .subscription(app::subscription)
    .run()
}
