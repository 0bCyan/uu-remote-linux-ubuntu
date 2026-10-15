//! Scoped PulseAudio / PipeWire-pulse routing. Never inspect or log audio payloads.
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::os::fd::AsRawFd;
const SINK: &str = "uu-remote-sink";
const MONITOR: &str = "uu-remote-sink.monitor";
#[derive(serde::Serialize, serde::Deserialize)]
struct Route { previous: String, module: Option<u32> }
struct State { route: Option<Route>, _lock: std::fs::File }
static STATE: OnceLock<Mutex<Option<State>>> = OnceLock::new();
fn path() -> std::path::PathBuf { crate::config::path().with_file_name("virtual_audio.json") }
fn pactl(args: &[&str]) -> Result<String, ()> {
    let out = Command::new("pactl").args(args).env("LC_ALL","C").stdin(Stdio::null()).stderr(Stdio::null()).output().map_err(|_| ())?;
    if !out.status.success() { return Err(()); }
    String::from_utf8(out.stdout).map(|s|s.trim().to_owned()).map_err(|_| ())
}
fn owned_modules(list: &str) -> Vec<u32> {
    list.lines().filter_map(|line| {
        let mut fields = line.split('\t');
        let id = fields.next()?.parse().ok()?;
        if fields.next()? != "module-null-sink" { return None; }
        fields.next()?.split_whitespace().any(|arg| arg == "sink_name=uu-remote-sink").then_some(id)
    }).collect()
}
fn sink_id(name: &str) -> Option<String> {
    pactl(&["list","short","sinks"]).ok()?.lines().find_map(|line| {
        let mut f=line.split_whitespace(); let id=f.next()?; (f.next()? == name).then(|| id.to_owned())
    })
}
fn move_inputs(from: &str, to: &str) {
    let Some(id)=sink_id(from) else { return };
    if let Ok(list)=pactl(&["list","short","sink-inputs"]) {
        for line in list.lines() {
            let mut f=line.split_whitespace();
            if let (Some(input),Some(sink))=(f.next(),f.next()) {
                if sink==id { let _=pactl(&["move-sink-input",input,to]); }
            }
        }
    }
}
fn persist(route: &Route) -> Result<(), ()> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::io::Write;
    let temp=path().with_extension("json.new");
    let mut file=std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&temp).map_err(|_| ())?;
    file.write_all(&serde_json::to_vec(route).map_err(|_| ())?).map_err(|_| ())?;
    file.sync_all().map_err(|_| ())?;
    std::fs::rename(temp,path()).map_err(|_| ())
}
fn recover(route: &Route) -> Result<(), ()> {
    // Do not overwrite a default explicitly changed by the user during remote control.
    if pactl(&["get-default-sink"])? == SINK { pactl(&["set-default-sink",&route.previous])?; }
    move_inputs(SINK,&route.previous);
    let modules=owned_modules(&pactl(&["list","short","modules"])?);
    for id in modules { pactl(&["unload-module",&id.to_string()])?; }
    let _=std::fs::remove_file(path());
    Ok(())
}
fn state() -> &'static Mutex<Option<State>> {
    STATE.get_or_init(|| {
        use std::os::unix::fs::OpenOptionsExt;
        let init=(|| {
            std::fs::create_dir_all(path().parent().ok_or(())?).map_err(|_| ())?;
            let lock=std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).mode(0o600)
                .open(path().with_extension("lock")).map_err(|_| ())?;
            if unsafe { libc::flock(lock.as_raw_fd(),libc::LOCK_EX|libc::LOCK_NB) } != 0 { return Err(()); }
            let route=if path().exists() {
                let old:Route=serde_json::from_slice(&std::fs::read(path()).map_err(|_| ())?).map_err(|_| ())?;
                if recover(&old).is_err() { Some(old) } else { None }
            } else { None };
            Ok(State { route, _lock:lock })
        })();
        Mutex::new(init.ok())
    })
}
/// Called on startup even if mute is disabled, to recover a previous crash.
pub fn init() { let _=state(); }
pub fn enable() -> bool {
    let mut guard=state().lock().unwrap();
    let Some(state)=guard.as_mut() else { return false };
    if state.route.is_some() { return pactl(&["get-default-sink"]).is_ok_and(|s|s==SINK); }
    let result=(|| {
        let previous=pactl(&["get-default-sink"])?;
        if previous==SINK { return Err(()); }
        // Refuse to claim another instance's or manually-created sink.
        if sink_id(SINK).is_some() { return Err(()); }
        let mut route=Route { previous, module:None };
        persist(&route)?; // Save before loading: also covers a crash between load and ID persistence.
        state.route=Some(Route { previous:route.previous.clone(), module:None });
        let id=pactl(&["load-module","module-null-sink","sink_name=uu-remote-sink","sink_properties=device.description=UU-Remote"])?;
        route.module=Some(id.parse().map_err(|_| ())?);
        persist(&route)?;
        state.route=Some(route);
        pactl(&["set-default-sink",SINK])?;
        move_inputs(&state.route.as_ref().ok_or(())?.previous,SINK);
        Ok(())
    })();
    if result.is_err() {
        if let Some(route)=state.route.as_ref() { if recover(route).is_ok() { state.route=None; } }
        log::warn!("Virtual audio routing unavailable; using the existing mute/monitor path");
    }
    result.is_ok()
}
pub fn disable() {
    let mut guard=state().lock().unwrap();
    if let Some(state)=guard.as_mut() {
        if let Some(route)=state.route.as_ref() {
            if recover(route).is_ok() { state.route=None; }
            else { log::warn!("Virtual audio restore incomplete; recovery record retained"); }
        }
    }
}
pub fn active() -> bool { state().lock().unwrap().as_ref().is_some_and(|s|s.route.is_some()) }
pub fn source() -> String {
    if state().lock().unwrap().as_ref().is_some_and(|s|s.route.as_ref().is_some_and(|r|r.module.is_some())) { MONITOR.into() }
    else { std::env::var("UU_AUDIO_SOURCE").unwrap_or_else(|_| "@DEFAULT_MONITOR@".into()) }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn recovery_only_claims_exact_null_sink() {
        let modules="1\tmodule-null-sink\tsink_name=uu-remote-sink\t0\n2\tmodule-null-sink\tsink_name=uu-remote-sink-other\t0\n3\tmodule-other\tsink_name=uu-remote-sink\t0\n";
        assert_eq!(owned_modules(modules),vec![1]);
    }
}

/// Also restore on ordinary returns from the host; SIGKILL is recovered next startup.
pub struct Cleanup;
impl Drop for Cleanup { fn drop(&mut self) { disable(); } }
