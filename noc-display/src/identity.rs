use windows::{core::*, Win32::System::WindowsProgramming::GetUserNameW};

/// Logon name from the Windows token, never from configuration or USERNAME.
pub fn current_username() -> Result<String> {
    let mut buffer = [0u16; 257]; // UNLEN + terminating NUL.
    let mut length = buffer.len() as u32;
    unsafe {
        GetUserNameW(PWSTR(buffer.as_mut_ptr()), &mut length)?;
    }
    Ok(String::from_utf16_lossy(
        &buffer[..length.saturating_sub(1) as usize],
    ))
}
