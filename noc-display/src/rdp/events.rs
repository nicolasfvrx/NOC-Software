use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::Gdi::{InvalidateRect, UpdateWindow},
        System::{Com::*, Variant::*},
        UI::WindowsAndMessaging::*,
    },
};

pub const WM_RDP_EVENT: u32 = WM_APP + 10;
pub fn ready_timer_id(generation: u64) -> usize {
    100 + generation as usize
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Connecting,
    TransportConnected,
    LoginComplete,
    RemoteDesktopSizeChanged,
    Disconnected(i32),
    Fatal(i32),
    Warning(i32),
    InteractionRequired { source: &'static str, code: i32 },
    AutoReconnecting,
    AutoReconnected,
}
#[derive(Debug, Clone, Copy)]
pub struct Event {
    pub generation: u64,
    pub kind: EventKind,
}
pub type Queue = Rc<RefCell<VecDeque<Event>>>;
pub struct Signals {
    pub parent: HWND,
    pub child: Cell<HWND>,
    pub generation: u64,
    pub failed: Cell<bool>,
    pub queue: Queue,
}
impl Signals {
    pub fn emit(&self, kind: EventKind) {
        self.queue.borrow_mut().push_back(Event {
            generation: self.generation,
            kind,
        });
        unsafe {
            let _ = PostMessageW(self.parent, WM_RDP_EVENT, WPARAM(0), LPARAM(0));
        }
    }
    pub fn hide(&self) {
        unsafe {
            let _ = ShowWindow(self.child.get(), SW_HIDE);
            let _ = InvalidateRect(self.parent, None, false);
            let _ = UpdateWindow(self.parent);
        }
    }
    pub fn fail(&self, kind: EventKind) {
        // Visibility is revoked synchronously, before posting, logging or disconnecting.
        self.hide();
        unsafe { let _ = KillTimer(self.parent, ready_timer_id(self.generation)); }
        if !self.failed.replace(true) {
            self.emit(kind);
        }
    }
}

#[interface("336d5562-efa8-482e-8cb3-c5c0fc7a7db6")]
pub unsafe trait IMsTscAxEvents: IDispatch {}
#[implement(IMsTscAxEvents)]
struct Sink {
    signals: Rc<Signals>,
}
impl IMsTscAxEvents_Impl for Sink {}
impl IDispatch_Impl for Sink {
    fn GetTypeInfoCount(&self) -> Result<u32> {
        Ok(0)
    }
    fn GetTypeInfo(&self, _: u32, _: u32) -> Result<ITypeInfo> {
        Err(E_NOTIMPL.into())
    }
    fn GetIDsOfNames(
        &self,
        _: *const GUID,
        _: *const PCWSTR,
        _: u32,
        _: u32,
        _: *mut i32,
    ) -> Result<()> {
        Err(DISP_E_UNKNOWNNAME.into())
    }
    fn Invoke(
        &self,
        id: i32,
        _: *const GUID,
        _: u32,
        _: DISPATCH_FLAGS,
        params: *const DISPPARAMS,
        _: *mut VARIANT,
        _: *mut EXCEPINFO,
        _: *mut u32,
    ) -> Result<()> {
        let args = unsafe {
            if params.is_null() || (*params).cArgs == 0 || (*params).rgvarg.is_null() {
                &[][..]
            } else {
                std::slice::from_raw_parts((*params).rgvarg, (*params).cArgs as usize)
            }
        };
        let number = || unsafe {
            args.first()
                .filter(|a| a.Anonymous.Anonymous.vt == VT_I4)
                .map(|a| a.Anonymous.Anonymous.Anonymous.lVal)
                .unwrap_or(-1)
        };
        match id {
            1 => self.signals.emit(EventKind::Connecting),
            2 => self.signals.emit(EventKind::TransportConnected),
            3 if !self.signals.failed.get() => self.signals.emit(EventKind::LoginComplete),
            4 => self.signals.fail(EventKind::Disconnected(number())),
            10 => self.signals.fail(EventKind::Fatal(number())),
            11 => self.signals.fail(EventKind::Warning(number())),
            12 => self.signals.emit(EventKind::RemoteDesktopSizeChanged),
            15 => unsafe {
                set_bool(args, true);
            }, // Allow programmatic close without confirmation.
            16 => {
                unsafe {
                    set_bool(args, false);
                }
                self.signals.fail(EventKind::InteractionRequired {
                    source: "OnReceivedTSPublicKey",
                    code: 0,
                });
            }
            17 => {
                unsafe {
                    if let Some(arg) = args.first() {
                        if arg.Anonymous.Anonymous.vt.0 == (VT_I4.0 | VT_BYREF.0) {
                            let p = arg.Anonymous.Anonymous.Anonymous.plVal;
                            if !p.is_null() {
                                *p = 1;
                            } // autoReconnectStop
                        }
                    }
                }
                self.signals.fail(EventKind::AutoReconnecting);
            }
            18 => self.signals.fail(EventKind::InteractionRequired {
                source: "OnAuthenticationWarningDisplayed",
                code: 0,
            }),
            22 => self.signals.fail(EventKind::InteractionRequired {
                source: "OnLogonError",
                code: number(),
            }),
            5 | 8 => self.signals.fail(EventKind::InteractionRequired {
                source: "ExternalFullscreenRequested",
                code: id,
            }),
            33 => self.signals.emit(EventKind::AutoReconnected),
            34 => self.signals.fail(EventKind::AutoReconnecting),
            _ => (),
        }
        Ok(())
    }
}
unsafe fn set_bool(args: &[VARIANT], value: bool) {
    if let Some(arg) = args.first() {
        if arg.Anonymous.Anonymous.vt.0 == (VT_BOOL.0 | VT_BYREF.0) {
            let p = arg.Anonymous.Anonymous.Anonymous.pboolVal;
            if !p.is_null() {
                *p = VARIANT_BOOL(if value { -1 } else { 0 });
            }
        }
    }
}
pub struct Subscription {
    point: IConnectionPoint,
    cookie: u32,
    _sink: IMsTscAxEvents,
}
impl Subscription {
    pub fn new(object: &IDispatch, signals: Rc<Signals>) -> Result<Self> {
        unsafe {
            let point = object
                .cast::<IConnectionPointContainer>()?
                .FindConnectionPoint(&IMsTscAxEvents::IID)?;
            let sink: IMsTscAxEvents = Sink { signals }.into();
            let cookie = point.Advise(&sink.cast::<IUnknown>()?)?;
            Ok(Self {
                point,
                cookie,
                _sink: sink,
            })
        }
    }
}
impl Drop for Subscription {
    fn drop(&mut self) {
        unsafe {
            let _ = self.point.Unadvise(self.cookie);
        }
    }
}
