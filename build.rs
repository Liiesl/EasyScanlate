fn main() {
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/app_icon.ico");
        // Version: CI injects VELPK_VERSION / VPK_PACK_VERSION / VERSION (stripped tag, e.g. 0.1.0).
        // Fallback is Cargo package version so local cargo build still shows 0.1.0.
        let version = std::env::var("VELPK_VERSION")
            .or_else(|_| std::env::var("VPK_PACK_VERSION"))
            .or_else(|_| std::env::var("VERSION"))
            .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string());
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
