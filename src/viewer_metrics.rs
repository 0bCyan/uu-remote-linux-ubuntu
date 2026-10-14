//! Bounded viewer telemetry extraction. Payloads and nonnumeric values never log.
use base64::Engine;
use std::io::Read;
use serde_json::Value;
const LIMIT:usize=256*1024;
fn permitted(key:&str)->bool{
    ["avg_received_fps","avg_decode_fps","avg_render_fps","avg_assembly_ms_interval","avg_current_delay_ms_interval","avg_decode_ms_interval","avg_delay_ms_interval","avg_jitter_buffer_ms_interval","avg_processing_delay_interval","avg_render_delay_ms_interval","avg_target_delay_ms_interval","received_framerate","actual_received_framerate","decode_framerate","render_framerate","actual_framerate",
    "current_delay","average_delay","max_delay","current_delay_ms_interval","target_delay_ms_interval",
    "jitter_buffer_ms_interval","render_delay_ms_interval","assembly_ms_interval","decode_ms_interval",
    "processing_delay_interval","sending_delay_interval","frames_received","frames_decoded","frames_actual_rendered",
    "packets_received","packets_lost","packet_loss_rate","current_packet_loss_rate",
    "frame_capture_delay_ms_interval","frame_encode_delay_ms_interval","frame_pacer_delay_ms_interval",
    "frame_transport_delay_ms_interval","frame_decode_delay_ms_interval","frame_e2e_delay_ms_interval"].contains(&key)
}
fn unpack(text:&str)->Option<Value>{
    if text.len()>LIMIT{return None;}
    if let Ok(value)=serde_json::from_str(text){return Some(value);}
    let bytes=base64::engine::general_purpose::STANDARD.decode(text).ok()?;
    let mut output=Vec::new();
    if bytes.starts_with(&[31,139]){
        flate2::read::GzDecoder::new(&bytes[..]).take(LIMIT as u64+1).read_to_end(&mut output).ok()?;
    }else if bytes.first()==Some(&0x78){
        flate2::read::ZlibDecoder::new(&bytes[..]).take(LIMIT as u64+1).read_to_end(&mut output).ok()?;
    }else{output=bytes;}
    if output.len()>LIMIT{return None;}serde_json::from_slice(&output).ok()
}
pub fn collect(event:&Value)->Vec<(String,f64)>{
    fn walk(v:&Value,path:&str,selected:bool,depth:usize,budget:&mut usize,out:&mut Vec<(String,f64)>){
        if depth>12||*budget==0||out.len()>=128{return;}*budget-=1;
        match v{
            Value::Number(n) if selected=>{if let Some(n)=n.as_f64(){out.push((path.into(),n));}},
            Value::Object(map)=>for(k,v)in map{
                let allowed=permitted(k);
                // After selecting a metric, only numeric aggregation keys are retained.
                let aggregation=selected&&["min","max","avg","mean","sum","count","value","current","total"].contains(&k.as_str());
                let next=if allowed||aggregation{format!("{path}.{k}")}else{path.into()};
                walk(v,&next,allowed||aggregation,depth+1,budget,out);
            },
            Value::Array(a)=>for v in a{walk(v,path,selected,depth+1,budget,out);},
            Value::String(text)=>{
                if selected{if let Ok(number)=text.parse::<f64>(){if number.is_finite(){out.push((path.into(),number));return;}}}
                if let Some(unpacked)=unpack(text){walk(&unpacked,path,selected,depth+1,budget,out);}
            },_=>{}
        }
    }
    let mut output=Vec::new();walk(event,"stats",false,0,&mut 8192,&mut output);output
}
pub fn safe_shape(event:&Value)->Vec<(String,&'static str)>{
    fn walk(v:&Value,path:&str,depth:usize,out:&mut Vec<(String,&'static str)>){
        if depth>3||out.len()>=24{return;}
        let kind=match v{Value::Object(_)=>"object",Value::Array(_)=>"array",Value::String(_)=>"string",Value::Number(_)=>"number",Value::Bool(_)=>"bool",Value::Null=>"null"};
        out.push((path.into(),kind));
        if let Value::Object(map)=v{for(k,v)in map{
            if k.len()<=48&&k.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'){walk(v,&format!("{path}.{k}"),depth+1,out);}
        }}
    }
    let mut output=Vec::new();walk(event,"event",0,&mut output);output
}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn extracts_compressed_metrics_but_no_private_values(){
        use std::io::Write;
        let text=r#"{"token":"secret","current_delay_ms_interval":{"avg":17},"render_framerate":60}"#;
        let mut z=flate2::write::GzEncoder::new(Vec::new(),flate2::Compression::default());z.write_all(text.as_bytes()).unwrap();
        let encoded=base64::engine::general_purpose::STANDARD.encode(z.finish().unwrap());
        assert_eq!(collect(&serde_json::json!({"data":encoded})),vec![("stats.current_delay_ms_interval.avg".into(),17.),("stats.render_framerate".into(),60.)]);
    }
    #[test]fn limits_depth_and_preserves_numeric_strings(){
        assert_eq!(collect(&serde_json::json!({"render_framerate":"59.9","cookie":12})),vec![("stats.render_framerate".into(),59.9)]);
        assert!(unpack(&"A".repeat(LIMIT+1)).is_none());
    }
}
