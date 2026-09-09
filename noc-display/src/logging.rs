use std::{fs::OpenOptions, io::Write, path::PathBuf, sync::OnceLock};

fn context() -> &'static str {
    static CONTEXT: OnceLock<String> = OnceLock::new();
    CONTEXT.get_or_init(|| {
        let account = crate::identity::current_username().unwrap_or_else(|_| "unknown".to_owned());
        format!("[windows_user={account:?} pid={}]", std::process::id())
    })
}

pub fn write(message: impl std::fmt::Display) {
    if let Some(dir) = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
    {
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("display-client.log"))
        {
            let time = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
            // Each Windows account has its own append-only log in its profile.
            let line = format!(
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02} {} {}\n",
                time.wYear,
                time.wMonth,
                time.wDay,
                time.wHour,
                time.wMinute,
                time.wSecond,
                context(),
                message
            );
            let _ = file.write_all(line.as_bytes());
        }
    }
}
