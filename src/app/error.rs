//! Non-panic error dialogs: native `rfd` message boxes with a Copy button.
//!
//! Panics use the sync `rfd::MessageDialog` in a detached reporter child
//! (see `crate::crash` — blocking the iced/winit thread deadlocks). Every
//! other failure goes through [`show`], which runs the *sync* dialog with
//! `OkCancelCustom(Copy, OK)` inside `tokio::spawn_blocking` — never on the
//! iced thread, so the UI stays alive.
//!
//! Why sync and not `AsyncMessageDialog`? Async only supports
//! `Ok|OkCancel|YesNo|YesNoCancel` — no custom buttons. Copy-on-demand needs
//! the sync `OkCancelCustom` API (same TaskDialog path as the crash
//! reporter, needs `common-controls-v6` — already enabled).
//!
//! Flow: `show` → native dialog → `OK` dismisses (`ErrorDismissed`), `Copy
//! details` writes the full text via `iced::clipboard` and re-shows the
//! dialog (`ErrorCopyRetry`) so the user can read on and dismiss with OK.
//! Nothing is ever copied without the user pressing the button.
//!
//! Body shows details only (the title is already in the title bar); a
//! truncation note appears only when the text was actually cut.

use iced::Task;

use super::Message;

/// Hidden self-test flag: `easyscanlate --error-test` shows a sample
/// non-panic error dialog (sync, before iced starts — safe) and exits.
pub const ERROR_TEST_ARG: &str = "--error-test";
/// "Copy details" custom button label.
const COPY_BUTTON: &str = "Copy details";
/// Dismiss button label.
const DISMISS_BUTTON: &str = "OK";
/// Native dialog text must stay short; full details go to the clipboard (on
/// demand) + stderr. Mirrors `crash::MAX_DIALOG_CHARS`.
const MAX_DIALOG_CHARS: usize = 900;

/// Show a native error dialog for a non-panic failure.
///
/// Keeps the caller's `tab.status = ...` line untouched (status bar stays as
/// the persistent record); this adds the interrupt the status bar lacks.
/// Nothing touches the clipboard until the user presses `Copy details`.
///
/// Call only for terminal/user-blocking failures (load/save/open total
/// export failure, engine-build failure, ...). Per-item streaming partials
/// must stay counters-only; dialog once when `saved == 0` at finalize.
pub fn show(title: impl Into<String> + Send + 'static, details: impl ToString) -> Task<Message> {
    let title = title.into();
    let details = details.to_string();
    eprintln!("[error] {title}: {details}");
    let body = dialog_body(&title, &details);
    Task::perform(
        async move {
            let t = title.clone();
            let b = body.clone();
            // Sync dialog off the UI thread: blocks a tokio worker, never
            // iced/winit (same deadlock argument as `crash::spawn_reporter`).
            // `catch_unwind`: the dialog must never panic the task.
            let choice = tokio::task::spawn_blocking(move || {
                std::panic::catch_unwind(|| {
                    rfd::MessageDialog::new()
                        .set_title(&t)
                        .set_description(&b)
                        .set_level(rfd::MessageLevel::Error)
                        .set_buttons(rfd::MessageButtons::OkCancelCustom(
                            COPY_BUTTON.to_string(),
                            DISMISS_BUTTON.to_string(),
                        ))
                        .show()
                })
            })
            .await;
            let wants_copy = matches!(
                choice,
                Ok(Ok(rfd::MessageDialogResult::Custom(ref label))) if label == COPY_BUTTON
            );
            if wants_copy {
                Message::ErrorCopyRetry { title, details }
            } else {
                Message::ErrorDismissed
            }
        },
        |msg| msg,
    )
}

/// Rebuildable full text for the clipboard: `"{title}\n\n{details}"`.
pub(crate) fn full_text(title: &str, details: &str) -> String {
    if details.trim().is_empty() {
        title.to_string()
    } else {
        format!("{title}\n\n{details}")
    }
}

fn dialog_body(title: &str, details: &str) -> String {
    let base = if details.trim().is_empty() {
        title.to_string()
    } else {
        details.to_string()
    };
    if base.chars().count() <= MAX_DIALOG_CHARS {
        return base;
    }
    format!(
        "{}\n\nTruncated — “{COPY_BUTTON}” has the rest.",
        truncate_chars(&base, MAX_DIALOG_CHARS)
    )
}

/// Early CLI handling for `--error-test`. Call in `main()` after the crash
/// reporter pre-check, before iced starts (sync dialog is safe there — no
/// event loop to deadlock). Returns `Some` when the flag was present.
pub fn run_error_test_if_requested(args: &[String]) -> Option<iced::Result> {
    if !args.iter().any(|a| a == ERROR_TEST_ARG) {
        return None;
    }
    let title = "Sample error";
    let body = dialog_body(
        title,
        "This is what a failure dialog looks like. Real details appear here.",
    );
    let choice = rfd::MessageDialog::new()
        .set_title(title)
        .set_description(&body)
        .set_level(rfd::MessageLevel::Error)
        .set_buttons(rfd::MessageButtons::OkCancelCustom(
            COPY_BUTTON.to_string(),
            DISMISS_BUTTON.to_string(),
        ))
        .show();
    if matches!(
        choice,
        rfd::MessageDialogResult::Custom(ref label) if label == COPY_BUTTON
    ) {
        eprintln!("[error-test] Copy pressed (no clipboard before iced starts)");
    }
    Some(Ok(()))
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{truncated}…")
}
