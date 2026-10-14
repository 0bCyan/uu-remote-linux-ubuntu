//! Optional GTK3 window for `uuremote-server login` (c_engine/login_window.c). Built only when
//! gtk+-3.0 is available (cfg uu_gtk); otherwise, and with `--no-gui` or no display, every call
//! is a no-op and the terminal QR remains the only UI.

#[cfg(uu_gtk)]
extern "C" {
    fn uu_login_ui_start(name: *const libc::c_char, os: *const libc::c_char, cpu: *const libc::c_char, gpu: *const libc::c_char) -> libc::c_int;
    fn uu_login_ui_set_qr(rgba: *const u8, edge: libc::c_int);
    fn uu_login_ui_set_status(text: *const libc::c_char, kind: libc::c_int);
    fn uu_login_ui_set_done(text: *const libc::c_char);
    fn uu_login_ui_closed() -> libc::c_int;
    fn uu_login_ui_close();
}

#[derive(Clone, Copy)]
pub enum Tone { Info = 0, Ok = 1, Error = 2 }

pub struct LoginUi { shown: bool }

#[cfg(uu_gtk)]
fn cstr(s: &str) -> std::ffi::CString { std::ffi::CString::new(s.replace('\0', "")).unwrap_or_default() }

impl LoginUi {
    pub fn none() -> Self { LoginUi { shown: false } }

    /// Opens the window when built with GTK and a display is present; falls back silently otherwise.
    #[allow(unused_variables)]
    pub fn open(name: &str, os: &str, cpu: &str, gpu: &str) -> Self {
        #[cfg(uu_gtk)]
        if std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some() {
            let (n, o, c, g) = (cstr(name), cstr(os), cstr(cpu), cstr(gpu));
            if unsafe { uu_login_ui_start(n.as_ptr(), o.as_ptr(), c.as_ptr(), g.as_ptr()) } == 0 {
                return LoginUi { shown: true };
            }
            eprintln!("Login window unavailable; using the terminal QR code");
        }
        LoginUi::none()
    }

    pub fn shown(&self) -> bool { self.shown }

    #[allow(unused_variables)]
    pub fn qr(&self, rgba: &[u8], edge: usize) {
        #[cfg(uu_gtk)]
        if self.shown && rgba.len() == edge * edge * 4 { unsafe { uu_login_ui_set_qr(rgba.as_ptr(), edge as libc::c_int) } }
    }

    #[allow(unused_variables)]
    pub fn status(&self, text: &str, tone: Tone) {
        #[cfg(uu_gtk)]
        if self.shown { let t = cstr(text); unsafe { uu_login_ui_set_status(t.as_ptr(), tone as libc::c_int) } }
    }

    #[allow(unused_variables)]
    pub fn done(&self, text: &str) {
        #[cfg(uu_gtk)]
        if self.shown { let t = cstr(text); unsafe { uu_login_ui_set_done(t.as_ptr()) } }
    }

    pub fn closed(&self) -> bool {
        #[cfg(uu_gtk)]
        if self.shown { return unsafe { uu_login_ui_closed() } != 0; }
        false
    }

    /// Resolves when the user closes the window; never resolves without a window.
    pub async fn wait_closed(&self) {
        if !self.shown { return std::future::pending().await; }
        while !self.closed() { tokio::time::sleep(std::time::Duration::from_millis(200)).await; }
    }
}

impl Drop for LoginUi {
    fn drop(&mut self) {
        #[cfg(uu_gtk)]
        if self.shown { unsafe { uu_login_ui_close() } }
    }
}
