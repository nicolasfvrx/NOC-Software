use std::process::Command;
mod windows_metadata;

fn main() {
    windows_metadata::embed("NOC Display");
    // Build-host local time, embedded in the EXE. No PowerShell dependency at runtime.
    // Cargo's default package tracking refreshes this when project files change.
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
    println!(
        "cargo:rustc-env=DISPLAYCLIENT_BUILD_TIME={}",
        timestamp.trim()
    );
}
