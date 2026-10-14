//! Native Linux X11 Input Injection Engine via XTest

#[cfg(target_os = "linux")]
extern "C" {
    fn x11_mouse_move(x: libc::c_int, y: libc::c_int) -> libc::c_int;
    fn x11_mouse_relative(dx: libc::c_int, dy: libc::c_int) -> libc::c_int;
    fn x11_mouse_button(button: libc::c_int, is_press: libc::c_int) -> libc::c_int;
    fn x11_pointer_state(x:*mut libc::c_int,y:*mut libc::c_int,mask:*mut libc::c_uint)->libc::c_int;
    fn x11_key_symbol(symbol: libc::c_ulong, is_press: libc::c_int) -> libc::c_int;
    fn x11_key_event(keycode: libc::c_int, is_press: libc::c_int) -> libc::c_int;
}

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
pub struct InputInjector { portal:std::sync::Mutex<Option<std::sync::Arc<crate::portal::PortalSession>>>, backend: crate::desktop_backend::DesktopBackend, pending: std::sync::Mutex<PendingPointer>, held: std::sync::Mutex<HeldInput> }

impl InputInjector {
    pub fn new() -> Self {
        Self { portal:std::sync::Mutex::new(None), backend:crate::desktop_backend::detect().unwrap_or(crate::desktop_backend::DesktopBackend::X11), pending: std::sync::Mutex::new(PendingPointer::default()), held:std::sync::Mutex::new(HeldInput::default()) }
    }

    pub fn bind_portal(&self,session:Option<std::sync::Arc<crate::portal::PortalSession>>) { *self.portal.lock().unwrap()=session; }
    pub fn mouse_move(&self, x: i32, y: i32) {
        self.inject_move(x,y,false);
    }

    pub fn mouse_absolute_normalized(&self, x: f64, y: f64, size: (u32,u32)) {
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
    }
    pub fn mouse_button(&self, button: i32, is_press: bool) {
        self.flush_pointer();
        let result = self.inject_button(button,is_press);
        if result == 0 {
            let mut held = self.held.lock().unwrap();
            if is_press { held.buttons.insert(button); } else { held.buttons.remove(&button); }
        } else { log::warn!("Desktop mouse injection failed: {}",result); }
        static BUTTON_AUDIT:std::sync::atomic::AtomicUsize=std::sync::atomic::AtomicUsize::new(0);
        if self.backend==crate::desktop_backend::DesktopBackend::X11 && BUTTON_AUDIT.fetch_add(1,std::sync::atomic::Ordering::Relaxed)<24 {
            let (mut x,mut y,mut mask)=(0,0,0);
            let query=unsafe {x11_pointer_state(&mut x,&mut y,&mut mask)};
            log::info!("Pointer button audit: button={} down={} result={} query={} x={} y={} mask={}",button,is_press,result,query,x,y,mask);
        }
    }

    pub fn windows_key_event(&self, key: i32, is_press: bool) {
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
    fn inject_move(&self,x:i32,y:i32,relative:bool)->i32 {
        if let Some(portal)=self.portal.lock().unwrap().as_ref() { return if portal.move_pointer(f64::from(x),f64::from(y),relative).is_ok() {0} else {-1}; }
        if self.backend==crate::desktop_backend::DesktopBackend::Portal {return -1;}
        if self.backend==crate::desktop_backend::DesktopBackend::Wayland { crate::wayland::move_pointer(x,y,relative) }
        else if relative { unsafe { x11_mouse_relative(x,y) } } else { unsafe { x11_mouse_move(x,y) } }
    }
    fn inject_button(&self,button:i32,pressed:bool)->i32 {
        if let Some(portal)=self.portal.lock().unwrap().as_ref() { return if portal.button(button,pressed).is_ok() {0} else {-1}; }
        if self.backend==crate::desktop_backend::DesktopBackend::Portal {return -1;}
        if self.backend==crate::desktop_backend::DesktopBackend::Wayland { crate::wayland::button(button,pressed) }
        else { unsafe { x11_mouse_button(button,i32::from(pressed)) } }
    }
    fn inject_key(&self,symbol:u64,pressed:bool)->i32 {
        if let Some(portal)=self.portal.lock().unwrap().as_ref() { return if portal.key(symbol,pressed).is_ok() {0} else {-1}; }
        if self.backend==crate::desktop_backend::DesktopBackend::Portal {return -1;}
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
