//! Synthetic clipboard acceptance; never prints clipboard data.
use std::{io::{BufRead,BufReader,Write},process::{Command,Stdio},sync::Arc,time::{Duration,Instant}};
pub fn run() -> Result<(),String> {
    if std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default().trim()!="uu-hyprland-test" {
        return Err("Clipboard acceptance is restricted to the isolated VM".into());
    }
    let session=Arc::new(crate::portal::PortalSession::open()?);
    let bridge=crate::clipboard::start_portal(session.clone());
    if !bridge.ready.blocking_recv().unwrap_or(false) { return Err("Portal clipboard permission not granted".into()); }
    let mut child=Command::new("python3").arg("/home/uuremote/clipboard-acceptance.py")
        .env("GDK_BACKEND","wayland").env("GSK_RENDERER","cairo")
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|_|"Start GTK clipboard fixture")?;
    let result=(|| {
        let mut writer=child.stdin.take().ok_or("Clipboard fixture stdin missing")?;
        let mut reader=BufReader::new(child.stdout.take().ok_or("Clipboard fixture stdout missing")?);
        let mut line=String::new(); reader.read_line(&mut line).map_err(|_|"Clipboard fixture not ready")?;
        let ready:serde_json::Value=serde_json::from_str(&line).map_err(|_|"Invalid clipboard fixture readiness")?;
        if ready["ready"]!=true || ready["native_wayland"]!=true { return Err("Clipboard fixture is not native Wayland".into()); }
        std::thread::sleep(Duration::from_secs(2));
        let mut outgoing=bridge.outgoing;
        let mut check=|op:&str,text:&str|->Result<bool,String> {
            writeln!(writer,"{}",serde_json::json!({"op":op,"text":text})).map_err(|_|"Clipboard fixture write failed")?;
            writer.flush().map_err(|_|"Clipboard fixture write failed")?;
            let mut line=String::new();reader.read_line(&mut line).map_err(|_|"Clipboard fixture read failed")?;
            let result:serde_json::Value=serde_json::from_str(&line).map_err(|_|"Invalid clipboard fixture result")?;
            Ok(result["ok"]==true)
        };
        for (name,text) in [("unicode", "UU incoming 中文🙂\nsecond line".to_owned()),("empty",String::new()),("maximum_utf8","中".repeat(crate::clipboard::MAX_TEXT/3))] {
            bridge.incoming.send_replace(Some(text.clone()));
            let deadline=Instant::now()+Duration::from_secs(6);let mut matched=false;
            while Instant::now()<deadline { if check("read",&text)? {matched=true;break;} std::thread::sleep(Duration::from_millis(100)); }
            if !matched { return Err(format!("Portal clipboard incoming {name} mismatch")); }
            std::thread::sleep(Duration::from_millis(300));
            if outgoing.has_changed().unwrap_or(false) { return Err("Portal clipboard echoed its own selection".into()); }
            println!("Portal clipboard incoming {name}: passed; no echo");
        }
        session.move_pointer(f64::from(session.width)/2.0,f64::from(session.height)/2.0,false)?;
        session.button(1,true)?; session.button(1,false)?;
        std::thread::sleep(Duration::from_millis(300));
        let external="UU outgoing 中文🚀\nsecond line";
        if !check("set",external)? { return Err("GTK clipboard ownership failed".into()); }
        let deadline=Instant::now()+Duration::from_secs(6);let mut matched=false;
        while Instant::now()<deadline {
            if outgoing.has_changed().unwrap_or(false) && outgoing.borrow_and_update().as_deref()==Some(external) {matched=true;break;}
            std::thread::sleep(Duration::from_millis(100));
        }
        if !matched { return Err("Portal clipboard outgoing Unicode mismatch".into()); }
        println!("Portal clipboard outgoing unicode: passed");
        bridge.incoming.send_replace(Some("x".repeat(crate::clipboard::MAX_TEXT+1)));
        std::thread::sleep(Duration::from_millis(400));
        if !check("read",external)? { return Err("Oversized text altered native clipboard".into()); }
        println!("Portal clipboard incoming oversized rejection: passed");
        if !check("set",&"y".repeat(crate::clipboard::MAX_TEXT+1))? { return Err("GTK oversized fixture setup failed".into()); }
        std::thread::sleep(Duration::from_secs(3));
        if outgoing.has_changed().unwrap_or(false) { return Err("Portal clipboard published oversized native text".into()); }
        println!("Portal clipboard outgoing oversized rejection: passed");
        if !check("set","")? { return Err("GTK empty fixture setup failed".into()); }
        let deadline=Instant::now()+Duration::from_secs(6);let mut matched=false;
        while Instant::now()<deadline {
            if outgoing.has_changed().unwrap_or(false) && outgoing.borrow_and_update().as_deref()==Some("") {matched=true;break;}
            std::thread::sleep(Duration::from_millis(100));
        }
        if !matched { return Err("Portal clipboard outgoing empty text mismatch".into()); }
        println!("Portal clipboard outgoing empty: passed");
        drop(writer); Ok(())
    })();
    let _=child.kill();let _=child.wait();result
}
