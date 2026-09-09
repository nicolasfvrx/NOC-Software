//! Defense in depth for native modal dialogs, scoped to the UI thread and control lifetime.
use super::events::{EventKind, Signals};
use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};
use windows::{
    core::*,
    Win32::{Foundation::*, System::Threading::GetCurrentThreadId, UI::WindowsAndMessaging::*},
};
thread_local! { static SIGNALS: RefCell<Option<Weak<Signals>>> = const { RefCell::new(None) }; }
pub struct DialogGuard(HHOOK);
impl DialogGuard {
    pub fn new(signals: &Rc<Signals>) -> Result<Self> {
        SIGNALS.with(|slot| *slot.borrow_mut() = Some(Rc::downgrade(signals)));
        unsafe { SetWindowsHookExW(WH_CBT, Some(hook), None, GetCurrentThreadId()).map(Self) }
    }
}
impl Drop for DialogGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = UnhookWindowsHookEx(self.0);
        }
        SIGNALS.with(|slot| *slot.borrow_mut() = None);
    }
}
unsafe extern "system" fn hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HCBT_CREATEWND as i32 && lparam.0 != 0 {
        let create = &*(lparam.0 as *const CBT_CREATEWNDW);
        if !create.lpcs.is_null() {
            let class = (*create.lpcs).lpszClass;
            let is_dialog = if class.0 as usize <= 0xffff {
                class.0 as usize == 32770
            } else {
                class.to_string().map(|s| s == "#32770").unwrap_or(false)
            };
            if is_dialog {
                let signals = SIGNALS.with(|slot| slot.borrow().as_ref().and_then(Weak::upgrade));
                if let Some(signals) = signals {
                    // Record only numeric metadata, never window text (may contain secrets).
                    signals.fail(EventKind::InteractionRequired {
                        source: "NativeDialogCreationBlocked",
                        code: (*create.lpcs).style,
                    });
                    return LRESULT(1);
                }
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}
