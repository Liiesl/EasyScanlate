mod app;
mod assoc;
mod single_instance;
mod updater;

use iced::Size;
use lucide_icons::LUCIDE_FONT_BYTES;
use neverliie_iced_widgets::title_bar::{NativeFrame, NativeFrameConfig};
use std::path::PathBuf;
#[cfg(all(windows, feature = "updates"))]
use velopack::VelopackApp;

fn print_help() {
    println!(
        r#"EasyScanlate — EasyScanlate (EasyScanlate.exe)

Usage:
  easyscanlate [OPTIONS] [PATH]

Arguments:
  PATH   Optional .mmtl project to open. Double-click association passes
         the file as the first argument (e.g. easyscanlate "C:\path\to\proj.mmtl").

Options:
  -h, --help          Show this help and exit
  -V, --version       Show version and exit
      --register      Register .mmtl with this executable (per-user HKCU, no admin)
      --unregister    Remove per-user .mmtl file association
      --check-assoc   Print whether .mmtl is currently associated with this exe

File association (Windows, per-user):
  Writes HKCU\Software\Classes\.mmtl → EasyScanlate.MMTLFile and
  HKCU\Software\Classes\EasyScanlate.MMTLFile\shell\open\command →
  "<exe>" "%1" (mirrors ManhwaOCR installer.nsi but without elevation).

Single-instance:
  A second launch with a .mmtl path forwards the path to the running
  instance via localhost TCP (port {}) and exits. The primary opens the
  project in a new tab.

Examples:
  easyscanlate project.mmtl
  easyscanlate --register
  easyscanlate --check-assoc
"#,
        single_instance::SINGLE_INSTANCE_PORT
    );
}

fn main() -> iced::Result {
    // ---- Velopack lifecycle (must be first, handles install/update/uninstall and exits) ----
    // Skipped when the `updates` feature is off (e.g. test-ui builds).
    // Fast hooks run during install/update/uninstall (also on --silent installs:
    // --silent only skips the final auto-launch, not the hooks). They must be
    // fast, show no UI, and never fail the install, so registry writes are
    // best-effort. HKCU needs no elevation, unlike the legacy HKLM NSIS keys.
    #[cfg(all(windows, feature = "updates", feature = "file-assoc"))]
    VelopackApp::build()
        .on_after_install_fast_callback(|_| {
            let _ = assoc::register();
        })
        .on_after_update_fast_callback(|_| {
            let _ = assoc::register();
        })
        .on_before_uninstall_fast_callback(|_| {
            let _ = assoc::unregister();
        })
        .run();
    #[cfg(all(windows, feature = "updates", not(feature = "file-assoc")))]
    VelopackApp::build().run();

    // ---- CLI flags (handled before iced / single-instance) ----------------
    let args: Vec<String> = std::env::args().collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);

    if has("--help") || has("-h") {
        print_help();
        return Ok(());
    }
    if has("--version") || has("-V") {
        println!("easyscanlate {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if has("--register") {
        match assoc::register() {
            Ok(()) => println!("Registered .mmtl → {} for {}", assoc::PROG_ID, std::env::current_exe().unwrap_or_else(|_| PathBuf::from("this exe")).display()),
            Err(e) => {
                eprintln!("Register failed: {e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if has("--unregister") {
        match assoc::unregister() {
            Ok(()) => println!("Removed per-user .mmtl association ({})", assoc::PROG_ID),
            Err(e) => {
                eprintln!("Unregister failed: {e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if has("--check-assoc") {
        println!("{}", if assoc::is_registered() { "registered" } else { "not-registered" });
        return Ok(());
    }

    // ---- Initial .mmtl path (first non-flag .mmtl arg, like ManhwaOCR main.py:216) ---
    let initial_mmtl_str = single_instance::parse_initial_mmtl(&args);
    let initial_mmtl_path: Option<PathBuf> = initial_mmtl_str
        .clone()
        .map(|s| PathBuf::from(s.trim().trim_matches('"').to_string()));

    // ---- Single-instance: secondary forwards and exits, primary keeps listener ---
    let ipc_listener = single_instance::acquire_or_forward(initial_mmtl_str.clone());

    easyscanlate_settings::init();

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
        size: Size::new(1024.0, 600.0),
        ..iced::window::Settings::default()
    });

    let ipc_cell = std::sync::Arc::new(std::sync::Mutex::new(ipc_listener));
    iced::application(
        {
            let frame = frame.clone();
            let initial = initial_mmtl_path.clone();
            let ipc_cell = ipc_cell.clone();
            move || {
                let listener = ipc_cell.lock().expect("ipc lock").take();
                let (app, task) = app::boot(frame.clone(), initial.clone(), listener);
                (app, iced::Task::batch([task, frame.clone().install_latest().discard()]))
            }
        },
        app::update,
        app::view,
    )
    .window(settings)
    .font(LUCIDE_FONT_BYTES)
    // Bundled text fonts: Anime Ace (regular + bold + italic) as default, and Augie.
    // Embedded at compile time — no system install or `assets/fonts/` at runtime needed.
    .font(include_bytes!("../assets/fonts/animeace.ttf"))
    .font(include_bytes!("../assets/fonts/anime-ace.bold.ttf"))
    .font(include_bytes!("../assets/fonts/anime-ace.italic.ttf"))
    .font(include_bytes!("../assets/fonts/augie.ttf"))
    .title("EasyScanlate")
    .theme(|app: &app::App| app.theme())
    .subscription(app::subscription)
    .run()
}
