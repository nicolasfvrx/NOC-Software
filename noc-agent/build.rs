// Build-host local time, embedded in the binary. Mirrors noc-display's build.rs.
// No `cargo:rerun-if-changed` on purpose: Cargo's default tracking re-runs this
// (and therefore refreshes the timestamp) whenever project files change.
fn main() {
    let now = chrono::Local::now();
    println!(
        "cargo:rustc-env=NOC_AGENT_BUILD_TIME={}",
        now.format("%d/%m/%y %H:%M")
    );
}
