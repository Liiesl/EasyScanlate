fn main() {
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("app_icon.ico");
        // Pull version from Cargo.toml package version so EXE shows correct FileVersion.
        let version = env!("CARGO_PKG_VERSION");
        // winres expects "x,y,z,w" — pad to 4 parts.
        let mut parts: Vec<&str> = version.split('.').collect();
        while parts.len() < 4 {
            parts.push("0");
        }
        let ver = parts[..4].join(",");
        res.set("FileVersion", &ver);
        res.set("ProductVersion", &ver);
        res.set("ProductName", "EasyScanlate");
        res.set("FileDescription", "EasyScanlate — Manga Translation Tool");
        res.set("CompanyName", "Liie");
        res.set("LegalCopyright", "© Liie");
        res.set_language(0x0409); // en-US
        if let Err(e) = res.compile() {
            eprintln!("winres compile failed: {e}");
        }
    }
}
