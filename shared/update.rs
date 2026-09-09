//! Startup-only updates. No application configuration is loaded by this module.
use std::{env, fs, path::Path, process::{Command, Stdio}, thread, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};

pub fn suite_version() -> &'static str {
    option_env!("NOC_SUITE_VERSION").unwrap_or(include_str!("../VERSION")).trim().trim_start_matches('v')
}

fn log(app: &str, message: &str) {
    eprintln!("[{app} update] {message}");
}

/// Return true only when a Windows helper has accepted the restart handoff.
pub fn run(app: &str, default_asset: &str) -> bool {
    let skip = env::var("NOC_SKIP_UPDATE").ok().as_deref() == Some(app);
    env::remove_var("NOC_SKIP_UPDATE");
    if skip || env::var("NOC_DISABLE_UPDATES").ok().as_deref() == Some("1")
        || env::args().any(|arg| matches!(arg.as_str(), "--version" | "-V" | "--set-credentials" | "--no-update")) {
        return false;
    }
    match prepare(app, default_asset) {
        Ok(updated) => updated,
        Err(error) => { log(app, &format!("Skipped: {error}")); false }
    }
}

fn prepare(app: &str, default_asset: &str) -> std::io::Result<bool> {
    let exe = env::current_exe()?;
    let dir = exe.parent().ok_or_else(|| std::io::Error::other("Executable directory missing"))?;
    let updates = dir.join(".noc-updates");
    fs::create_dir_all(&updates)?;
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let stage = updates.join(format!("{app}-{}-{nonce}", std::process::id()));
    fs::create_dir(&stage)?;
    let result = launch(app, default_asset, &exe, &stage);
    // The Windows helper owns its staging directory after accepting the handoff.
    if !matches!(result, Ok(true)) { let _ = fs::remove_dir_all(&stage); }
    result
}

fn launch(app: &str, default_asset: &str, exe: &Path, stage: &Path) -> std::io::Result<bool> {
    #[cfg(windows)]
    let (script, interpreter) = ("update.ps1", "powershell.exe");
    #[cfg(not(windows))]
    let (script, interpreter) = ("update.py", "python3");
    #[cfg(windows)]
    fs::write(stage.join(script), include_str!("update.ps1"))?;
    #[cfg(not(windows))]
    fs::write(stage.join(script), include_str!("update.py"))?;
    let mut command = Command::new(interpreter);
    #[cfg(windows)] {
        use std::os::windows::process::CommandExt;
        command.args(["-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"]);
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW: no console on kiosk screens.
    }
    command.arg(stage.join(script))
        .env("NOC_UPDATE_APP", app)
        .env("NOC_UPDATE_EXE", exe)
        .env("NOC_UPDATE_STAGE", stage)
        .env("NOC_UPDATE_CURRENT", suite_version())
        .env("NOC_UPDATE_ASSET", option_env!("NOC_UPDATE_ASSET").unwrap_or(default_asset))
        .env("NOC_UPDATE_PARENT", std::process::id().to_string())
        .env("NOC_UPDATE_CWD", env::current_dir()?)
        .env("NOC_UPDATE_ARGS", encoded_args())
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let mut child = command.spawn()?;
    let start = Instant::now();
    loop {
        #[cfg(windows)]
        if stage.join("ready").is_file() {
            fs::write(stage.join("commit"), b"yes")?;
            return Ok(true);
        }
        if let Some(status) = child.try_wait()? {
            #[cfg(unix)]
            if status.code() == Some(20) {
                use std::os::unix::{fs::MetadataExt, process::CommandExt};
                let installed_inode = fs::metadata(exe)?.ino();
                let _ = fs::remove_dir_all(stage);
                let error = Command::new(exe).args(env::args_os().skip(1))
                    .env("NOC_SKIP_UPDATE", app).exec();
                // exec preserves the PID and therefore the systemd service ownership.
                if fs::metadata(exe).map(|m| m.ino()).ok() == Some(installed_inode) {
                    let backup = exe.parent().unwrap().join(".noc-updates").join(format!("{app}.previous"));
                    if let Err(restore) = fs::rename(backup, exe) { log(app, &format!("Rollback failed: {restore}")); }
                }
                log(app, &format!("New executable did not start: {error}; continuing current process"));
            }
            let _ = status;
            return Ok(false);
        }
        if start.elapsed() >= Duration::from_secs(45) {
            let _ = child.kill();
            let _ = child.wait();
            log(app, "Time limit reached; continuing current version");
            return Ok(false);
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn encoded_args() -> String {
    env::args_os().skip(1).map(|arg| {
        #[cfg(windows)]
        let bytes: Vec<u8> = { use std::os::windows::ffi::OsStrExt; arg.encode_wide().flat_map(u16::to_le_bytes).collect() };
        #[cfg(unix)]
        let bytes: Vec<u8> = { use std::os::unix::ffi::OsStrExt; arg.as_bytes().to_vec() };
        format!("x{}", bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>())
    }).collect::<Vec<_>>().join("\n")
}
