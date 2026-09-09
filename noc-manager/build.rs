mod windows_metadata;
use std::process::Command;

fn main() {
    windows_metadata::embed("NOC Manager");
    // Build-host local time, embedded in the EXE. Mirrors noc-display's build.rs.
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "(Get-Date).ToString('dd/MM/yy HH:mm', [Globalization.CultureInfo]::InvariantCulture)",
        ])
        .output()
        .expect("Cannot obtain build timestamp from Windows PowerShell");
    assert!(output.status.success(), "Cannot obtain build timestamp");
    let timestamp = String::from_utf8(output.stdout).expect("Invalid build timestamp");
    println!("cargo:rustc-env=NOC_MANAGER_BUILD_TIME={}", timestamp.trim());
}
