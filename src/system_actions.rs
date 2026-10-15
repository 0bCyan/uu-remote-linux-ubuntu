//! Official SimpleAction system commands from the viewer's 安全 / 更多 menus, mapped to the
//! Linux desktop session: lock (logind), show desktop, system monitor, reboot/power-off
//! (systemctl through logind/polkit), host mute (wpctl / pactl) and lock-after-session.
//! Declared through FeatureFlag ff_simple_action = FF_SIMPLE_ACTION_AUTO_LOCK (2, includes
//! MUTING) and ff_system_metrics = FF_SYSTEM_METRICS_SETTING_AUTO_LOCK (2).

use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

pub const FF_SIMPLE_ACTION: u8 = 2;
pub const FF_SYSTEM_METRICS: u8 = 2;

pub const SHOW_DESKTOP: u64 = 2;
pub const REBOOT_SYSTEM: u64 = 9;
pub const LOCK_SYSTEM: u64 = 10;
pub const SHUTDOWN_SYSTEM: u64 = 12;
pub const SHOW_TASKMGR: u64 = 13;
pub const MUTE_SYSTEM: u64 = 14;
pub const MUTE_RESTORE: u64 = 16;
pub const MUTE_PERMANENT: u64 = 17;
pub const MUTE_TEMPORARY: u64 = 18;
pub const SET_AUTO_LOCK: u64 = 19;

/// Host settings from the viewer's 安全 menu. The viewer sends them only when a switch is toggled
/// and shows what SystemMetrics reports, so they are host-wide and persisted (like the official
/// host), not per connection: a reconnect must still lock / mute.
#[derive(Debug, Default, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
struct Settings {
    #[serde(default)] auto_lock: bool,
    #[serde(default)] keep_mute: bool,
    #[serde(default)] mute_permanent: bool,
}

struct HostState {
    settings: Settings,
    /// Host mute state before remote control muted it (restored by MUTE_RESTORE / last session end).
    restore: Option<bool>,
    desktop_sessions: usize,
}

fn settings_path() -> std::path::PathBuf { crate::config::path().with_file_name("host_settings.json") }

fn state() -> &'static Mutex<HostState> {
    static STATE: std::sync::OnceLock<Mutex<HostState>> = std::sync::OnceLock::new();
    STATE.get_or_init(|| {
        let settings = std::fs::read(settings_path()).ok()
            .and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Mutex::new(HostState { settings, restore: None, desktop_sessions: 0 })
    })
}

fn save(settings: &Settings) {
    let path = settings_path();
    let result = serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())
        .and_then(|json| std::fs::write(&path, json).map_err(|e| e.to_string()));
    if let Err(e) = result { log::warn!("Host settings not saved: {}", e); }
}

/// Handle for one desktop connection (system actions are host-wide).
#[derive(Default)]
pub struct PeerActions { started: AtomicBool }

fn quiet(program: &str, args: &[&str]) -> bool {
    Command::new(program).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
        .status().map(|s| s.success()).unwrap_or(false)
}

fn detached(program: &str, args: &[&str]) -> bool {
    Command::new(program).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
        .spawn().map(|mut child| { std::thread::spawn(move || child.wait()); true }).unwrap_or(false)
}

/// Current mute state of the default output (None = no audio control available).
pub fn host_muted() -> Option<bool> {
    if let Ok(out) = Command::new("wpctl").args(["get-volume", "@DEFAULT_AUDIO_SINK@"]).output() {
        if out.status.success() { return Some(String::from_utf8_lossy(&out.stdout).contains("MUTED")); }
    }
    let out = Command::new("pactl").args(["get-sink-mute", "@DEFAULT_SINK@"]).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).contains("yes"))
}

pub fn set_host_muted(muted: bool) -> bool {
    let flag = if muted { "1" } else { "0" };
    quiet("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", flag]) || quiet("pactl", &["set-sink-mute", "@DEFAULT_SINK@", flag])
}

pub fn lock_session() -> bool { quiet("loginctl", &["lock-session"]) }

/// The on/off value of a toggle action: SimpleAction.value (field 5), else args JSON/text; default on.
pub fn toggle_value(value: Option<u64>, args: &[u8]) -> bool {
    if let Some(v) = value { return v != 0; }
    let text = String::from_utf8_lossy(args);
    let text = text.trim();
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(text) {
        let scalar = |v: &serde_json::Value| v.as_bool().or_else(|| v.as_i64().map(|n| n != 0))
            .or_else(|| v.as_str().map(|s| matches!(s, "1" | "true" | "on")));
        if let Some(b) = scalar(&json) { return b; }
        if let Some(b) = json.as_object().and_then(|o| o.values().find_map(scalar)) { return b; }
    }
    !matches!(text, "0" | "false" | "off")
}

/// Shape of action args for diagnostics: JSON keys and scalar types, never values.
pub fn args_shape(args: &[u8]) -> String {
    match serde_json::from_slice::<serde_json::Value>(args) {
        Ok(serde_json::Value::Object(o)) => format!("{:?}", o.iter().map(|(k, v)| (k.chars().take(32).collect::<String>(), match v {
            serde_json::Value::Bool(_) => "bool", serde_json::Value::Number(_) => "number",
            serde_json::Value::String(_) => "string", _ => "other" })).collect::<Vec<_>>()),
        Ok(_) => "json scalar".into(),
        Err(_) => format!("{} bytes non-json", args.len()),
    }
}

fn mute_for_session(state: &mut HostState) {
    if state.restore.is_none() {
        state.restore = host_muted();
        set_host_muted(true);
    }
    crate::virtual_audio::enable();
}
fn restore_mute(state: &mut HostState) {
    crate::virtual_audio::disable();
    if let Some(previous) = state.restore.take() { set_host_muted(previous); }
}

impl PeerActions {
    /// Runs one system action; returns true when the settings report should be resent.
    /// Blocking (spawns system tools); call from spawn_blocking.
    pub fn handle(&self, action: u64, value: Option<u64>, args: &[u8], show_desktop: impl FnOnce()) -> bool {
        match action {
            SHOW_DESKTOP => { show_desktop(); log::info!("System action show_desktop"); return false }
            LOCK_SYSTEM => { log::info!("System action lock_system: ok={}", lock_session()); return false }
            SHOW_TASKMGR => {
                let ok = detached("gnome-system-monitor", &[]) || detached("plasma-systemmonitor", &[]) || detached("xfce4-taskmanager", &[]);
                log::info!("System action show_taskmgr: ok={}", ok); return false
            }
            REBOOT_SYSTEM => { log::warn!("System action reboot_system requested by viewer"); log::info!("reboot ok={}", quiet("systemctl", &["reboot"])); return false }
            SHUTDOWN_SYSTEM => { log::warn!("System action shutdown_system requested by viewer"); log::info!("poweroff ok={}", quiet("systemctl", &["poweroff"])); return false }
            _ => {}
        }
        let mut state = state().lock().unwrap();
        let before = state.settings;
        match action {
            // 静音运行 on: the viewer sends MUTE_SYSTEM + MUTE_TEMPORARY (observed), off: MUTE_RESTORE.
            MUTE_SYSTEM | MUTE_TEMPORARY => {
                let on = toggle_value(value, args);
                state.settings.keep_mute = on;
                if on { mute_for_session(&mut state); } else { restore_mute(&mut state); }
                log::info!("System action mute (action {}): on={} muted={:?}", action, on, host_muted());
            }
            // Keep the host muted after the session (远程结束后，恢复被控端声音设置 unchecked).
            MUTE_PERMANENT => {
                let on = toggle_value(value, args);
                state.settings.mute_permanent = on;
                if on { state.settings.keep_mute = true; mute_for_session(&mut state); }
                log::info!("System action mute_permanent: on={} muted={:?}", on, host_muted());
            }
            MUTE_RESTORE => {
                state.settings.keep_mute = false;
                state.settings.mute_permanent = false;
                restore_mute(&mut state);
                log::info!("System action mute_restore: muted={:?}", host_muted());
            }
            SET_AUTO_LOCK => {
                // Observed: enabling 远程结束后自动锁屏 sends value 2 and disabling value 1.
                let on = match value { Some(v) => v == 2, None => toggle_value(None, args) };
                state.settings.auto_lock = on;
                log::info!("System action set_auto_lock: on={}", on);
            }
            _ => return false,
        }
        if state.settings != before { save(&state.settings); }
        true
    }

    /// Desktop connection started: re-apply 静音运行 for the first remote session.
    pub fn session_started(&self) {
        if self.started.swap(true, Ordering::Relaxed) { return; }
        let mut state = state().lock().unwrap();
        state.desktop_sessions += 1;
        if state.desktop_sessions == 1 && state.settings.keep_mute {
            mute_for_session(&mut state);
            log::info!("Host muted for remote session (静音运行)");
        }
    }

    /// Desktop connection ended. After the last one: undo a session-only mute and lock if the
    /// viewer asked for it.
    pub fn session_ended(&self) {
        if !self.started.swap(false, Ordering::Relaxed) { return; }
        let mut state = state().lock().unwrap();
        state.desktop_sessions = state.desktop_sessions.saturating_sub(1);
        if state.desktop_sessions > 0 { return; }
        crate::virtual_audio::disable();
        if !state.settings.mute_permanent { restore_mute(&mut state); }
        if state.settings.auto_lock {
            log::info!("Auto lock after remote session: ok={}", lock_session());
        }
    }

    /// Message 13 SystemMetrics: permissions, file-transfer folder and the current settings.
    pub fn metrics_payload(&self, sound: bool, desktop: crate::system_metrics::Permission) -> Vec<u8> {
        use crate::system_metrics::payload_with_settings;
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
        let current = state().lock().unwrap().settings;
        payload_with_settings(desktop, desktop, desktop, &home, &crate::system_metrics::Settings {
            sound, keep_mute: current.keep_mute, mute_permanent: current.mute_permanent, auto_lock: current.auto_lock,
        })
    }
}

/// Session RPCs answered with success: 15 send_video_track_req -> 11 (the single desktop track
/// 0 is always sent), 21 virtual_audio_driver_policy_req -> 17 (no virtual driver needed: the
/// sink monitor is captured directly). Responses carry error_code 0 (default).
pub fn session_rpc_response(rpc: &[crate::protocol::WireField]) -> Option<Vec<u8>> {
    use crate::protocol::{decode_wire, WireValue};
    let body = |n: u32| rpc.iter().find_map(|f| match (f.number, &f.value) { (x, WireValue::Bytes(b)) if x == n => Some(*b), _ => None });
    let (request, response_field) = [(15u32, 11u8), (21, 17)].into_iter().find_map(|(req, resp)| body(req).map(|b| (b, resp)))?;
    let value = decode_wire(request).ok().and_then(|f| f.iter().find_map(|f| match (f.number, &f.value) { (1, WireValue::Varint(v)) => Some(*v), _ => None }));
    log::info!("Session RPC answered: response_field={} value={:?}", response_field, value);
    let header = body(1).unwrap_or(&[]);
    let mut out = vec![10, header.len() as u8];
    out.extend_from_slice(header);
    out.extend_from_slice(&[response_field << 3 | 2, 0]);
    Some(out)
}

/// Plugin RPCs (魔兽世界排队提醒工具): no plugins exist on Linux. Query answers "disabled"
/// (QueryPluginSettingResponse field 12); enable/install/notification requests answer error_code 1.
pub fn plugin_response(rpc: &[crate::protocol::WireField]) -> Option<Vec<u8>> {
    use crate::protocol::{decode_wire, WireValue};
    fn varint(mut v: u64, out: &mut Vec<u8>) { while v >= 128 { out.push((v as u8 & 127) | 128); v >>= 7; } out.push(v as u8); }
    fn bytes(field: u64, v: &[u8], out: &mut Vec<u8>) { varint(field << 3 | 2, out); varint(v.len() as u64, out); out.extend_from_slice(v); }
    let body = |n: u32| rpc.iter().find_map(|f| match (f.number, &f.value) { (x, WireValue::Bytes(b)) if x == n => Some(*b), _ => None });
    // (request field, response field, plugin_id field in the response)
    let (request, response_field, id_field): (&[u8], u64, u64) = [(16u32, 12u64, 3u64), (17, 13, 3), (18, 14, 2), (19, 15, 2), (20, 16, 2)]
        .into_iter().find_map(|(req, resp, id)| body(req).map(|b| (b, resp, id)))?;
    let plugin_id = decode_wire(request).ok()?.iter().find_map(|f| match (f.number, &f.value) { (1, WireValue::Bytes(b)) => Some(b.to_vec()), _ => None }).unwrap_or_default();
    let mut payload = Vec::new();
    if response_field != 12 { payload.extend_from_slice(&[8, 1]); } // error_code 1: unsupported
    bytes(id_field, &plugin_id, &mut payload); // query: error_code 0, enable false (defaults)
    let mut out = Vec::new();
    bytes(1, body(1).unwrap_or(&[]), &mut out);
    bytes(response_field, &payload, &mut out);
    log::info!("Plugin RPC answered: response_field={} (no Linux plugins)", response_field);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn toggle_values_from_value_field_json_and_text() {
        assert!(!toggle_value(Some(0), b"1"));
        assert!(toggle_value(Some(1), b""));
        assert!(!toggle_value(None, br#"{"enable":false}"#));
        assert!(toggle_value(None, br#"{"value":1}"#));
        assert!(!toggle_value(None, b"0"));
        assert!(toggle_value(None, b""));
    }
    #[test]
    fn plugin_query_answers_disabled_with_same_request_and_plugin_id() {
        use crate::protocol::{decode_wire, WireValue};
        // RpcRequest { 1 header{1:9}, 16 query{1:"wow"} }
        let request = [10, 2, 8, 9, 130, 1, 5, 10, 3, b'w', b'o', b'w'];
        let response = plugin_response(&decode_wire(&request).unwrap()).unwrap();
        let fields = decode_wire(&response).unwrap();
        assert_eq!(fields[0].value, WireValue::Bytes(&[8, 9]));
        assert_eq!(fields[1].number, 12);
        assert_eq!(fields[1].value, WireValue::Bytes(&[26, 3, b'w', b'o', b'w']));
        assert!(plugin_response(&decode_wire(&[10, 2, 8, 9]).unwrap()).is_none());
    }
    #[test]
    fn args_shape_never_contains_values() {
        let shape = args_shape(br#"{"enable":true,"secret":"abc"}"#);
        assert!(shape.contains("enable") && !shape.contains("abc"));
    }
}
