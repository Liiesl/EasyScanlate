//! Single-instance IPC via localhost TCP.
//! Primary instance binds `127.0.0.1:PORT`; secondary connects, sends `.mmtl`
//! path(s) newline-delimited, waits for `OK`, then exits. Primary polls the
//! listener from `App::subscription` (`Message::IpcPoll`) and opens received
//! files.
//!
//! Port is stable across releases so that old + new builds still
//! single-instance together. Chosen high and unlikely to collide.
//! HKCU file-assoc ProgID is `EasyScanlate.MMTLFile` (reused).

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::Duration;

/// Stable localhost port for single-instance handshake.
pub const SINGLE_INSTANCE_PORT: u16 = 34763;

/// Seconds secondary waits for primary to accept.
const CONNECT_TIMEOUT: Duration = Duration::from_millis(1200);
const READ_TIMEOUT: Duration = Duration::from_millis(600);

/// Holds the primary's `TcpListener`. Kept inside `App` and polled via
/// `Message::IpcPoll`. `None` on secondary (which exits before `App` exists).
pub struct Listener {
    inner: TcpListener,
}

impl Listener {
    fn new(inner: TcpListener) -> Self {
        Self { inner }
    }

    /// Non-blocking drain of all pending connections. Returns the paths
    /// (trimmed, de-quoted) sent by secondaries; empty string means "just
    /// focus the existing window".
    pub fn poll(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        loop {
            match self.inner.accept() {
                Ok((mut stream, _addr)) => {
                    // Read until newline or timeout/EOF.
                    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
                    let mut buf = Vec::new();
                    let mut tmp = [0u8; 4096];
                    // Try to read up to 8 KiB or until newline.
                    let mut total = 0usize;
                    loop {
                        match stream.read(&mut tmp) {
                            Ok(0) => break,
                            Ok(n) => {
                                buf.extend_from_slice(&tmp[..n]);
                                total += n;
                                if buf.contains(&b'\n') || total > 8192 {
                                    break;
                                }
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock
                                || e.kind() == std::io::ErrorKind::TimedOut =>
                            {
                                break;
                            }
                            Err(_) => break,
                        }
                        if total == 0 {
                            break;
                        }
                    }
                    let text = String::from_utf8_lossy(&buf).trim().to_string();
                    if text.is_empty() {
                        out.push(String::new());
                    } else {
                        for line in text.lines() {
                            let t = line.trim().trim_matches('"').trim();
                            if !t.is_empty() {
                                out.push(t.to_string());
                            }
                        }
                        if out.is_empty() && !text.trim().is_empty() {
                            // Fallback: treat whole payload as one path (no newline).
                            let t = text.trim().trim_matches('"').trim().to_string();
                            if !t.is_empty() {
                                out.push(t);
                            }
                        }
                    }
                    // Ack so secondary can exit promptly.
                    let _ = stream.write_all(b"OK");
                    let _ = stream.flush();
                    bring_to_front_best_effort();
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }
        out
    }
}

fn addr() -> SocketAddr {
    format!("127.0.0.1:{}", SINGLE_INSTANCE_PORT)
        .parse()
        .expect("single instance addr must parse")
}

/// Try to become primary. If another instance is already bound, forward
/// `initial_mmtl` (if any) to it and return `None` (caller should exit).
/// Otherwise return `Some(Listener)` for the primary to keep.
///
/// `initial_mmtl` should be the raw CLI path string (already trimmed of
/// surrounding quotes) or `None` if no `.mmtl` was passed.
pub fn acquire_or_forward(initial_mmtl: Option<String>) -> Option<Listener> {
    match TcpListener::bind(addr()) {
        Ok(l) => {
            // Make non-blocking so `poll()` never blocks the UI thread.
            if let Err(e) = l.set_nonblocking(true) {
                eprintln!("[single-instance] set_nonblocking failed: {e}");
            }
            Some(Listener::new(l))
        }
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            // Another instance is primary — forward and exit.
            let paths: Vec<String> = initial_mmtl
                .into_iter()
                .filter(|s| !s.trim().is_empty())
                .collect();
            match forward_to_primary(&paths) {
                Ok(()) => {
                    // Give primary a moment to bring window forward via our poll ack.
                }
                Err(fe) => {
                    eprintln!("[single-instance] forward failed: {fe} (primary may be hung)");
                }
            }
            // Always exit the secondary; otherwise we'd have two windows.
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("[single-instance] bind failed ({e}), running without single-instance guard");
            None
        }
    }
}

fn forward_to_primary(paths: &[String]) -> Result<(), String> {
    let mut stream = TcpStream::connect_timeout(&addr(), CONNECT_TIMEOUT)
        .map_err(|e| format!("connect: {e}"))?;
    stream
        .set_write_timeout(Some(Duration::from_millis(600)))
        .ok();
    stream
        .set_read_timeout(Some(READ_TIMEOUT))
        .ok();
    if paths.is_empty() {
        stream
            .write_all(b"\n")
            .map_err(|e| format!("write: {e}"))?;
    } else {
        for p in paths {
            // Send each path on its own line (handles spaces safely).
            stream
                .write_all(p.as_bytes())
                .map_err(|e| format!("write: {e}"))?;
            stream
                .write_all(b"\n")
                .map_err(|e| format!("write: {e}"))?;
        }
    }
    stream.flush().map_err(|e| format!("flush: {e}"))?;
    // Wait for ack; ignore failure.
    let mut ack = [0u8; 4];
    let _ = stream.read(&mut ack);
    Ok(())
}

/// Parse CLI args into the first `.mmtl` path (case-insensitive) and the
/// filtered flag list. Handles quoted paths with spaces from Explorer:
/// `EasyScanlate.exe "C:\My Projects\proj.mmtl"`.
pub fn parse_initial_mmtl(args: &[String]) -> Option<String> {
    for raw in args.iter().skip(1) {
        let t = raw.trim().trim_matches('"').trim();
        if t.is_empty() || t.starts_with('-') {
            continue;
        }
        if t.to_ascii_lowercase().ends_with(".mmtl") {
            return Some(t.to_string());
        }
    }
    None
}

/// Also collect *all* .mmtl args (for future multi-drop), but current callers
/// only need the first.
pub fn parse_all_mmtl(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for raw in args.iter().skip(1) {
        let t = raw.trim().trim_matches('"').trim();
        if t.is_empty() || t.starts_with('-') { continue; }
        if t.to_ascii_lowercase().ends_with(".mmtl") {
            out.push(t.to_string());
        }
    }
    out
}

pub fn is_mmtl_path(p: &str) -> bool {
    p.trim().trim_matches('"').to_ascii_lowercase().ends_with(".mmtl")
}

pub fn is_mmtl_pathbuf(p: &PathBuf) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("mmtl"))
        .unwrap_or(false)
}

// --- window focus helper (best-effort, Windows only) ---------------------

#[cfg(windows)]
fn bring_to_front_best_effort() {
    // Use raw Win32 so we avoid a `windows` crate dep. The window title is
    // "EasyScanlate" (see `src/main.rs:.title("EasyScanlate")`). Custom frame
    // still creates a normal top-level HWND with that title for the taskbar.
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn FindWindowW(lpClassName: *const u16, lpWindowName: *const u16) -> *mut std::ffi::c_void;
        fn SetForegroundWindow(hWnd: *mut std::ffi::c_void) -> i32;
        fn ShowWindow(hWnd: *mut std::ffi::c_void, nCmdShow: i32) -> i32;
    }
    const SW_RESTORE: i32 = 9;
    unsafe {
        let title: Vec<u16> = OsStr::new("EasyScanlate")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
        if !hwnd.is_null() {
            ShowWindow(hwnd, SW_RESTORE);
            SetForegroundWindow(hwnd);
        }
    }
}

#[cfg(not(windows))]
fn bring_to_front_best_effort() {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_skips_flags() {
        let args = vec!["easyscanlate".to_string(), "--register".to_string(), "a.mmtl".to_string()];
        assert_eq!(parse_initial_mmtl(&args), Some("a.mmtl".to_string()));
    }
    #[test]
    fn parse_quoted_spaces() {
        let args = vec!["easyscanlate".to_string(), "\"C:\\My Proj\\a.mmtl\"".to_string()];
        assert_eq!(parse_initial_mmtl(&args), Some("C:\\My Proj\\a.mmtl".to_string()));
    }
    #[test]
    fn is_mmtl_case_insensitive() {
        assert!(is_mmtl_path("a.MMTL"));
        assert!(is_mmtl_path("\"a.MmTl\""));
        assert!(!is_mmtl_path("a.png"));
    }
}
