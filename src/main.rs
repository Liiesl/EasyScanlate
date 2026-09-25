#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod assoc;
mod crash;
mod single_instance;
mod updater;

use iced::Size;
use lucide_icons::LUCIDE_FONT_BYTES;
use neverliie_iced_widgets::title_bar::{NativeFrame, NativeFrameConfig};
use std::path::PathBuf;
#[cfg(all(windows, feature = "updates"))]
use velopack::VelopackApp;

fn attach_parent_console() {
    // GUI-subsystem exe has no console: attach to the parent so piped
    // callers (`subprocess.PIPE`, `CreateProcess` pipes) still capture
    // stdout/stderr. Best-effort, no-op when there is no parent console.
    #[cfg(windows)]
    {
        use std::ffi::c_void;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn AttachConsole(dwProcessId: u32) -> i32;
        }
        const ATTACH_PARENT_PROCESS: u32 = 0xFFFFFFFF;
        unsafe {
            let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        }
        let _ = std::ptr::null::<c_void>();
    }
}

fn print_help() {
    attach_parent_console();
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
       --headless      Run without opening a window (JSON on stdout, logs on stderr)
       --no-single-instance  Skip single-instance forwarding (for headless callers)
       --input <PATH>  Headless input (.mmtl path; also accepts bare PATH)
       --output <DIR>  Headless output dir (default: input parent)
       --format <FMT>  Headless output format: json (default)

File association (Windows, per-user):
  Writes HKCU\Software\Classes\.mmtl → EasyScanlate.MMTLFile and
  HKCU\Software\Classes\EasyScanlate.MMTLFile\shell\open\command →
  "<exe>" "%1" (mirrors ManhwaOCR installer.nsi but without elevation).

Single-instance:
  A second launch with a .mmtl path forwards the path to the running
  instance via localhost TCP (port {}) and exits. The primary opens the
  project in a new tab. Headless callers should pass --no-single-instance
  to avoid the ~1.8s worst-case forward block and get machine-readable output.

Examples:
  easyscanlate project.mmtl
  easyscanlate --register
  easyscanlate --check-assoc
  easyscanlate --headless --input project.mmtl --output out/
  easyscanlate --headless --no-single-instance project.mmtl
"#,
        single_instance::SINGLE_INSTANCE_PORT
    );
    let _ = std::io::Write::flush(&mut std::io::stdout());
}

fn print_version() {
    attach_parent_console();
    println!("easyscanlate {}", env!("CARGO_PKG_VERSION"));
    let _ = std::io::Write::flush(&mut std::io::stdout());
}

/// Minimal headless mode for external callers: validates the input without
/// touching single-instance, settings-window, iced, GPU, fonts, or update
/// checks, prints one JSON object on stdout, and exits. Heavy pipeline work
/// (OCR/translate/export) stays in-process via the library crates when flags
/// are added later; today this path only inspects the project header fast.
fn run_headless(args: &[String]) -> ! {
    attach_parent_console();
    let get_value = |name: &str| -> Option<String> {
        let mut it = args.iter().peekable();
        while let Some(a) = it.next() {
            if a == name {
                return it.next().cloned();
            }
            if let Some(v) = a.strip_prefix(&format!("{name}=")) {
                return Some(v.to_string());
            }
        }
        None
    };
    // Accept `--input X`, `--input=X`, or a bare `.mmtl` PATH.
    let mut input = get_value("--input");
    if input.is_none() {
        input = single_instance::parse_initial_mmtl(args);
    }
    let output = get_value("--output");
    let format = get_value("--format").unwrap_or_else(|| "json".to_string());
    let input_str = input.unwrap_or_default();
    let ok_exists = !input_str.is_empty() && std::path::Path::new(&input_str).exists();
    let image_count: Option<usize> = if ok_exists {
        // Fast header-only probe off the critical path: read the zip central
        // directory via `load_mmtl` in this short-lived process. On failure
        // report the error instead of opening a window.
        match easyscanlate_mmtl::load_mmtl(std::path::Path::new(&input_str)) {
            Ok(res) => Some(res.project.image_count()),
            Err(e) => {
                eprintln!("headless load failed: {e}");
                println!(
                    "{{\"ok\":false,\"input\":{},\"error\":{}}}",
                    serde_json::to_string(&input_str).unwrap_or_default(),
                    serde_json::to_string(&e.to_string()).unwrap_or_default()
                );
                let _ = std::io::Write::flush(&mut std::io::stdout());
                std::process::exit(1);
            }
        }
    } else {
        None
    };
    let ok = ok_exists && image_count.is_some();
    // Keep schema stable for callers: {ok,input,exists,image_count,output,format,version}.
    println!(
        "{{\"ok\":{},\"input\":{},\"exists\":{},\"image_count\":{},\"output\":{},\"format\":{},\"version\":{}}}",
        if ok { "true" } else { "false" },
        serde_json::to_string(&input_str).unwrap_or_default(),
        if ok_exists { "true" } else { "false" },
        match image_count {
            Some(n) => n.to_string(),
            None => "null".to_string(),
        },
        serde_json::to_string(&output).unwrap_or_default(),
        serde_json::to_string(&format).unwrap_or_default(),
        serde_json::to_string(&env!("CARGO_PKG_VERSION")).unwrap_or_default(),
    );
    let _ = std::io::Write::flush(&mut std::io::stdout());
    std::process::exit(if ok { 0 } else { 1 });
}

fn main() -> iced::Result {
    // ---- Crash reporter child (must be first: no hook, no Velopack,
    // no single-instance forwarding — just show the dialog and exit) ----
    let early_args: Vec<String> = std::env::args().collect();
    if let Some(result) = crash::run_reporter_if_requested(&early_args) {
        return result;
    }
    // ---- Panic hook (catches everything below; release has no console so
    // without this a panic would vanish silently). The hook only writes the
    // crash log and spawns a `--crash-report` child for the dialog — it never
    // blocks on UI itself (that deadlocks the iced thread). ----
    crash::install_hook();
    if early_args.iter().any(|a| a == crash::CRASH_TEST_ARG) {
        panic!("crash-test: intentional panic to exercise the panic panel");
    }
    // ---- CLI flags (fast path: before Velopack / single-instance / iced) ----
    // (`early_args` above was only for the crash reporter pre-check.)
    // Keep these above `VelopackApp::run()` so `--help`/`--version`/headless
    // never pay update-lifecycle init when called by another program.
    let args: Vec<String> = early_args;
    let has = |flag: &str| args.iter().any(|a| a == flag);

    if has("--help") || has("-h") {
        print_help();
        return Ok(());
    }
    if has("--version") || has("-V") {
        print_version();
        return Ok(());
    }
    if has("--register") {
        attach_parent_console();
        match assoc::register() {
            Ok(()) => println!("Registered .mmtl → {} for {}", assoc::PROG_ID, std::env::current_exe().unwrap_or_else(|_| PathBuf::from("this exe")).display()),
            Err(e) => {
                eprintln!("Register failed: {e}");
                std::process::exit(1);
            }
        }
        let _ = std::io::Write::flush(&mut std::io::stdout());
        return Ok(());
    }
    if has("--unregister") {
        attach_parent_console();
        match assoc::unregister() {
            Ok(()) => println!("Removed per-user .mmtl association ({})", assoc::PROG_ID),
            Err(e) => {
                eprintln!("Unregister failed: {e}");
                std::process::exit(1);
            }
        }
        let _ = std::io::Write::flush(&mut std::io::stdout());
        return Ok(());
    }
    if has("--check-assoc") {
        attach_parent_console();
        println!("{}", if assoc::is_registered() { "registered" } else { "not-registered" });
        let _ = std::io::Write::flush(&mut std::io::stdout());
        return Ok(());
    }
    if has("--headless") {
        run_headless(&args);
    }

    // ---- Velopack lifecycle (handles install/update/uninstall and exits) ----
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

    // ---- Initial .mmtl path (first non-flag .mmtl arg, like ManhwaOCR main.py:216) ---
    let initial_mmtl_str = single_instance::parse_initial_mmtl(&args);
    let initial_mmtl_path: Option<PathBuf> = initial_mmtl_str
        .clone()
        .map(|s| PathBuf::from(s.trim().trim_matches('"').to_string()));

    // ---- Single-instance: secondary forwards and exits, primary keeps listener ---
    // `--no-single-instance` (headless/programmatic callers) skips the
    // loopback handshake entirely so a hung primary can never block startup.
    let ipc_listener = if has("--no-single-instance") {
        None
    } else {
        single_instance::acquire_or_forward(initial_mmtl_str.clone())
    };

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
    // Bundled text fonts: Anime Ace (regular + bold + italic) as default, Augie,
    // Komika (Boo/Hand/Jam/Slick/Slim incl. bold/italic), Fuzzy Bubbles, Nanum Pen.
    // Embedded at compile time — no system install or `assets/fonts/` at runtime needed.
    // (Nanum Gothic/Myeongjo deliberately excluded: ~15.5MB saved, OS CJK fallback covers Korean.)
    .font(include_bytes!("../assets/fonts/animeace.ttf"))
    .font(include_bytes!("../assets/fonts/anime-ace.bold.ttf"))
    .font(include_bytes!("../assets/fonts/anime-ace.italic.ttf"))
    .font(include_bytes!("../assets/fonts/augie.ttf"))
    .font(include_bytes!("../assets/fonts/komika-boo.regular.ttf"))
    .font(include_bytes!("../assets/fonts/KOMIKAH_.ttf"))
    .font(include_bytes!("../assets/fonts/KOMIKAHB.ttf"))
    .font(include_bytes!("../assets/fonts/KOMIKHI_.ttf"))
    .font(include_bytes!("../assets/fonts/KOMIKHBI.ttf"))
    .font(include_bytes!("../assets/fonts/KOMIKJ__.ttf"))
    .font(include_bytes!("../assets/fonts/KOMIKJI_.ttf"))
    .font(include_bytes!("../assets/fonts/KOMIKASK.ttf"))
    .font(include_bytes!("../assets/fonts/KOMIKSKI.ttf"))
    .font(include_bytes!("../assets/fonts/KOMIKASL.ttf"))
    .font(include_bytes!("../assets/fonts/KOMIKSLI.ttf"))
    .font(include_bytes!("../assets/fonts/FuzzyBubbles-Regular.ttf"))
    .font(include_bytes!("../assets/fonts/FuzzyBubbles-Bold.ttf"))
    .font(include_bytes!("../assets/fonts/NanumPenScript-Regular.ttf"))
    .title("EasyScanlate")
    .theme(|app: &app::App| app.theme())
    .subscription(app::subscription)
    .run()
    .map_err(|e| {
        // Non-panic fatal (window/GPU startup): same native panic panel.
        crash::report_iced_error(&e.to_string());
        e
    })
}
