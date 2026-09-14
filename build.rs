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
        // App manifest: required for comctl32 v6 (TaskDialog used by rfd's
        // `common-controls-v6` message dialogs, e.g. the crash reporter with
        // its custom "Report issue" button). Without this dependency the
        // loader resolves comctl32 v5, which has no TaskDialogIndirect.
        // Side effect: v6 visual styles for native dialogs app-wide.
        res.set_manifest(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity version="1.0.0.0" processorArchitecture="*" name="Liie.EasyScanlate" type="win32"/>
  <description>EasyScanlate — Manga Translation Tool</description>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
</assembly>"#,
        );
        if let Err(e) = res.compile() {
            eprintln!("winres compile failed: {e}");
        }
    }
}
