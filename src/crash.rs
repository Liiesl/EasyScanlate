//! Panic panel: native crash dialog with GitHub issue-form reporting.
//!
//! Release builds set `windows_subsystem = "windows"` (see `main.rs`), so a
//! panic would otherwise vanish silently.
//!
//! Design (deliberately split across two processes):
//! - The panic hook [`install_hook`] does only fast, non-blocking work on the
//!   panicking thread: capture payload + location + backtrace, write a crash
//!   `.log` to the temp dir, and spawn a detached reporter child
//!   (`easyscanlate --crash-report <log>`). It never shows UI itself —
//!   blocking a `MessageBox` inside the hook deadlocks when the panicking
//!   thread is the iced/winit UI thread.
//! - The reporter child ([`run_reporter_if_requested`], handled before
//!   Velopack / single-instance / iced in `main`) reads the log, shows a
//!   minimal native dialog (`rfd`), and on "Yes" opens a prefilled
//!   `bug_report.yml` issue form in the browser.
//!
//! No auto-upload: GitHub issues need user auth, so we only open a prefilled
//! form URL (`?template=bug_report.yml&<id>=...` — `id` is the canonical
//! prefill key for issue forms) via `open::that`. No new dependencies:
//! only `std` + the already-present `rfd` and `open` crates.

use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, Ordering};

const ISSUE_BASE: &str = "https://github.com/Liiesl/EasyScanlate/issues/new";
const ISSUE_TEMPLATE: &str = "bug_report.yml";
/// CLI flag for the detached reporter child. Handled first in `main()`.
pub const CRASH_REPORT_ARG: &str = "--crash-report";
/// Hidden self-test flag: `easyscanlate --crash-test` panics on purpose so
/// the hook → log → reporter dialog path can be exercised manually.
pub const CRASH_TEST_ARG: &str = "--crash-test";
/// Marker separating headers from the backtrace in the crash log. The
/// reporter parses the log back with [`CrashReport::from_log_text`].
const BACKTRACE_MARKER: &str = "\nbacktrace:\n";
/// Browsers / GitHub reject very long URLs; keep the whole URL under this.
const MAX_URL_LEN: usize = 8000;
/// Traceback is the longest field — cap it first so the URL stays valid.
/// Full backtrace is always preserved in the local crash `.log`.
const MAX_TRACEBACK_CHARS: usize = 4000;
/// Native dialog text must stay short; full details live in the log file.
const MAX_DIALOG_CHARS: usize = 900;
/// Traceback excerpt shown in the dialog body (full backtrace is in the log).
const MAX_DIALOG_TRACEBACK_CHARS: usize = 600;
/// Crash dialog buttons: "Report issue" opens the prefilled GitHub form,
/// "OK" just dismisses.
const REPORT_BUTTON: &str = "Report issue";
const DISMISS_BUTTON: &str = "OK";

/// Guard against recursive panics inside the hook itself (hook work must
/// never spawn a second reporter).
static IN_HOOK: AtomicBool = AtomicBool::new(false);

/// Install the panic hook. Call once in `main()` after the
/// `--crash-report` early-exit (so the reporter child never re-spawns)
/// and before everything else, so every later panic is caught.
pub fn install_hook() {
    std::panic::set_hook(Box::new(|info| {
        // Only one reporter ever: a nested panic just logs to stderr.
        if IN_HOOK.swap(true, Ordering::SeqCst) {
            eprintln!("[crash] recursive panic while handling: {info}");
            return;
        }
        handle_panic_info(info);
    }));
}

/// Early CLI handling for the detached reporter child. Call at the very top
/// of `main()`, before Velopack / single-instance / iced. Returns `Some`
/// when this process IS the reporter (caller must return the result
/// immediately); `None` for normal startup.
pub fn run_reporter_if_requested(args: &[String]) -> Option<iced::Result> {
    let pos = args.iter().position(|a| a == CRASH_REPORT_ARG)?;
    let log_path = args.get(pos + 1).cloned().unwrap_or_default();
    Some(run_reporter(&log_path))
}

/// Non-panic fatal path: `iced::application(...).run()` returned `Err`
/// (e.g. window/GPU startup failure). The event loop is already dead, so
/// showing the dialog directly here is safe (no hook context).
pub fn report_iced_error(err: &str) {
    let report = CrashReport {
        message: format!("Failed to start: {err}"),
        location: "n/a".to_string(),
        thread: "main".to_string(),
        backtrace: std::backtrace::Backtrace::force_capture().to_string(),
    };
    let log_path = write_crash_log(&report.full_text());
    show_dialog(&report, log_path.as_deref());
}

// ---------------------------------------------------------------------------
// Panic-hook side (fast, non-blocking — never shows UI here)
// ---------------------------------------------------------------------------

fn handle_panic_info(info: &std::panic::PanicHookInfo<'_>) {
    // Everything is best-effort: the hook must never panic itself.
    // (`AssertUnwindSafe`: the hook closure only reads the panic info and
    // writes a file / spawns a child — no shared mutable state involved.)
    let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let report = CrashReport::from_panic_info(info);
        let log_path = write_crash_log(&report.full_text());
        if let Some(path) = log_path.as_deref() {
            spawn_reporter(path);
        }
        // `eprintln!` is invisible in release (no console) but helps debug runs.
        eprintln!("[crash] {}", report.short_message());
        if let Some(path) = log_path.as_deref() {
            eprintln!("[crash] log saved to: {path}");
        }
    }));
}

/// Spawn a detached `easyscanlate --crash-report <log>` child that owns the
/// dialog. Fire-and-forget: never wait, never propagate errors.
fn spawn_reporter(log_path: &str) {
    let _ = std::panic::catch_unwind(|| {
        let exe = std::env::current_exe()?;
        std::process::Command::new(exe)
            .arg(CRASH_REPORT_ARG)
            .arg(log_path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        Ok::<(), std::io::Error>(())
    });
}

// ---------------------------------------------------------------------------
// Reporter-child side (normal process context — may block on UI)
// ---------------------------------------------------------------------------

fn run_reporter(log_path: &str) -> iced::Result {
    let text = std::fs::read_to_string(log_path).unwrap_or_else(|_| {
        format!(
            "EasyScanlate crash report\nversion: {}\nlocation: unknown\nmessage: (could not read log: {log_path})\n",
            app_version(),
        )
    });
    let report = CrashReport::from_log_text(&text);
    show_dialog(
        &report,
        if log_path.is_empty() {
            None
        } else {
            Some(log_path)
        },
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Report model
// ---------------------------------------------------------------------------

struct CrashReport {
    message: String,
    location: String,
    thread: String,
    backtrace: String,
}

impl CrashReport {
    fn from_panic_info(info: &std::panic::PanicHookInfo<'_>) -> Self {
        let payload = info.payload();
        let message = if let Some(s) = payload.downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic payload".to_string()
        };
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown location".to_string());
        let thread = std::thread::current()
            .name()
            .unwrap_or("<unnamed>")
            .to_string();
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        Self {
            message,
            location,
            thread,
            backtrace,
        }
    }

    /// Re-parse a log written by [`CrashReport::full_text`]. All fields fall
    /// back to placeholders so a corrupt log still yields a dialog.
    fn from_log_text(text: &str) -> Self {
        let (head, backtrace) = match text.split_once(BACKTRACE_MARKER) {
            Some((h, b)) => (h, b.trim_end().to_string()),
            None => (text, String::new()),
        };
        let field = |name: &str| {
            head.lines()
                .find_map(|l| l.strip_prefix(name).map(|v| v.trim().to_string()))
        };
        Self {
            message: field("message: ").unwrap_or_else(|| "Unknown error".to_string()),
            location: field("location: ").unwrap_or_else(|| "unknown".to_string()),
            thread: field("thread: ").unwrap_or_else(|| "unknown".to_string()),
            backtrace,
        }
    }

    /// First line of the panic, capped for dialog titles / issue titles.
    fn short_message(&self) -> String {
        let first = self.message.lines().next().unwrap_or("Unknown error").trim();
        truncate_chars(first, 200)
    }

    fn full_text(&self) -> String {
        format!(
            "EasyScanlate crash report\n\
             version: {}\n\
             os: {} ({})\n\
             arch: {}\n\
             thread: {}\n\
             location: {}\n\
             message: {}\n\
             {BACKTRACE_MARKER}{}\n",
            app_version(),
            std::env::consts::OS,
            std::env::consts::ARCH,
            std::env::consts::ARCH,
            self.thread,
            self.location,
            self.message,
            self.backtrace,
        )
    }
}

// ---------------------------------------------------------------------------
// Dialog + log + issue URL (best-effort side effects)
// ---------------------------------------------------------------------------

fn show_dialog(report: &CrashReport, log_path: Option<&str>) {
    let log_line = log_path.unwrap_or("temp dir (write failed)");
    let body = format!(
        "EasyScanlate crashed unexpectedly.\n\n{}\n\nCrash log saved to:\n{}\n\n{}",
        truncate_chars(&report.short_message(), MAX_DIALOG_CHARS),
        log_line,
        truncate_chars(report.backtrace.trim(), MAX_DIALOG_TRACEBACK_CHARS),
    );
    // `rfd` MessageBoxW is sync/native. This runs either in the reporter
    // child or after iced already returned — never on a panicking iced
    // thread — so no event-loop deadlock. Best-effort: never propagate.
    // Custom labels need rfd's `common-controls-v6` feature (TaskDialog);
    // without it this falls back to stock OK/Cancel and returns `Ok`/`Cancel`
    // instead of `Custom`, so only `Custom(REPORT_BUTTON)` reports.
    let choice = std::panic::catch_unwind(|| {
        rfd::MessageDialog::new()
            .set_title("EasyScanlate crashed")
            .set_description(&body)
            .set_level(rfd::MessageLevel::Error)
            .set_buttons(rfd::MessageButtons::OkCancelCustom(
                REPORT_BUTTON.to_string(),
                DISMISS_BUTTON.to_string(),
            ))
            .show()
    });
    let wants_report = matches!(
        choice,
        Ok(rfd::MessageDialogResult::Custom(ref label)) if label == REPORT_BUTTON
    );
    if wants_report {
        let url = build_issue_url(report);
        let _ = open::that(&url);
    }
}

/// Write the full report to `%TEMP%/easyscanlate-crash-<unix_secs>.log`.
/// Returns the path as a string for display, or `None` on any failure.
fn write_crash_log(full_text: &str) -> Option<String> {
    let result = std::panic::catch_unwind(|| {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!("easyscanlate-crash-{secs}.log"));
        std::fs::write(&path, full_text)?;
        Ok::<String, std::io::Error>(path.to_string_lossy().into_owned())
    });
    result.ok().and_then(|r| r.ok())
}

// ---------------------------------------------------------------------------
// Prefilled `bug_report.yml` issue-form URL
// ---------------------------------------------------------------------------

fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

fn running_mode() -> &'static str {
    if cfg!(debug_assertions) {
        "Self-compiled (Cargo)"
    } else {
        "Precompiled (Release)"
    }
}

/// Human-readable OS label matching the `os` dropdown options in
/// `.github/ISSUE_TEMPLATE/bug_report.yml`, so the user can set the dropdown
/// to the value shown in `additional-info`.
fn map_os() -> &'static str {
    match std::env::consts::OS {
        "windows" => "Windows 11",
        "macos" => "macOS 14+",
        "linux" => "Linux Ubuntu 22.04+",
        _ => "Other",
    }
}

/// Human-readable arch label matching the `arch` dropdown options (same
/// reason as [`map_os`]).
fn map_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "ARM64",
        "arm" => "ARM",
        _ => "Unknown",
    }
}

fn build_issue_url(report: &CrashReport) -> String {
    // `id` keys are the canonical prefill keys for issue forms
    // (see `.github/ISSUE_TEMPLATE/bug_report.yml`). NOTE: only `input` and
    // `textarea` fields accept prefills — `dropdown` params (`os`, `arch`,
    // `running-mode`, `error-type`) are silently ignored by GitHub and show
    // "None", so those auto-detected values are embedded in `additional-info`
    // (a textarea) instead. Verified against the live form.
    let traceback = truncate_chars(&report.backtrace, MAX_TRACEBACK_CHARS);
    let mut fields: Vec<(&str, String)> = vec![
        ("template", ISSUE_TEMPLATE.to_string()),
        (
            "title",
            format!("[BUG] panic: {}", truncate_chars(&report.short_message(), 80)),
        ),
        (
            "error-details",
            format!("{}\n\nat {}", report.message, report.location),
        ),
        ("traceback", format!("```\n{traceback}\n```")),
        (
            "os-details",
            format!("{} / {}", std::env::consts::OS, std::env::consts::ARCH),
        ),
        ("app-version", app_version()),
        (
            "additional-info",
            format!(
                "Auto-detected (please set the dropdowns above to match): OS = {} [{} / {}], Architecture = {}, Running Mode = {}, Severity = critical.",
                map_os(),
                std::env::consts::OS,
                std::env::consts::ARCH,
                map_arch(),
                running_mode(),
            ),
        ),
        // NOTE: `context` ("What were you doing when the error occurred?")
        // is intentionally NOT prefilled — only the user can answer that.
    ];
    // Shrink the longest field until the whole URL fits browser limits.
    let mut url = encode_issue_url(&fields);
    while url.len() > MAX_URL_LEN {
        let Some(entry) = fields.iter_mut().find(|(k, _)| *k == "traceback") else {
            break;
        };
        if entry.1.len() < 256 {
            break;
        }
        let keep = entry.1.len() / 2;
        entry.1.truncate(keep);
        url = encode_issue_url(&fields);
    }
    url
}

fn encode_issue_url(fields: &[(&str, String)]) -> String {
    let mut out = String::from(ISSUE_BASE);
    out.push('?');
    for (i, (k, v)) in fields.iter().enumerate() {
        if i > 0 {
            out.push('&');
        }
        out.push_str(k);
        out.push('=');
        out.push_str(&percent_encode(v));
    }
    out
}

/// Percent-encode a query value (RFC 3986 unreserved set left as-is).
/// Local helper so the root crate needs no extra `url` dependency.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{b:02X}"));
            }
        }
    }
    out
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{truncated}…")
}
