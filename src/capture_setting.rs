//! Viewer quality/frame-rate settings (画质 / 帧率) for the desktop stream.
//!
//! The viewer only sends settings to a host whose echo FeatureFlag declares ff_capture_setting
//! (field 1); our echo declares FF_CAPTURE_SETTING_CUSTOM_BITRATE = 4. Requests arrive as
//! Message 21 RpcRequest { 1 RequestHeader, 2 CaptureSettingRequest | 3 FpsSettingRequest |
//! 4 VideoQualitySettingReqeust } and are answered with Message 22 RpcResponse { 1 ResponseHeader
//! (same request_id), 2 CaptureSettingResponse { 1 errors (none = applied) } }. The applied values
//! are then reported with Message 25 CaptureSettingSync. Field maps: server-field-maps.json.

use crate::protocol::{decode_wire, WireField, WireValue};
use tokio::sync::watch;

/// Official GvPb_VideoQuality values.
pub const QUALITY_FAST: u64 = 1;
pub const QUALITY_GENERAL: u64 = 2;
pub const QUALITY_HD: u64 = 3;
pub const QUALITY_BLURAY: u64 = 4;
pub const QUALITY_AUTO: u64 = 5;
pub const QUALITY_CUSTOM: u64 = 6;

/// FeatureFlag.ff_capture_setting we declare: fps, preset quality and custom bitrate.
/// Versions are cumulative (official FF_CaptureSetting enum: 3 CHROMA_SAMPLING, 4 CUSTOM_BITRATE,
/// 5 DPI_SCALE, 6 HDR), so 4 also tells the viewer we accept a chroma format (真彩).
pub const FEATURE_VERSION: u64 = 4;

/// Official GvPb_ChromaFormat values (CaptureSettingRequest field 7, CaptureSettingSync field 3).
pub const CHROMA_420: u64 = 1;
pub const CHROMA_444: u64 = 3;

const MIN_FPS: u32 = 5;
const MAX_FPS: u32 = 144;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoSettings {
    pub fps: u32,
    pub quality: u64,
    /// Viewer max_custom_bitrate (only used for QUALITY_CUSTOM): Mbit/s, or kbit/s above 1000.
    pub custom_mbps: u32,
    /// Scaled stream size (其他分辨率); None streams the native desktop size.
    pub output: Option<(u32, u32)>,
    /// CHROMA_420 or CHROMA_444 (真彩); 4:4:4 only when the encoder pipeline supports it.
    pub chroma: u64,
}

impl Default for VideoSettings {
    fn default() -> Self { Self { fps: 60, quality: QUALITY_AUTO, custom_mbps: 0, output: None, chroma: CHROMA_420 } }
}

/// Selectable smaller stream sizes with the desktop's aspect ratio (even dimensions).
/// The host display itself is never reconfigured; the encoder scales.
pub fn scaled_sizes(native: (u32, u32)) -> Vec<(u32, u32)> {
    [5u32, 4, 3, 2].iter().map(|sixths| (native.0 * sixths / 6 & !1, native.1 * sixths / 6 & !1))
        .filter(|(w, h)| *w >= 640 && *h >= 360).collect()
}

/// Requested size clamped to the desktop: None (native) when at least as large.
fn clamp_output(native: (u32, u32), width: u64, height: u64) -> Option<(u32, u32)> {
    if width == 0 || height == 0 { return None; }
    if width >= u64::from(native.0) || height >= u64::from(native.1) { return None; }
    Some(((width as u32).max(320) & !1, (height as u32).max(180) & !1))
}

impl VideoSettings {
    /// Constant-quality level: presets must look different even on a mostly static desktop,
    /// where a bitrate cap alone never binds (measured ~2.7 Mbit/s at CQ18).
    pub fn cq(&self) -> u32 {
        match self.quality { QUALITY_BLURAY => 17, QUALITY_HD => 21, QUALITY_GENERAL => 26, QUALITY_FAST => 30, _ => 18 }
    }

    /// Encoder bitrate cap in kbit/s; None keeps the constant-quality default (自动).
    /// Preset caps follow the viewer menu labels: 高清 2M, 超清 8M, 原画 20M.
    pub fn max_kbps(&self) -> Option<u32> {
        match self.quality {
            QUALITY_FAST => Some(1_000),
            QUALITY_GENERAL => Some(2_000),
            QUALITY_HD => Some(8_000),
            QUALITY_BLURAY => Some(20_000),
            QUALITY_CUSTOM if self.custom_mbps > 0 =>
                Some(if self.custom_mbps > 1_000 { self.custom_mbps } else { self.custom_mbps * 1_000 }.min(100_000)),
            _ => None,
        }
    }
}

fn varint(mut v: u64, out: &mut Vec<u8>) { while v >= 128 { out.push((v as u8 & 127) | 128); v >>= 7; } out.push(v as u8); }
fn put_varint(field: u64, v: u64, out: &mut Vec<u8>) { varint(field << 3, out); varint(v, out); }
fn put_bytes(field: u64, v: &[u8], out: &mut Vec<u8>) { varint(field << 3 | 2, out); varint(v.len() as u64, out); out.extend_from_slice(v); }

fn number(fields: &[WireField], n: u32) -> Option<u64> {
    fields.iter().find_map(|f| match (f.number, &f.value) { (x, WireValue::Varint(v)) if x == n => Some(*v), _ => None })
}
fn message<'a>(fields: &[WireField<'a>], n: u32) -> Option<&'a [u8]> {
    fields.iter().find_map(|f| match (f.number, &f.value) { (x, WireValue::Bytes(b)) if x == n => Some(*b), _ => None })
}

/// Frame rate from a value that is either an fps option index (observed 1 = 30, 2 = 60;
/// 3/4 assumed 90/144 from the menu order) or already frames per second.
fn fps_from_option(value: u64) -> Option<u64> {
    match value { 0 => None, 1 => Some(30), 2 => Some(60), 3 => Some(90), 4 => Some(144), v => Some(v) }
}

/// Applies a capture-setting RPC to `settings` and returns the RpcResponse payload (Message 22),
/// or None when the RPC is not a capture/fps/quality setting.
pub fn handle_rpc(rpc: &[WireField], settings: &watch::Sender<VideoSettings>, native: (u32, u32)) -> Option<Vec<u8>> {
    apply_rpc(rpc, settings, native, crate::video::full_chroma_available())
}

fn apply_rpc(rpc: &[WireField], settings: &watch::Sender<VideoSettings>, native: (u32, u32), full_chroma: bool) -> Option<Vec<u8>> {
    let header = message(rpc, 1).unwrap_or(&[]);
    let mut next = *settings.borrow();
    if let Some(request) = message(rpc, 2) {
        let fields = decode_wire(request).ok()?;
        // Numbers only: fps, quality, bitrate, resolution and flags carry no user content.
        log::info!("Capture setting request: {:?}", fields.iter().filter_map(|f| match f.value {
            WireValue::Varint(v) => Some((f.number, v)), _ => None }).collect::<Vec<_>>());
        // Field 1 is the viewer's fps option index (1 = 30, 2 = 60, ...); 18 fpsCount is the frame rate.
        if let Some(fps) = number(&fields, 18).filter(|v| *v > 0).or_else(|| number(&fields, 1).and_then(fps_from_option)) { next.fps = fps as u32; }
        if let Some(quality) = number(&fields, 2).filter(|v| *v > 0) { next.quality = quality; }
        if let Some(bitrate) = number(&fields, 8).filter(|v| *v > 0) { next.custom_mbps = bitrate as u32; }
        // Field 7 chorma_format (sic): 3 = 4:4:4 when 真彩 is on, 1 = 4:2:0. The Sync reports what is encoded.
        match number(&fields, 7) {
            Some(CHROMA_444) if full_chroma => next.chroma = CHROMA_444,
            Some(CHROMA_444) => { log::info!("Chroma 4:4:4 requested; this encoder pipeline only supports 4:2:0"); next.chroma = CHROMA_420; }
            Some(CHROMA_420) => next.chroma = CHROMA_420,
            _ => {}
        }
        // ChooseResolutionType 3 = follow remote (native); otherwise pixel size, else logical size.
        if number(&fields, 10) == Some(3) { next.output = None; }
        else if let Some((w, h)) = number(&fields, 16).zip(number(&fields, 17)).or_else(|| number(&fields, 5).zip(number(&fields, 6))) {
            next.output = clamp_output(native, w, h);
        }
    } else if let Some(request) = message(rpc, 6) {
        let fields = decode_wire(request).ok()?;
        let (w, h) = (number(&fields, 1).unwrap_or(0), number(&fields, 2).unwrap_or(0));
        log::info!("Resolution setting request: {}x{}", w, h);
        next.output = clamp_output(native, w, h);
    } else if let Some(request) = message(rpc, 3) {
        let fields = decode_wire(request).ok()?;
        log::info!("Fps setting request: fps={:?}", number(&fields, 1));
        if let Some(fps) = number(&fields, 1).and_then(fps_from_option) { next.fps = fps as u32; }
    } else if let Some(request) = message(rpc, 4) {
        let fields = decode_wire(request).ok()?;
        log::info!("Video quality setting request: quality={:?}", number(&fields, 1));
        if let Some(quality) = number(&fields, 1).filter(|v| *v > 0) { next.quality = quality; }
    } else {
        return None;
    }
    next.fps = next.fps.clamp(MIN_FPS, MAX_FPS);
    settings.send_if_modified(|current| if *current != next {
        log::info!("Desktop video settings: fps={} quality={} max_kbps={:?} output={:?} chroma={}", next.fps, next.quality, next.max_kbps(), next.output, next.chroma);
        *current = next; true
    } else { false });
    let mut out = Vec::new();
    put_bytes(1, header, &mut out);
    put_bytes(2, &[], &mut out); // CaptureSettingResponse without errors: applied
    Some(out)
}

/// Message 7 ScreenSources for the single desktop: native size, scaled choices, current size.
pub fn screen_sources(seq: u64, timestamp: u64, native: (u32, u32), output: Option<(u32, u32)>, name: &str) -> Result<Vec<u8>, String> {
    use crate::initialization::{Screen, WinRect};
    let (w, h) = (native.0 as i32, native.1 as i32);
    let scaled: Vec<(i32, i32)> = scaled_sizes(native).into_iter().map(|(w, h)| (w as i32, h as i32)).collect();
    let resolution = WinRect { left: 0, top: 0, width: w, height: h, pixel_width: w, pixel_height: h };
    let screens = [Screen { id: 0, fps: crate::video::FPS as i32, resolution, primary: true, dpr: 1.0,
        display_name: name, video_track_index: 0, screen_type: 0, builtin_screen_type: 0,
        scaled: &scaled, current: output.map(|(w, h)| (w as i32, h as i32)) }];
    crate::initialization::encode_screen_sources(seq, timestamp, &screens, 0)
}

/// Message 25 CaptureSettingSync payload reporting what the host now encodes.
pub fn sync_payload(settings: &VideoSettings, native: (u32, u32)) -> Vec<u8> {
    let size = settings.output.unwrap_or(native);
    let mut out = Vec::new();
    put_varint(1, u64::from(settings.fps), &mut out);
    put_varint(2, settings.quality, &mut out);
    put_varint(3, settings.chroma, &mut out);
    if settings.custom_mbps > 0 { put_varint(4, u64::from(settings.custom_mbps), &mut out); }
    put_varint(5, u64::from(size.0), &mut out);
    put_varint(6, u64::from(size.1), &mut out);
    put_varint(9, u64::from(size.0), &mut out);
    put_varint(10, u64::from(size.1), &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rpc(request_id: u64, field: u64, body: &[u8]) -> Vec<u8> {
        let mut header = Vec::new();
        put_varint(1, request_id, &mut header);
        let mut out = Vec::new();
        put_bytes(1, &header, &mut out);
        put_bytes(field, body, &mut out);
        out
    }

    #[test]
    fn capture_setting_request_updates_fps_and_quality_and_echoes_request_id() {
        let (tx, rx) = watch::channel(VideoSettings::default());
        let mut body = Vec::new();
        put_varint(1, 1, &mut body); // option index, not fps
        put_varint(2, QUALITY_GENERAL, &mut body);
        put_varint(18, 30, &mut body);
        let request = rpc(77, 2, &body);
        let response = handle_rpc(&decode_wire(&request).unwrap(), &tx, (1920, 1080)).unwrap();
        assert_eq!(*rx.borrow(), VideoSettings { fps: 30, quality: QUALITY_GENERAL, custom_mbps: 0, output: None, chroma: CHROMA_420 });
        assert_eq!(rx.borrow().max_kbps(), Some(2_000));
        let fields = decode_wire(&response).unwrap();
        assert_eq!(message(&fields, 1), Some(&[8u8, 77][..]));
        assert_eq!(message(&fields, 2), Some(&[][..]));
    }

    #[test]
    fn separate_fps_and_quality_requests_and_limits() {
        let (tx, rx) = watch::channel(VideoSettings::default());
        handle_rpc(&decode_wire(&rpc(1, 3, &[8, 200, 1])).unwrap(), &tx, (1920, 1080)).unwrap();
        assert_eq!(rx.borrow().fps, MAX_FPS);
        // Observed viewer request: option 2 with fpsCount 60 must stream 60 fps, not 2.
        let observed = [8, 2, 16, 4, 40, 128, 15, 48, 184, 8, 144, 1, 60];
        handle_rpc(&decode_wire(&rpc(4, 2, &observed)).unwrap(), &tx, (1920, 1080)).unwrap();
        assert_eq!(rx.borrow().fps, 60);
        handle_rpc(&decode_wire(&rpc(2, 4, &[8, QUALITY_BLURAY as u8])).unwrap(), &tx, (1920, 1080)).unwrap();
        assert_eq!(rx.borrow().max_kbps(), Some(20_000));
        assert!(handle_rpc(&decode_wire(&rpc(3, 9, &[])).unwrap(), &tx, (1920, 1080)).is_none());
    }

    #[test]
    fn true_color_request_selects_444_only_when_the_encoder_supports_it() {
        let (tx, rx) = watch::channel(VideoSettings::default());
        // Observed Mac viewer request with 真彩 on: field 7 = 3.
        let request = rpc(5, 2, &[8, 2, 16, 4, 56, 3, 144, 1, 60]);
        apply_rpc(&decode_wire(&request).unwrap(), &tx, (1920, 1080), false).unwrap();
        assert_eq!(rx.borrow().chroma, CHROMA_420);
        apply_rpc(&decode_wire(&request).unwrap(), &tx, (1920, 1080), true).unwrap();
        assert_eq!(rx.borrow().chroma, CHROMA_444);
        assert_eq!(number(&decode_wire(&sync_payload(&rx.borrow(), (1920, 1080))).unwrap(), 3), Some(CHROMA_444));
        apply_rpc(&decode_wire(&rpc(6, 2, &[56, 1])).unwrap(), &tx, (1920, 1080), true).unwrap();
        assert_eq!(rx.borrow().chroma, CHROMA_420);
    }

    #[test]
    fn resolution_requests_scale_below_native_only() {
        let (tx, rx) = watch::channel(VideoSettings::default());
        handle_rpc(&decode_wire(&rpc(1, 6, &[8, 128, 10, 16, 208, 5])).unwrap(), &tx, (1920, 1080)).unwrap();
        assert_eq!(rx.borrow().output, Some((1280, 720)));
        handle_rpc(&decode_wire(&rpc(2, 6, &[8, 160, 32, 16, 160, 20])).unwrap(), &tx, (1920, 1080)).unwrap();
        assert_eq!(rx.borrow().output, None);
        assert_eq!(scaled_sizes((1920, 1080)), vec![(1600, 900), (1280, 720), (960, 540), (640, 360)]);
    }

    #[test]
    fn auto_quality_keeps_constant_quality_encoding() {
        assert_eq!(VideoSettings::default().max_kbps(), None);
        assert_eq!(VideoSettings { quality: QUALITY_CUSTOM, custom_mbps: 0, ..Default::default() }.max_kbps(), None);
        assert_eq!(VideoSettings { quality: QUALITY_CUSTOM, custom_mbps: 12, ..Default::default() }.max_kbps(), Some(12_000));
    }

    #[test]
    fn sync_reports_fps_quality_and_resolution() {
        let payload = sync_payload(&VideoSettings::default(), (1920, 1080));
        let fields = decode_wire(&payload).unwrap();
        assert_eq!(number(&fields, 1), Some(60));
        assert_eq!(number(&fields, 2), Some(QUALITY_AUTO));
        assert_eq!(number(&fields, 5), Some(1920));
        assert_eq!(number(&fields, 10), Some(1080));
    }
}
