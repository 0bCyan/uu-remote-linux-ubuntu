//! Experimental linear KMS framebuffer capture. Never selected automatically.
#[cfg(all(feature="kms",target_os="linux"))]
mod imp {
    use std::ffi::CString;
    use std::sync::Mutex;
    extern "C" {
        fn uu_kms_open(device:*const libc::c_char,w:*mut u32,h:*mut u32)->*mut libc::c_void;
        fn uu_kms_read(handle:*mut libc::c_void,out:*mut u8,length:usize)->i32;
        fn uu_kms_close(handle:*mut libc::c_void);
    }
    struct Handle(*mut libc::c_void);
    // The FD/CRTC handle is exclusively accessed through Capture's mutex.
    unsafe impl Send for Handle {}
    impl Drop for Handle { fn drop(&mut self) { unsafe { uu_kms_close(self.0) }; } }
    pub struct Capture { handle:Mutex<Handle>, pub width:u32, pub height:u32 }
    impl Capture {
        pub fn open()->Result<Self,String> {
            let device=CString::new(std::env::var("UU_KMS_DEVICE").unwrap_or_else(|_|"/dev/dri/card0".into())).map_err(|_|"Invalid KMS device")?;
            let (mut width,mut height)=(0,0);
            let handle=unsafe { uu_kms_open(device.as_ptr(),&mut width,&mut height) };
            if handle.is_null() { return Err("KMS: cannot open an active CRTC; check card permissions and connected output".into()); }
            let capture=Self { handle:Mutex::new(Handle(handle)), width,height };
            // Do not advertise a desktop until we have actually read the buffer.
            let mut frame=vec![0; width as usize*height as usize*4];
            capture.capture(&mut frame)?;
            if frame.chunks_exact(4).all(|p| p[..3]==[0,0,0]) {
                log::warn!("KMS initial frame is uniformly black; GPU buffer visibility is unverified (see KMS limitations)");
            }
            Ok(capture)
        }
        pub fn capture(&self,output:&mut [u8])->Result<(),String> {
            if output.len()!=self.width as usize*self.height as usize*4 { return Err("KMS: invalid frame dimensions".into()); }
            let handle=self.handle.lock().unwrap();
            match unsafe { uu_kms_read(handle.0,output.as_mut_ptr(),output.len()) } {
                0=>Ok(()), -2=>Err("KMS mode changed; restart host to refresh desktop geometry".into()),
                -5=>Err("KMS framebuffer handles unavailable; requires cap_sys_admin+ep".into()),
                code=>Err(format!("KMS capture failed ({code}); prototype needs a mappable linear XRGB/ARGB dumb buffer")),
            }
        }
    }
}
#[cfg(all(feature="kms",target_os="linux"))]
pub use imp::Capture;
#[cfg(not(all(feature="kms",target_os="linux")))]
pub struct Capture { pub width:u32, pub height:u32 }
#[cfg(not(all(feature="kms",target_os="linux")))]
impl Capture {
    pub fn open()->Result<Self,String> { Err("KMS requires Linux and --features kms".into()) }
    pub fn capture(&self,_:&mut [u8])->Result<(),String> { Err("KMS requires Linux and --features kms".into()) }
}
