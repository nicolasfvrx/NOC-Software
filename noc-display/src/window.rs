//! Win32 shell window. Window callbacks only borrow the view or enqueue requests;
//! they never access App/ActiveX mutably, including during nested COM dispatch.
use crate::{assets::Assets, config::Config, renderer::Renderer, status::Status};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{GetKeyState, SetFocus, VK_CONTROL, VK_SHIFT},
            WindowsAndMessaging::*,
        },
    },
};
const APP_TIMER: usize = 1;
const ANIMATION_TIMER: usize = 2;
struct View {
    assets: Assets,
    renderer: Option<Renderer>,
    status: Status,
    last_error: Option<HRESULT>,
}
pub struct Context {
    view: RefCell<Option<View>>,
    pub resized: Cell<bool>,
    pub quit: Cell<bool>,
    pub exit_enabled: Cell<bool>,
}
pub struct Window {
    pub hwnd: HWND,
    pub context: Rc<Context>,
}
impl Window {
    pub fn create() -> Result<Self> {
        unsafe {
            let _ = SetProcessDPIAware();
            let instance = GetModuleHandleW(None)?;
            let class = w!("DisplayClientWindow");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance.into(),
                lpszClassName: class,
                hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                ..Default::default()
            };
            if RegisterClassW(&wc) == 0 {
                return Err(Error::from_win32());
            }
            let context = Rc::new(Context {
                view: RefCell::new(None),
                resized: Cell::new(false),
                quit: Cell::new(false),
                exit_enabled: Cell::new(true),
            });
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST,
                class,
                w!("NOC Display"),
                WS_POPUP | WS_CLIPCHILDREN,
                0,
                0,
                GetSystemMetrics(SM_CXSCREEN),
                GetSystemMetrics(SM_CYSCREEN),
                None,
                None,
                instance,
                Some(Rc::as_ptr(&context).cast()),
            );
            if hwnd.0 == 0 {
                return Err(Error::from_win32());
            }
            let window = Self { hwnd, context };
            fit_monitor(hwnd);
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            let _ = UpdateWindow(hwnd);
            let assets = Assets::load();
            *window.context.view.borrow_mut() = Some(View {
                assets,
                renderer: None,
                status: Status::Loading,
                last_error: None,
            });
            if SetTimer(hwnd, APP_TIMER, 250, None) == 0 {
                return Err(Error::from_win32());
            }
            Ok(window)
        }
    }
    pub fn configure(&self, config: &Config, username: &str) {
        self.context
            .exit_enabled
            .set(config.ui.development_exit_enabled);
        if let Some(view) = self.context.view.borrow_mut().as_mut() {
            match Renderer::new(config, username) {
                Ok(renderer) => view.renderer = Some(renderer),
                Err(e) => crate::log_error(format!("Renderer HRESULT={:08x}", e.code().0)),
            }
        }
    }
    /// Code court de l'etat courant, affiche dans le bandeau bas avec
    /// l'heure en direct. N'invalide pas la fenetre lui-meme : le timer
    /// applicatif (250 ms, WM_TIMER) s'en charge deja en continu, pour que
    /// l'horloge avance meme sans autre changement d'etat.
    pub fn set_state_code(&self, code: &'static str) {
        if let Some(view) = self.context.view.borrow_mut().as_mut() {
            if let Some(renderer) = view.renderer.as_mut() {
                renderer.set_state_code(code);
            }
        }
    }
    pub fn update(&self, status: Status, text: &str, animate: bool) {
        if let Some(view) = self.context.view.borrow_mut().as_mut() {
            view.status = status;
            if let Some(renderer) = view.renderer.as_mut() {
                renderer.set_message(text);
            }
        }
        unsafe {
            if animate {
                SetTimer(self.hwnd, ANIMATION_TIMER, 50, None);
            } else {
                let _ = KillTimer(self.hwnd, ANIMATION_TIMER);
            }
            let _ = InvalidateRect(self.hwnd, None, false);
        }
    }
    pub fn size(&self) -> (i32, i32) {
        let rect = crate::rdp::host::rect(self.hwnd);
        (rect.right.max(1), rect.bottom.max(1))
    }
    pub fn exit_shortcut(&self, message: &MSG) -> bool {
        if !self.context.exit_enabled.get()
            || !matches!(message.message, WM_KEYDOWN | WM_SYSKEYDOWN)
        {
            return false;
        }
        unsafe {
            (cfg!(feature = "dev-escape") && message.wParam.0 == 0x1b)
                || (message.wParam.0 == 0x7b
                    && GetKeyState(VK_CONTROL.0 as i32) < 0
                    && GetKeyState(VK_SHIFT.0 as i32) < 0)
        }
    }
    pub fn focus(&self) {
        unsafe {
            let _ = SetFocus(self.hwnd);
        }
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            let _ = KillTimer(self.hwnd, APP_TIMER);
            let _ = KillTimer(self.hwnd, ANIMATION_TIMER);
            let _ = DestroyWindow(self.hwnd);
            if let Ok(instance) = GetModuleHandleW(None) {
                let _ = UnregisterClassW(w!("DisplayClientWindow"), instance);
            }
        }
    }
}
unsafe fn fit_monitor(hwnd: HWND) {
    let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if GetMonitorInfoW(monitor, &mut info).as_bool() {
        let r = info.rcMonitor;
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            SWP_NOOWNERZORDER,
        );
    }
}
unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if message == WM_NCCREATE {
        SetWindowLongPtrW(
            hwnd,
            GWLP_USERDATA,
            (*(lp.0 as *const CREATESTRUCTW)).lpCreateParams as isize,
        );
        return LRESULT(1);
    }
    let context = (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Context).as_ref();
    match message {
        WM_NCDESTROY => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        }
        WM_CLOSE => {
            if let Some(c) = context {
                if c.exit_enabled.get() {
                    c.quit.set(true);
                }
            }
            return LRESULT(0);
        }
        WM_QUERYENDSESSION => return LRESULT(1),
        WM_ENDSESSION if wp.0 != 0 => {
            if let Some(c) = context {
                c.quit.set(true);
            }
            return LRESULT(0);
        }
        WM_DISPLAYCHANGE => {
            fit_monitor(hwnd);
            if let Some(c) = context {
                c.resized.set(true);
            }
            return LRESULT(0);
        }
        WM_SIZE => {
            if let Some(c) = context {
                c.resized.set(true);
            }
            let _ = InvalidateRect(hwnd, None, false);
            return LRESULT(0);
        }
        WM_ERASEBKGND => return LRESULT(1),
        WM_TIMER => {
            if wp.0 == ANIMATION_TIMER && !IsIconic(hwnd).as_bool() {
                let _ = InvalidateRect(hwnd, None, false);
            }
            // Inconditionnel (plus seulement sur erreur) : c'est ce qui fait
            // avancer l'horloge du bandeau bas (draw_live_info) au moins 4
            // fois par seconde, meme dans un etat par ailleurs statique
            // (ex. "Bienvenue, votre poste est pret", sans ANIMATION_TIMER).
            if wp.0 == APP_TIMER && !IsIconic(hwnd).as_bool() {
                let _ = InvalidateRect(hwnd, None, false);
            }
            return LRESULT(0);
        }
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut paint);
            let rect = crate::rdp::host::rect(hwnd);
            let mut rendered = false;
            if let Some(context) = context {
                if let Ok(mut cell) = context.view.try_borrow_mut() {
                    if let Some(view) = cell.as_mut() {
                        if let Some(renderer) = view.renderer.as_mut() {
                            match renderer.paint(
                                hwnd,
                                rect.right.max(0) as u32,
                                rect.bottom.max(0) as u32,
                                &view.assets,
                                view.status,
                                false,
                            ) {
                                Ok(()) => {
                                    rendered = true;
                                    view.last_error = None;
                                }
                                Err(e) => {
                                    if view.last_error != Some(e.code()) {
                                        crate::log_error(format!(
                                            "PAINT HRESULT={:08x}",
                                            e.code().0
                                        ));
                                    }
                                    view.last_error = Some(e.code());
                                }
                            }
                        }
                    }
                }
            }
            if !rendered {
                FillRect(dc, &rect, HBRUSH(GetStockObject(BLACK_BRUSH).0));
            }
            let _ = EndPaint(hwnd, &paint);
            return LRESULT(0);
        }
        _ => (),
    }
    DefWindowProcW(hwnd, message, wp, lp)
}
