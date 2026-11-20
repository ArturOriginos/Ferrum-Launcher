// Встраивает иконку и сведения об авторе в .exe на Windows.
// Эти сведения видны в свойствах файла и помогают антивирусам и SmartScreen опознать программу.
fn main() {
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("CompanyName", "ArturOriginos");
        res.set("LegalCopyright", "Copyright (c) 2026 ArturOriginos");
        res.set("FileDescription", "Ferrum Launcher - Minecraft launcher");
        res.set("ProductName", "Ferrum Launcher");
        res.set("InternalName", "ferrum-launcher");
        res.set("OriginalFilename", "ferrum-launcher.exe");
        res.set("Comments", "Open source: https://github.com/ArturOriginos/ferrum-launcher");
        res.set_manifest(
            r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
</assembly>"#,
        );
        if let Err(e) = res.compile() {
            println!("cargo:warning=не удалось встроить ресурсы Windows: {e}");
        }
    }
}
