fn main() {
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rerun-if-changed=resources/ferric.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("resources/ferric.ico");
        res.set_language(0x0409); // en-US Unicode (040904b0)
        res.set("ProductName", "Ferric");
        res.set("FileDescription", "Ferric");
        res.set("InternalName", "ferric");
        res.set("OriginalFilename", "ferric.exe");
        res.set("CompanyName", "Raflyf");
        res.set("LegalCopyright", "Copyright (c) 2026 Raflyf");
        let manifest = format!(
            r#"
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0" xmlns:asmv3="urn:schemas-microsoft-com:asm.v3">
  <assemblyIdentity version="{ver}.0" processorArchitecture="amd64" name="Raflyf.Ferric" type="win32"/>
  <description>Ferric</description>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <!-- Windows 10 and Windows 11 -->
      <supportedOS Id="{{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}}"/>
    </application>
  </compatibility>
  <asmv3:application>
    <asmv3:windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2, PerMonitor</dpiAwareness>
      <activeCodePage xmlns="http://schemas.microsoft.com/SMI/2019/WindowsSettings">UTF-8</activeCodePage>
      <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
    </asmv3:windowsSettings>
  </asmv3:application>
</assembly>
"#,
            ver = env!("CARGO_PKG_VERSION"),
        );
        res.set_manifest(&manifest);
        res.compile()
            .expect("winresource failed to compile Windows PE icon, version info, and manifest");
    }
}
