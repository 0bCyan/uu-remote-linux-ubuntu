//! Native Linux X11 Input Injection Engine via XTest

#[cfg(target_os = "linux")]
extern "C" {
    fn x11_mouse_move(x: libc::c_int, y: libc::c_int) -> libc::c_int;
    fn x11_mouse_relative(dx: libc::c_int, dy: libc::c_int) -> libc::c_int;
    fn x11_mouse_button(button: libc::c_int, is_press: libc::c_int) -> libc::c_int;
    fn x11_pointer_state(x:*mut libc::c_int,y:*mut libc::c_int,mask:*mut libc::c_uint)->libc::c_int;
    fn x11_key_symbol(symbol: libc::c_ulong, is_press: libc::c_int) -> libc::c_int;
    fn x11_key_event(keycode: libc::c_int, is_press: libc::c_int) -> libc::c_int;
    fn x11_terminal_shortcut_state() -> libc::c_int;
}
#[cfg(not(target_os = "linux"))]
unsafe fn x11_terminal_shortcut_state() -> libc::c_int { 0 }

#[cfg(not(target_os = "linux"))]
unsafe fn x11_mouse_move(_: libc::c_int, _: libc::c_int) -> libc::c_int { 0 }
#[cfg(not(target_os = "linux"))]
unsafe fn x11_mouse_button(_: libc::c_int, _: libc::c_int) -> libc::c_int { 0 }
#[cfg(not(target_os = "linux"))]
unsafe fn x11_key_event(_: libc::c_int, _: libc::c_int) -> libc::c_int { 0 }

#[cfg(not(target_os = "linux"))]
unsafe fn x11_key_symbol(_: libc::c_ulong, _: libc::c_int) -> libc::c_int { 0 }

#[cfg(not(target_os = "linux"))]
unsafe fn x11_mouse_relative(_: libc::c_int, _: libc::c_int) -> libc::c_int {0}

#[derive(Default)]
struct PendingPointer {
    absolute: Option<(i32,i32)>,
    relative: (i64,i64),
    scroll: (i64,i64),
}
#[cfg(not(target_os = "linux"))]
unsafe fn x11_pointer_state(_: *mut libc::c_int,_:*mut libc::c_int,_:*mut libc::c_uint)->libc::c_int {-1}
#[derive(Default)]
struct HeldInput { buttons: std::collections::BTreeSet<i32>, keys: std::collections::BTreeSet<u64> }
/// Held modifier VKs (bit 0 Ctrl, 1 Shift, 2 Alt, 3 Win) and keys that got a synthetic Shift.
#[derive(Default)]
struct Shortcuts { modifiers: u8, shifted: std::collections::BTreeSet<i32> }

pub struct InputInjector { shortcuts: std::sync::Mutex<Shortcuts>, uinput: Option<crate::uinput::Devices>, screen: std::sync::Mutex<(u32,u32)>, portal_failures: std::sync::atomic::AtomicUsize, portal_outcomes: [std::sync::atomic::AtomicUsize; 3], portal:std::sync::Mutex<Option<std::sync::Arc<crate::portal::PortalSession>>>, backend: crate::desktop_backend::DesktopBackend, pending: std::sync::Mutex<PendingPointer>, held: std::sync::Mutex<HeldInput> }

impl InputInjector {
    pub fn new() -> Self {
        let backend = crate::desktop_backend::detect().unwrap_or(crate::desktop_backend::DesktopBackend::X11);
        let uinput = if crate::uinput::wanted(backend) {
            match crate::uinput::Devices::open() {
                Ok(devices) => { log::info!("Input backend: uinput (kernel virtual keyboard/pointer)"); Some(devices) }
                Err(e) => { log::warn!("uinput unavailable ({}); using the desktop input path. See README for the udev rule.", e.kind()); None }
            }
        } else { None };
        Self { shortcuts: std::sync::Mutex::new(Shortcuts::default()), uinput, screen: std::sync::Mutex::new((0,0)), portal_failures: std::sync::atomic::AtomicUsize::new(0), portal_outcomes: std::array::from_fn(|_| std::sync::atomic::AtomicUsize::new(0)), portal:std::sync::Mutex::new(None), backend:crate::desktop_backend::detect().unwrap_or(crate::desktop_backend::DesktopBackend::X11), pending: std::sync::Mutex::new(PendingPointer::default()), held:std::sync::Mutex::new(HeldInput::default()) }
    }

    pub fn bind_portal(&self,session:Option<std::sync::Arc<crate::portal::PortalSession>>) {
        log::info!("Desktop portal input binding updated: bound={}", session.is_some());
        *self.portal.lock().unwrap()=session;
    }
    pub fn mouse_move(&self, x: i32, y: i32) {
        self.inject_move(x,y,false);
    }

    pub fn mouse_absolute_normalized(&self, x: f64, y: f64, size: (u32,u32)) {
        *self.screen.lock().unwrap() = size;
        if let Some(position) = crate::input_mapping::normalized_pointer(x,y,size.0,size.1) {
            let mut pending = self.pending.lock().unwrap();
            pending.absolute = Some(position);
            pending.relative = (0,0);
        }
    }
    pub fn mouse_relative(&self, dx: i32, dy: i32) {
        if dx.unsigned_abs() <= 65535 && dy.unsigned_abs() <= 65535 {
            let mut pending = self.pending.lock().unwrap();
            pending.relative.0 = (pending.relative.0 + i64::from(dx)).clamp(-65535,65535);
            pending.relative.1 = (pending.relative.1 + i64::from(dy)).clamp(-65535,65535);
        }
    }
    pub fn mouse_scroll(&self, dx: i32, dy: i32) {
        // Official Mac normal wheel=120; precise trackpads use smaller values.
        let mut pending = self.pending.lock().unwrap();
        pending.scroll.0 = (pending.scroll.0 + i64::from(dx)).clamp(-12000,12000);
        pending.scroll.1 = (pending.scroll.1 + i64::from(dy)).clamp(-12000,12000);
    }
    pub fn start_pointer_worker(self: &std::sync::Arc<Self>) -> tokio::task::JoinHandle<()> {
        let input = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_millis(4));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop { tick.tick().await; input.flush_pointer(); }
        })
    }
    pub fn discard_pointer(&self) { *self.pending.lock().unwrap() = PendingPointer::default(); }
    pub fn flush_pointer(&self) {
        // Hold the queue lock through injection so an older timer tick cannot
        // overwrite a newer position flushed before a button transition.
        let mut pending = self.pending.lock().unwrap();
        let motion = std::mem::take(&mut *pending);
        pending.scroll = (motion.scroll.0 % 120, motion.scroll.1 % 120);
        if let Some((x,y)) = motion.absolute { self.inject_move(x,y,false); }
        if motion.relative != (0,0) { self.inject_move(motion.relative.0 as i32,motion.relative.1 as i32,true); }
        for (value, positive, negative) in [(motion.scroll.1,4,5),(motion.scroll.0,7,6)] {
            let count = value / 120;
            let button = if count > 0 { positive } else { negative };
            for _ in 0..count.unsigned_abs().min(100) {
                self.inject_button(button,true); self.inject_button(button,false);
            }
        }
    }
    pub fn wire_mouse_button(&self, button: i32, is_press: bool) {
        if let Some(mapped) = crate::input_mapping::mouse_button_to_x11(button) { self.mouse_button(mapped,is_press); }
        else {
            static UNKNOWN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            if UNKNOWN.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 16 {
                log::warn!("Unsupported desktop wire mouse button: {}", button);
            }
        }
    }
    pub fn mouse_button(&self, button: i32, is_press: bool) {
        self.flush_pointer();
        let result = self.inject_button(button,is_press);
        if result == 0 {
            let mut held = self.held.lock().unwrap();
            if is_press { held.buttons.insert(button); } else { held.buttons.remove(&button); }
        } else if self.backend != crate::desktop_backend::DesktopBackend::Portal { log::warn!("Desktop mouse injection failed: {}",result); }
        static BUTTON_AUDIT:std::sync::atomic::AtomicUsize=std::sync::atomic::AtomicUsize::new(0);
        if self.backend==crate::desktop_backend::DesktopBackend::X11 && BUTTON_AUDIT.fetch_add(1,std::sync::atomic::Ordering::Relaxed)<24 {
            let (mut x,mut y,mut mask)=(0,0,0);
            let query=unsafe {x11_pointer_state(&mut x,&mut y,&mut mask)};
            log::info!("Pointer button audit: button={} down={} result={} query={} x={} y={} mask={}",button,is_press,result,query,x,y,mask);
        }
    }

    /// Mac viewers send Cmd+V as Ctrl+V (verified: Cmd and Ctrl both arrive as VK_LCONTROL), which in
    /// a Linux terminal quotes the next key instead of pasting. In a focused terminal Ctrl+V becomes
    /// Ctrl+Shift+V. Ctrl+C is never translated: it must always interrupt, and copy is Cmd+Shift+C
    /// (= Ctrl+Shift+C). X11 only; `UU_TERMINAL_SHORTCUTS=0` disables.
    fn terminal_shift(&self, key: i32, is_press: bool) -> bool {
        let bit = match key { 0x11 | 0xa2 | 0xa3 => 1, 0x10 | 0xa0 | 0xa1 => 2, 0x12 | 0xa4 | 0xa5 => 4, 0x5b | 0x5c => 8, _ => 0 };
        let mut shortcuts = self.shortcuts.lock().unwrap();
        if bit != 0 {
            if is_press { shortcuts.modifiers |= bit } else { shortcuts.modifiers &= !bit }
            return false;
        }
        if !is_press { return shortcuts.shifted.remove(&key); }
        if key != 0x56 || shortcuts.modifiers != 1 { return false; }
        if std::env::var("UU_TERMINAL_SHORTCUTS").is_ok_and(|v| v == "0") || std::env::var_os("DISPLAY").is_none() { return false; }
        let shift = unsafe { x11_terminal_shortcut_state() } & 1 != 0;
        if shift {
            shortcuts.shifted.insert(key);
            static NOTED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            if NOTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 4 {
                log::info!("Terminal shortcut: Ctrl+V sent as Ctrl+Shift+V");
            }
        }
        shift
    }

    pub fn windows_key_event(&self, key: i32, is_press: bool) {
        if self.terminal_shift(key, is_press) {
            const SHIFT_L: u64 = 0xffe1;
            if is_press {
                self.inject_key(SHIFT_L, true);
                self.key_symbol_event(key, true);
            } else {
                self.key_symbol_event(key, false);
                self.inject_key(SHIFT_L, false);
            }
            return;
        }
        self.key_symbol_event(key, is_press);
    }

    fn key_symbol_event(&self, key: i32, is_press: bool) {
        if let Some(symbol) = crate::input_mapping::windows_vk_to_keysym(key) {
            let result = self.inject_key(symbol,is_press);
            static AUDIT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            if AUDIT.fetch_add(1,std::sync::atomic::Ordering::Relaxed) < 8 {
                log::info!("Desktop keyboard injection outcome: result={result}");
            }
            if result == 0 {
                let mut held=self.held.lock().unwrap();
                if is_press {held.keys.insert(symbol);} else {held.keys.remove(&symbol);}
            }
        }
    }

    pub fn release_held(&self) {
        self.discard_pointer();
        let mut held=self.held.lock().unwrap();
        for button in &held.buttons {self.inject_button(*button,false);}
        for key in &held.keys {self.inject_key(*key,false);}
        *held=HeldInput::default();
    }
    // Per-injector budgets cover missing bindings, locked sessions, stale handles
    // and D-Bus errors. Never include keys, coordinates, text or portal tokens.
    fn portal_result(&self, operation: &str, result: Result<(), String>) -> i32 {
        use std::sync::atomic::Ordering::Relaxed;
        let slot = match operation { "motion" => 0, "button" => 1, _ => 2 };
        if self.portal_outcomes[slot].fetch_add(1, Relaxed) < 16 {
            log::info!("Portal input outcome: operation={} success={}", operation, result.is_ok());
        }
        match result {
            Ok(()) => 0,
            Err(reason) => {
                let index = self.portal_failures.fetch_add(1, Relaxed);
                if index < 16 {
                    log::warn!("Portal input injection failed: operation={} reason={} failure={}/16{}",
                        operation, reason, index + 1, if index == 15 { " (further failures suppressed)" } else { "" });
                }
                -1
            }
        }
    }
    fn uinput_result(result: std::io::Result<()>) -> i32 {
        match result {
            Ok(()) => 0,
            Err(e) => {
                static FAILURES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
                if FAILURES.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 8 { log::warn!("uinput injection failed: {}", e.kind()); }
                -1
            }
        }
    }
    fn inject_move(&self,x:i32,y:i32,relative:bool)->i32 {
        if let Some(uinput)=&self.uinput {
            if relative { return Self::uinput_result(uinput.move_relative(x,y)); }
            let (w,h)=*self.screen.lock().unwrap();
            if w > 1 && h > 1 {
                let scale = |v: i32, extent: u32| (i64::from(v) * i64::from(crate::uinput::ABS_MAX) / i64::from(extent - 1)) as i32;
                return Self::uinput_result(uinput.move_absolute(scale(x,w),scale(y,h)));
            }
        }
        if let Some(portal)=self.portal.lock().unwrap().as_ref() { return self.portal_result("motion", portal.move_pointer(f64::from(x),f64::from(y),relative)); }
        if self.backend==crate::desktop_backend::DesktopBackend::Portal {return self.portal_result("motion", Err("Portal input session is not bound".into()));}
        if self.backend==crate::desktop_backend::DesktopBackend::Wayland { crate::wayland::move_pointer(x,y,relative) }
        else if relative { unsafe { x11_mouse_relative(x,y) } } else { unsafe { x11_mouse_move(x,y) } }
    }
    fn inject_button(&self,button:i32,pressed:bool)->i32 {
        if let Some(uinput)=&self.uinput {
            if let Some((v,h))=crate::uinput::wheel_step(button) { return if pressed { Self::uinput_result(uinput.wheel(v,h)) } else { 0 }; }
            if let Some(code)=crate::uinput::button_code(button) { return Self::uinput_result(uinput.button(code,pressed)); }
        }
        if let Some(portal)=self.portal.lock().unwrap().as_ref() { return self.portal_result("button", portal.button(button,pressed)); }
        if self.backend==crate::desktop_backend::DesktopBackend::Portal {return self.portal_result("button", Err("Portal input session is not bound".into()));}
        if self.backend==crate::desktop_backend::DesktopBackend::Wayland { crate::wayland::button(button,pressed) }
        else { unsafe { x11_mouse_button(button,i32::from(pressed)) } }
    }
    fn inject_key(&self,symbol:u64,pressed:bool)->i32 {
        if let Some(uinput)=&self.uinput {
            if let Some(code)=crate::uinput::keysym_to_evdev(symbol) { return Self::uinput_result(uinput.key(code,pressed)); }
        }
        if let Some(portal)=self.portal.lock().unwrap().as_ref() { return self.portal_result("keyboard", portal.key(symbol,pressed)); }
        if self.backend==crate::desktop_backend::DesktopBackend::Portal {return self.portal_result("keyboard", Err("Portal input session is not bound".into()));}
        if self.backend==crate::desktop_backend::DesktopBackend::Wayland { crate::wayland::key(symbol,pressed) }
        else { unsafe { x11_key_symbol(symbol as libc::c_ulong,i32::from(pressed)) } }
    }
    pub fn key_event(&self, keycode: i32, is_press: bool) {
        if self.backend!=crate::desktop_backend::DesktopBackend::X11 { return; }
        unsafe { x11_key_event(keycode, if is_press { 1 } else { 0 }) };
    }
}

#[cfg(test)]
mod motion_tests {
    use super::*;
    #[test]
    fn coalesces_absolute_history_to_latest_position() {
        let input = InputInjector::new();
        input.mouse_absolute_normalized(0.1,0.1,(1920,1080));
        input.mouse_absolute_normalized(0.9,0.9,(1920,1080));
        assert_eq!(input.pending.lock().unwrap().absolute,Some((1727,971)));
    }
    #[test]
    fn preserves_relative_distance_after_latest_absolute() {
        let input = InputInjector::new();
        input.mouse_relative(10,20);
        input.mouse_absolute_normalized(0.5,0.5,(1920,1080));
        input.mouse_relative(3,-2); input.mouse_relative(4,5);
        let state = input.pending.lock().unwrap();
        assert_eq!(state.absolute,Some((960,540)));
        assert_eq!(state.relative,(7,3));
    }
    #[test]
    fn button_transition_flushes_motion_and_teardown_discards_pending() {
        let input = InputInjector::new();
        input.mouse_absolute_normalized(0.5,0.5,(1920,1080));
        input.mouse_button(1,true);
        assert!(input.pending.lock().unwrap().absolute.is_none());
        input.mouse_relative(1,2); input.discard_pointer();
        assert_eq!(input.pending.lock().unwrap().relative,(0,0));
    }
}

#[cfg(test)]
mod shortcut_tests {
    use super::*;
    #[test]
    fn modifiers_are_tracked_and_non_terminal_keys_pass_through() {
        let input = InputInjector::new();
        assert!(!input.terminal_shift(0x11, true));
        assert_eq!(input.shortcuts.lock().unwrap().modifiers, 1);
        // No X display in tests: Ctrl+V is not translated.
        assert!(!input.terminal_shift(0x56, true));
        assert!(!input.terminal_shift(0x56, false));
        assert!(!input.terminal_shift(0x11, false));
        assert_eq!(input.shortcuts.lock().unwrap().modifiers, 0);
        // A translated key keeps its synthetic Shift until its own release.
        input.shortcuts.lock().unwrap().shifted.insert(0x56);
        assert!(input.terminal_shift(0x56, false));
        assert!(!input.terminal_shift(0x56, false));
        // Ctrl+C is never translated.
        input.terminal_shift(0x11, true);
        assert!(!input.terminal_shift(0x43, true));
    }
}

#[cfg(test)]
mod scroll_tests {
    use super::*;
    #[test]
    fn precise_scroll_accumulates_sub_notch_remainder() {
        let input = InputInjector::new();
        input.mouse_scroll(0, 2); input.flush_pointer();
        assert_eq!(input.pending.lock().unwrap().scroll,(0,2));
        input.mouse_scroll(0,118); input.flush_pointer();
        assert_eq!(input.pending.lock().unwrap().scroll,(0,0));
        input.mouse_scroll(-125,-125); input.flush_pointer();
        assert_eq!(input.pending.lock().unwrap().scroll,(-5,-5));
    }
}
