//! Windows resources of the executable (inkboard-mcp.exe): icon and version information (Explorer properties, installers, SmartScreen).
//! No effect on other platforms.

use std::fmt::Write as _;

fn main() {
    println!("cargo:rerun-if-changed=../inkboard-app/assets/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows") {
        let dir = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
        let out = std::env::var("OUT_DIR").expect("cargo sets OUT_DIR");
        let icon = format!("{dir}/../inkboard-app/assets/icon.ico").replace('\\', "/");
        let rc = std::path::Path::new(&out).join("inkboard.rc");
        let manifest = std::path::Path::new(&out).join("inkboard.manifest");
        let _ = std::fs::write(&manifest, MANIFEST);
        let _ = std::fs::write(&rc, resource_script(&icon, &manifest.to_string_lossy().replace('\\', "/"), "Inkboard MCP server", "inkboard-mcp.exe"));
        // a missing resource compiler (rc.exe) must not prevent compilation: the resources are then omitted
        let _ = embed_resource::compile(&rc, embed_resource::NONE);
    }
}

/// `.rc` script with the icon and a `VERSIONINFO` block built from the package version.
fn resource_script(icon: &str, manifest: &str, description: &str, original: &str) -> String {
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let nums: Vec<u32> = version.split(['.', '-']).take(3).map(|p| p.parse().unwrap_or(0)).collect();
    let (a, b, c) = (nums.first().copied().unwrap_or(0), nums.get(1).copied().unwrap_or(0), nums.get(2).copied().unwrap_or(0));
    let mut s = String::new();
    let _ = writeln!(s, "1 ICON \"{icon}\"");
    let _ = writeln!(s, "1 24 \"{manifest}\"");
    let _ = write!(
        s,
        "1 VERSIONINFO\nFILEVERSION {a},{b},{c},0\nPRODUCTVERSION {a},{b},{c},0\nFILEOS 0x40004\nFILETYPE 0x1\nBEGIN\n BLOCK \"StringFileInfo\"\n BEGIN\n  BLOCK \"040904B0\"\n  BEGIN\n   VALUE \"CompanyName\", \"The Inkboard contributors\"\n   VALUE \"FileDescription\", \"{description}\"\n   VALUE \"FileVersion\", \"{version}\"\n   VALUE \"InternalName\", \"{original}\"\n   VALUE \"LegalCopyright\", \"Copyright (c) 2026 The Inkboard contributors. MIT OR Apache-2.0\"\n   VALUE \"OriginalFilename\", \"{original}\"\n   VALUE \"ProductName\", \"Inkboard\"\n   VALUE \"ProductVersion\", \"{version}\"\n  END\n END\n BLOCK \"VarFileInfo\"\n BEGIN\n  VALUE \"Translation\", 0x409, 1200\n END\nEND\n"
    );
    s
}

/// Application manifest: runs as the invoking user (no elevation), UTF-8 and long paths, per-monitor DPI, modern common controls.
const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security><requestedPrivileges><requestedExecutionLevel level="asInvoker" uiAccess="false"/></requestedPrivileges></security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <activeCodePage xmlns="http://schemas.microsoft.com/SMI/2019/WindowsSettings">UTF-8</activeCodePage>
      <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
    </windowsSettings>
  </application>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
</assembly>
"#;
