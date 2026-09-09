//! Suite version, independent of each app's own `Cargo.toml` `version`
//! (those stay independent on purpose — see the root README). This is
//! what's shown on screen and in `--version`: the exact tag when built via
//! the release packaging scripts (`NOC_SUITE_VERSION`, set by
//! scripts/build-windows.ps1 / scripts/build-linux.sh), or the repo-root
//! `VERSION` file otherwise (e.g. a local `cargo build`).
//!
//! Deliberately its own tiny file rather than pulled from `shared/update.rs`
//! (the disabled auto-updater) so apps don't drag that module's dependencies
//! in just to display a version string.
pub fn suite_version() -> &'static str {
    option_env!("NOC_SUITE_VERSION")
        .unwrap_or(include_str!("../VERSION"))
        .trim()
        .trim_start_matches('v')
}
