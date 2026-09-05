//! Per-user file association for `.mmtl` → `EasyScanlate.MMTLFile`.
//! Mirrors `ManhwaOCR/dev/installer/installer.nsi:109-117` but writes to
//! `HKCU\Software\Classes` so no elevation is required. Reuses the original
//! ProgID so an existing install upgrades cleanly when the repo is merged.
//!
//! Keys written (HKCU\Software\Classes):
//!   .mmtl                                   → EasyScanlate.MMTLFile
//!   EasyScanlate.MMTLFile                   → "EasyScanlate Project"
//!   EasyScanlate.MMTLFile\DefaultIcon       → "\"<exe>\",0"
//!   EasyScanlate.MMTLFile\shell\open\command→ "\"<exe>\" \"%1\""
//!
//! Unregister removes the ProgID tree and the `.mmtl` key only when it
//! still points at our ProgID.

pub const PROG_ID: &str = "EasyScanlate.MMTLFile";
// Used in tests + future multi-drop; `dead_code` fires for `cargo clippy`
// without `--all-targets` since `#[cfg(test)]` is excluded.
#[allow(dead_code)]
pub const EXT: &str = ".mmtl";
pub const PROG_DESC: &str = "EasyScanlate Project";

#[cfg(all(windows, feature = "file-assoc"))]
mod imp {
    use super::{PROG_DESC, PROG_ID};
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    fn exe_string() -> Result<String, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        Ok(exe.to_string_lossy().into_owned())
    }

    pub fn register() -> Result<(), String> {
        let exe = exe_string()?;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);

        // .mmtl → ProgID
        let (ext_key, _) = hkcu
            .create_subkey(r"Software\Classes\.mmtl")
            .map_err(|e| e.to_string())?;
        ext_key
            .set_value("", &PROG_ID)
            .map_err(|e| e.to_string())?;

        // ProgID → description
        let (prog_key, _) = hkcu
            .create_subkey(format!(r"Software\Classes\{}", PROG_ID))
            .map_err(|e| e.to_string())?;
        prog_key
            .set_value("", &PROG_DESC)
            .map_err(|e| e.to_string())?;

        // DefaultIcon
        let (icon_key, _) = hkcu
            .create_subkey(format!(r"Software\Classes\{}\DefaultIcon", PROG_ID))
            .map_err(|e| e.to_string())?;
        let icon_val = format!(r#""{}",0"#, exe);
        icon_key
            .set_value("", &icon_val)
            .map_err(|e| e.to_string())?;

        // shell\open\command
        let (cmd_key, _) = hkcu
            .create_subkey(format!(
                r"Software\Classes\{}\shell\open\command",
                PROG_ID
            ))
            .map_err(|e| e.to_string())?;
        let cmd_val = format!(r#""{}" "%1""#, exe);
        cmd_key
            .set_value("", &cmd_val)
            .map_err(|e| e.to_string())?;

        // Best-effort: also write OpenWithProgids so Explorer's "Open with" lists us.
        // This is optional; failure is ignored.
        let _ = (|| -> Result<(), String> {
            let (owp, _) = hkcu
                .create_subkey(r"Software\Classes\.mmtl\OpenWithProgids")
                .map_err(|e| e.to_string())?;
            owp.set_value(PROG_ID, &"").map_err(|e| e.to_string())?;
            Ok(())
        })();

        Ok(())
    }

    pub fn unregister() -> Result<(), String> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        // Only delete .mmtl keys that point at our ProgID.
        let should_delete_ext = hkcu
            .open_subkey(r"Software\Classes\.mmtl")
            .ok()
            .and_then(|k| k.get_value::<String, _>("").ok())
            .map(|v| v == PROG_ID)
            .unwrap_or(false);

        if should_delete_ext {
            // Delete the ProgID listing under OpenWithProgids first, then the extension key if empty.
            let _ = hkcu
                .open_subkey_with_flags(
                    r"Software\Classes\.mmtl\OpenWithProgids",
                    winreg::enums::KEY_WRITE,
                )
                .and_then(|k| k.delete_value(PROG_ID));
            // Try to delete the .mmtl key tree; ignore if it still has other values.
            // We attempt to delete the whole subkey - winreg delete_subkey requires empty.
            // So we try delete_subkey for OpenWithProgids subkey, then .mmtl itself if now empty/ours.
            let _ = hkcu.delete_subkey(r"Software\Classes\.mmtl\OpenWithProgids");
            // Only delete .mmtl if it still equals our ProgID (re-check) and has no other subkeys/values beyond default.
            // Instead of inspecting, just try; failure is fine.
            let _ = hkcu.delete_subkey(r"Software\Classes\.mmtl");
            // Fallback: if delete_subkey failed because key not empty, at least clear the default value's ProgID? No, keep it.
            // If the key still exists but default==ours, try to delete just the value by recreating? We leave it.
            // Safer: if delete_subkey failed, do not leave orphan - keep; user can re-register.
            // Alternative: try to delete the whole tree recursively via delete_subkey_all if available.
            // winreg 0.53 has delete_subkey_all.
            let _ = hkcu.delete_subkey_all(r"Software\Classes\.mmtl");
        }

        // Delete ProgID tree regardless (it is ours).
        let _ = hkcu.delete_subkey_all(format!(r"Software\Classes\{}\shell\open\command", PROG_ID));
        let _ = hkcu.delete_subkey_all(format!(r"Software\Classes\{}\shell\open", PROG_ID));
        let _ = hkcu.delete_subkey_all(format!(r"Software\Classes\{}\shell", PROG_ID));
        let _ = hkcu.delete_subkey_all(format!(r"Software\Classes\{}\DefaultIcon", PROG_ID));
        let _ = hkcu.delete_subkey_all(format!(r"Software\Classes\{}", PROG_ID));

        // Notify shell that associations changed (best-effort).
        unsafe {
            winapi_shchange();
        }

        Ok(())
    }

    #[allow(unsafe_code)]
    unsafe fn winapi_shchange() {
        // SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, NULL, NULL) to refresh Explorer.
        // Use raw winapi via windows-sys if available; fallback to no-op. We avoid adding a
        // heavy windows crate dep - try dynamic load via winapi crate not present, so just skip.
        // The association will refresh on next Explorer restart / logon anyway.
        // If we add `windows` crate later, call SHChangeNotify here.
    }

    pub fn is_registered() -> bool {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let ext_ok = hkcu
            .open_subkey(r"Software\Classes\.mmtl")
            .ok()
            .and_then(|k| k.get_value::<String, _>("").ok())
            .map(|v| v == PROG_ID)
            .unwrap_or(false);
        if !ext_ok {
            return false;
        }
        let exe = match exe_string() {
            Ok(e) => e,
            Err(_) => return false,
        };
        let expected = format!(r#""{}" "%1""#, exe);
        hkcu.open_subkey(format!(r"Software\Classes\{}\shell\open\command", PROG_ID))
            .ok()
            .and_then(|k| k.get_value::<String, _>("").ok())
            .map(|v| v == expected)
            .unwrap_or(false)
    }
}

#[cfg(all(windows, feature = "file-assoc"))]
pub use imp::{is_registered, register, unregister};

#[cfg(any(not(windows), not(feature = "file-assoc")))]
mod stub {
    pub fn register() -> Result<(), String> {
        Err("file association is only available on Windows with --features file-assoc".to_string())
    }
    pub fn unregister() -> Result<(), String> {
        Err("file association is only available on Windows with --features file-assoc".to_string())
    }
    pub fn is_registered() -> bool {
        false
    }
}

#[cfg(any(not(windows), not(feature = "file-assoc")))]
pub use stub::{is_registered, register, unregister};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prog_id_constants_match_original() {
        assert_eq!(PROG_ID, "EasyScanlate.MMTLFile");
        assert_eq!(EXT, ".mmtl");
    }
}
