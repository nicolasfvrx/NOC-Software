use super::{
    dialog_guard::DialogGuard,
    dispatch,
    events::{Queue, Signals, Subscription},
    host, RdpClient,
};
use std::{marker::PhantomData, rc::Rc};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::Gdi::{GetStockObject, BLACK_BRUSH, HBRUSH},
        System::LibraryLoader::GetModuleHandleW,
        System::{Com::*, Ole::*},
        UI::WindowsAndMessaging::*,
    },
};

pub struct ActiveX {
    subscription: Option<Subscription>,
    pub signals: Rc<Signals>,
    _dialogs: DialogGuard,
    pub dispatch: IDispatch,
    object: IOleObject,
    inplace: IOleInPlaceObject,
    _site: IOleClientSite,
    pub child: HWND,
    _sta: PhantomData<Rc<()>>,
}
impl ActiveX {
    pub fn create(
        parent: HWND,
        width: i32,
        height: i32,
        generation: u64,
        queue: Queue,
    ) -> Result<Self> {
        unsafe {
            // These versions shipped with Windows 8 / Windows 7 respectively.
            let mut found = None;
            for (version, id) in [
                (8, GUID::from_u128(0xa3bc03a0_041d_42e3_ad22_882b7865c9c5)),
                (7, GUID::from_u128(0x54d38bf7_b1ef_4479_9674_1bd6ea465258)),
            ] {
                match CoCreateInstance::<_, IOleObject>(&id, None, CLSCTX_INPROC_SERVER) {
                    Ok(object) => {
                        if object
                            .cast::<super::interfaces::IMsRdpClientNonScriptable5>()
                            .is_err()
                        {
                            crate::log_error(format!(
                                "RDP ActiveX version={version} lacks required no-prompt interface"
                            ));
                            continue;
                        }
                        crate::log_error(format!("RDP ActiveX version={version}"));
                        found = Some(object);
                        break;
                    }
                    Err(e) => crate::log_error(format!(
                        "RDP ActiveX version={version} unavailable HRESULT={:08x}",
                        e.code().0
                    )),
                }
            }
            let object = found.ok_or_else(|| Error::from(REGDB_E_CLASSNOTREG))?;
            let dispatch = object.cast()?;
            let inplace = object.cast()?;
            let instance = GetModuleHandleW(None)?;
            let wc = WNDCLASSW {
                lpfnWndProc: Some(child_proc),
                hInstance: instance.into(),
                lpszClassName: w!("DisplayClientRdpHost"),
                hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
                ..Default::default()
            };
            if RegisterClassW(&wc) == 0
                && Error::from_win32().code() != HRESULT::from_win32(ERROR_CLASS_ALREADY_EXISTS.0)
            {
                return Err(Error::from_win32());
            }
            let child = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("DisplayClientRdpHost"),
                w!(""),
                WS_CHILD | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
                0,
                0,
                width,
                height,
                parent,
                None,
                None,
                None,
            );
            if child.0 == 0 {
                return Err(Error::from_win32());
            }
            let signals = Rc::new(Signals {
                parent,
                child: std::cell::Cell::new(child),
                generation,
                failed: std::cell::Cell::new(false),
                queue,
            });
            let dialogs = match DialogGuard::new(&signals) {
                Ok(g) => g,
                Err(e) => {
                    let _ = DestroyWindow(child);
                    return Err(e);
                }
            };
            let site = host::client_site(child, parent);
            let mut client = Self {
                subscription: None,
                signals,
                _dialogs: dialogs,
                dispatch,
                object,
                inplace,
                _site: site,
                child,
                _sta: PhantomData,
            };
            client.object.SetClientSite(&client._site)?;
            if let Ok(persist) = client.object.cast::<IPersistStreamInit>() {
                persist.InitNew()?;
            }
            OleSetContainedObject(&client.object, true)?;
            client.object.DoVerb(
                OLEIVERB_INPLACEACTIVATE.0,
                std::ptr::null(),
                &client._site,
                0,
                child,
                &host::rect(child),
            )?;
            client.resize(width, height)?;
            client.subscription =
                Some(Subscription::new(&client.dispatch, client.signals.clone())?);
            Ok(client)
        }
    }
    /// Read before destruction/Disconnect can reset the control's diagnostic state.
    pub fn is_connected(&self) -> Result<bool> {
        let value = dispatch::get(&self.dispatch, "Connected")?;
        unsafe {
            use windows::Win32::System::Variant::{VT_I2, VT_I4};
            let data = &value.0.Anonymous.Anonymous;
            match data.vt {
                VT_I2 => Ok(data.Anonymous.iVal == 1),
                VT_I4 => Ok(data.Anonymous.lVal == 1),
                _ => Err(E_UNEXPECTED.into()),
            }
        }
    }

    pub fn extended_disconnect_reason(&self) -> Result<i32> {
        let value = dispatch::get(&self.dispatch, "ExtendedDisconnectReason")?;
        unsafe {
            use windows::Win32::System::Variant::{VT_I4, VT_UI4};
            let data = &value.0.Anonymous.Anonymous;
            if matches!(data.vt, VT_I4 | VT_UI4) {
                Ok(data.Anonymous.lVal)
            } else {
                Err(E_UNEXPECTED.into())
            }
        }
    }

    pub fn set_password(&self, password: &[u16]) -> Result<()> {
        struct SecretBstr {
            raw: *const u16,
            length: usize,
        }
        impl Drop for SecretBstr {
            fn drop(&mut self) {
                unsafe {
                    for i in 0..self.length {
                        std::ptr::write_volatile((self.raw as *mut u16).add(i), 0);
                    }
                    drop(BSTR::from_raw(self.raw));
                }
            }
        }
        let secret = SecretBstr {
            raw: BSTR::from_wide(password)?.into_raw(),
            length: password.len(),
        };
        unsafe {
            self.dispatch
                .cast::<super::interfaces::IMsTscNonScriptable>()?
                .put_ClearTextPassword(secret.raw)
                .ok()
        }
    }
}
unsafe extern "system" fn child_proc(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    DefWindowProcW(hwnd, message, wp, lp)
}
impl RdpClient for ActiveX {
    fn connect(&self) -> Result<()> {
        dispatch::call(&self.dispatch, "Connect")
    }
    fn disconnect(&self) {
        self.set_visible(false);
        let _ = dispatch::call(&self.dispatch, "Disconnect");
    }
    fn resize(&self, width: i32, height: i32) -> Result<()> {
        unsafe {
            SetWindowPos(
                self.child,
                None,
                0,
                0,
                width,
                height,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )?;
            let rect = RECT {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            };
            self.inplace.SetObjectRects(&rect, &rect)
        }
    }
    fn set_visible(&self, visible: bool) {
        let visible = visible && !self.signals.failed.get();
        unsafe {
            let _ = ShowWindow(self.child, if visible { SW_SHOW } else { SW_HIDE });
            if visible {
                if let Ok(hwnd) = self.inplace.GetWindow() {
                    let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(hwnd);
                }
            }
        }
    }
    fn translate_accelerator(&self, message: &MSG) -> bool {
        unsafe {
            self.object
                .cast::<IOleInPlaceActiveObject>()
                .map(|a| (Interface::vtable(&a).TranslateAccelerator)(a.as_raw(), message) == S_OK)
                .unwrap_or(false)
        }
    }
}
impl Drop for ActiveX {
    fn drop(&mut self) {
        self.signals.failed.set(true);
        self.subscription.take(); // Unadvise before Disconnect/Close can emit callbacks.
        self.disconnect();
        unsafe {
            if let Ok(password) = self
                .dispatch
                .cast::<super::interfaces::IMsTscNonScriptable>()
            {
                let _ = password.ResetPassword();
            }
            let _ = self.inplace.InPlaceDeactivate();
            let _ = self.object.Close(OLECLOSE_NOSAVE);
            let _ = self.object.SetClientSite(None);
            let _ = DestroyWindow(self.child);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refused_connection_emits_events_without_exposing_control() -> Result<()> {
        use std::time::{Duration, Instant};
        unsafe {
            OleInitialize(None)?;
            let parent = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("RDP events test"),
                WS_POPUP,
                0,
                0,
                800,
                600,
                None,
                None,
                None,
                None,
            );
            let result = (|| -> Result<()> {
                let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
                let port = listener.local_addr().unwrap().port();
                drop(listener);
                let queue: Queue = Default::default();
                let client = ActiveX::create(parent, 800, 600, 42, queue.clone())?;
                // A mapped frame, kept outside the desktop, matches normal shell activation.
                SetWindowPos(
                    parent,
                    None,
                    -30000,
                    -30000,
                    800,
                    600,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                )?;
                let _ = ShowWindow(parent, SW_SHOWNOACTIVATE);
                super::super::settings::configure(
                    &client.dispatch,
                    &crate::config::RdpSettings {
                        server: "127.0.0.1".into(),
                        username: "synthetic-test".into(),
                        port,
                        ..Default::default()
                    },
                    800,
                    600,
                    parent,
                )?;
                client.set_password(&"synthetic-password".encode_utf16().collect::<Vec<_>>())?;
                super::super::settings::finalize_xrdp_credentials(&client.dispatch)?;
                client.connect()?;
                let start = Instant::now();
                let mut message = MSG::default();
                while !client.signals.failed.get() && start.elapsed() < Duration::from_secs(90) {
                    while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                        let _ = TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                    // Test harness only; production uses the Win32 message loop and timers.
                    std::thread::sleep(Duration::from_millis(10));
                }
                let events: Vec<_> = queue.borrow().iter().map(|e| e.kind).collect();
                eprintln!("Local refusal event sequence: {events:?}");
                assert!(events.contains(&super::super::events::EventKind::Connecting));
                assert!(
                    client.signals.failed.get(),
                    "Expected a failure event within 90 seconds"
                );
                assert!(!events.contains(&super::super::events::EventKind::LoginComplete));
                assert!(!IsWindowVisible(client.child).as_bool());
                Ok(())
            })();
            let _ = DestroyWindow(parent);
            OleUninitialize();
            result
        }
    }
    #[test]
    fn create_activate_resize_and_destroy_real_mstscax_without_connecting() -> Result<()> {
        unsafe {
            OleInitialize(None)?;
            let parent = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("Host test"),
                WS_POPUP,
                0,
                0,
                800,
                600,
                None,
                None,
                None,
                None,
            );
            let result = (|| -> Result<()> {
                let client = ActiveX::create(parent, 800, 600, 1, Default::default())?;
                super::super::settings::configure(
                    &client.dispatch,
                    &crate::config::RdpSettings {
                        server: "127.0.0.1".into(),
                        username: "host-test".into(),
                        ..Default::default()
                    },
                    800,
                    600,
                    parent,
                )?;
                // Exercise the explicit certificate override and COM readback as well.
                super::super::settings::configure(
                    &client.dispatch,
                    &crate::config::RdpSettings {
                        server: "127.0.0.1".into(),
                        username: "host-test".into(),
                        ignore_certificate_errors: true,
                        ..Default::default()
                    },
                    800,
                    600,
                    parent,
                )?;
                client
                    .set_password(&"synthetic-test-password".encode_utf16().collect::<Vec<_>>())?;
                super::super::settings::finalize_xrdp_credentials(&client.dispatch)?;
                let _ = client.extended_disconnect_reason()?;
                let _ = dispatch::error_description(&client.dispatch, 2, 0)?;
                client.set_visible(true);
                client.resize(1024, 768)?;
                assert_eq!(GetParent(client.child), parent);
                assert!(IsWindow(client.inplace.GetWindow()?).as_bool());
                client.set_visible(false);
                // Standard native modal dialogs are refused before they can be shown.
                let dialog = CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    w!("#32770"),
                    w!("Blocked test dialog"),
                    WS_POPUP,
                    0,
                    0,
                    100,
                    100,
                    parent,
                    None,
                    None,
                    None,
                );
                assert_eq!(dialog.0, 0);
                assert!(client.signals.failed.get());
                Ok(())
            })();
            let _ = DestroyWindow(parent);
            OleUninitialize();
            result
        }
    }
}
