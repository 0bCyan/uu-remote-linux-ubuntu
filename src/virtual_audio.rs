//! Scoped PulseAudio / PipeWire-pulse routing. Never inspect or log audio payloads.
//! `pactl` is used when present; plain PipeWire installs without it (default Ubuntu desktop)
//! use `pw-cli`/`pw-dump`/`wpctl`, where WirePlumber moves default-following streams itself.
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::os::fd::AsRawFd;
const SINK: &str = "uu-remote-sink";
const MONITOR: &str = "uu-remote-sink.monitor";
#[derive(serde::Serialize, serde::Deserialize)]
struct Route { previous: String, module: Option<u32>, #[serde(default)] pipewire: bool }
struct State { route: Option<Route>, _lock: std::fs::File }
static STATE: OnceLock<Mutex<Option<State>>> = OnceLock::new();
fn path() -> std::path::PathBuf { crate::config::path().with_file_name("virtual_audio.json") }
fn pactl(args: &[&str]) -> Result<String, ()> { run("pactl", args) }
fn run(program: &str, args: &[&str]) -> Result<String, ()> {
    let out = Command::new(program).args(args).env("LC_ALL","C").stdin(Stdio::null()).stderr(Stdio::null()).output().map_err(|_| ())?;
    if !out.status.success() { return Err(()); }
    String::from_utf8(out.stdout).map(|s|s.trim().to_owned()).map_err(|_| ())
}
fn pipewire_only() -> bool { pactl(&["info"]).is_err() && run("wpctl",&["status"]).is_ok() }
/// Node ids whose node.name is `name`; `null_sink` restricts to null-audio-sink nodes (ownership).
fn pw_nodes(dump: &str, name: &str, null_sink: bool) -> Vec<u32> {
    let Ok(serde_json::Value::Array(objects)) = serde_json::from_str(dump) else { return Vec::new() };
    objects.iter().filter_map(|o| {
        let props = o.get("info")?.get("props")?;
        if props.get("node.name")?.as_str()? != name { return None; }
        if null_sink && props.get("factory.name").and_then(|v| v.as_str()) != Some("support.null-audio-sink") { return None; }
        u32::try_from(o.get("id")?.as_u64()?).ok()
    }).collect()
}
fn pw_node(name: &str, null_sink: bool) -> Option<u32> { pw_nodes(&run("pw-dump",&["Node"]).ok()?, name, null_sink).first().copied() }
fn default_sink(pipewire: bool) -> Result<String, ()> {
    if !pipewire { return pactl(&["get-default-sink"]); }
    run("wpctl",&["inspect","@DEFAULT_AUDIO_SINK@"])?.lines()
        .find_map(|l| l.trim().trim_start_matches("* ").strip_prefix("node.name = ").map(|v| v.trim_matches('"').to_owned())).ok_or(())
}
fn set_default(pipewire: bool, name: &str) -> Result<(), ()> {
    if !pipewire { return pactl(&["set-default-sink",name]).map(|_| ()); }
    run("wpctl",&["set-default",&pw_node(name,false).ok_or(())?.to_string()]).map(|_| ())
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
    // The previous output may be gone (unplugged USB card): still unload our sink so the server
    // picks another default instead of leaving every stream on a null sink.
    let pw=route.pipewire;
    if default_sink(pw).is_ok_and(|s| s == SINK) && set_default(pw,&route.previous).is_err() {
        log::warn!("Previous default audio output unavailable; the sound server will choose a new default");
    }
    if pw {
        for id in pw_nodes(&run("pw-dump",&["Node"])?,SINK,true) { run("pw-cli",&["destroy",&id.to_string()])?; }
    } else {
        move_inputs(SINK,&route.previous);
        let modules=owned_modules(&pactl(&["list","short","modules"])?);
        for id in modules { pactl(&["unload-module",&id.to_string()])?; }
    }
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
        let pipewire=pipewire_only();
        let previous=default_sink(pipewire)?;
        if previous==SINK { return Err(()); }
        // Refuse to claim another instance's or manually-created sink.
        if if pipewire { pw_node(SINK,false).is_some() } else { sink_id(SINK).is_some() } { return Err(()); }
        let mut route=Route { previous, module:None, pipewire };
        persist(&route)?; // Save before loading: also covers a crash between load and ID persistence.
        state.route=Some(Route { previous:route.previous.clone(), module:None, pipewire });
        let id=if pipewire {
            // object.linger keeps the node after pw-cli exits; recover() destroys it.
            run("pw-cli",&["create-node","adapter","{ factory.name=support.null-audio-sink node.name=uu-remote-sink \
                node.description=UU-Remote media.class=Audio/Sink object.linger=true audio.position=[FL FR] }"])?;
            let mut node=None;
            for _ in 0..20 { node=pw_node(SINK,true); if node.is_some() { break; } std::thread::sleep(std::time::Duration::from_millis(50)); }
            node.ok_or(())?
        } else {
            pactl(&["load-module","module-null-sink","sink_name=uu-remote-sink","sink_properties=device.description=UU-Remote"])?.parse().map_err(|_| ())?
        };
        route.module=Some(id);
        persist(&route)?;
        state.route=Some(route);
        set_default(pipewire,SINK)?;
        if !pipewire { move_inputs(&state.route.as_ref().ok_or(())?.previous,SINK); }
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
    #[test] fn pipewire_recovery_only_claims_owned_null_sink() {
        let dump=r#"[{"id":5,"info":{"props":{"node.name":"uu-remote-sink","factory.name":"support.null-audio-sink"}}},
            {"id":6,"info":{"props":{"node.name":"uu-remote-sink","factory.name":"api.alsa.pcm.sink"}}},
            {"id":7,"info":{"props":{"node.name":"other","factory.name":"support.null-audio-sink"}}},{"id":8}]"#;
        assert_eq!(pw_nodes(dump,"uu-remote-sink",true),vec![5]);
        assert_eq!(pw_nodes(dump,"uu-remote-sink",false),vec![5,6]);
        let old:Route=serde_json::from_str(r#"{"previous":"a","module":3}"#).unwrap();
        assert!(!old.pipewire);
    }
}

/// Also restore on ordinary returns from the host; SIGKILL is recovered next startup.
pub struct Cleanup;
impl Drop for Cleanup { fn drop(&mut self) { disable(); } }
