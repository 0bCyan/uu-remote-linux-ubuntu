//! Bounded RFC 6184 / RFC 7798 non-interleaved receivers (no DONL). No media contents in logs.
use std::{collections::VecDeque, sync::{Arc, Mutex}};
use std::time::{Duration,Instant};
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub enum Codec { #[default] H264, H265 }
impl Codec {
    pub fn from_mime(mime:&str)->Option<Self> {
        if mime.eq_ignore_ascii_case("video/H264"){Some(Self::H264)}
        else if mime.eq_ignore_ascii_case("video/H265"){Some(Self::H265)}else{None}
    }
    pub fn demuxer(self)->&'static str {match self {Self::H264=>"h264",Self::H265=>"hevc"}}
}
pub struct AccessUnit {pub codec:Codec,pub data:Vec<u8>}
const MAX_AU: usize = 8 * 1024 * 1024;
#[derive(Clone)]
pub struct Packet { pub seq: u16, pub timestamp: u32, pub marker: bool, pub payload: Vec<u8> }
#[derive(Default)]
pub struct Output { pub units: Vec<Vec<u8>>, pub pli: bool }
pub struct Receiver {
    codec: Codec,
    next: Option<u16>, pending: Vec<Packet>, timestamp: Option<u32>,
    au: Vec<u8>, fragment: Option<u16>, damaged: bool, gap_since:Option<Instant>, last_packet:Instant,
}
impl Default for Receiver {
    fn default() -> Self { Self { codec: Codec::H264, next: None, pending: Vec::new(), timestamp: None, au: Vec::new(), fragment: None, damaged: false, gap_since:None,last_packet:Instant::now() } }
}
impl Receiver {
    pub fn new(codec:Codec)->Self {Self{codec,..Self::default()}}
    pub fn codec(&self)->Codec {self.codec}
    pub fn push(&mut self, p: Packet) -> Output {
        if p.payload.len()>65536 {return Output{units:Vec::new(),pli:true}}
        let next = *self.next.get_or_insert(p.seq);
        if p.seq.wrapping_sub(next) >= 32768 || self.pending.iter().any(|q| q.seq == p.seq) { return Output::default() }
        self.last_packet=Instant::now();
        self.pending.push(p);
        let mut out = Output::default();
        self.drain(&mut out);
        if self.pending.len() >= 32 { self.skip_gap(&mut out); }
        if self.pending.is_empty(){self.gap_since=None;}else{self.gap_since.get_or_insert(Instant::now());}
        out
    }
    // A timer bounds latency even if the missing packet is the last packet sent.
    pub fn expire(&mut self) -> Output {
        self.expire_at(Instant::now())
    }
    fn expire_at(&mut self,now:Instant)->Output {
        let mut out=Output::default();
        if self.gap_since.is_some_and(|since|now.duration_since(since)>=Duration::from_millis(40)) && !self.pending.is_empty() {
            self.skip_gap(&mut out);self.gap_since=if self.pending.is_empty(){None}else{Some(now)};
        }
        if !self.au.is_empty() && now.duration_since(self.last_packet)>=Duration::from_millis(200) {
            self.au.clear();self.fragment=None;self.damaged=true;out.pli=true;
        }
        out
    }
    fn skip_gap(&mut self, out: &mut Output) {
        let next = self.next.unwrap();
        self.next = self.pending.iter().min_by_key(|p| p.seq.wrapping_sub(next)).map(|p| p.seq);
        self.au.clear(); self.fragment = None; self.damaged = true; out.pli = true;
        self.drain(out);
    }
    fn drain(&mut self, out: &mut Output) {
        while let Some(i) = self.pending.iter().position(|p| Some(p.seq) == self.next) {
            let p = self.pending.remove(i); self.next = Some(p.seq.wrapping_add(1));
            if self.timestamp != Some(p.timestamp) {
                // A timestamp transition without marker is an incomplete AU.
                if !self.au.is_empty() || self.fragment.is_some() { out.pli = true; }
                self.au.clear(); self.fragment = None; self.damaged = false; self.timestamp = Some(p.timestamp);
            }
            if !self.damaged && self.nal(&p.payload).is_err() { self.damaged = true; self.au.clear(); self.fragment = None; out.pli = true; }
            if p.marker {
                if self.fragment.is_some() { out.pli = true; self.damaged = true; }
                if !self.damaged && !self.au.is_empty() { out.units.push(std::mem::take(&mut self.au)); }
                self.au.clear(); self.fragment = None;
            }
        }
    }
    fn append(&mut self, bytes: &[u8]) -> Result<(), ()> {
        if self.au.len() + bytes.len() > MAX_AU { return Err(()) }
        self.au.extend_from_slice(bytes); Ok(())
    }
    fn nal(&mut self, p: &[u8]) -> Result<(), ()> {
        if self.codec==Codec::H265 {return self.hevc(p)}
        let h = *p.first().ok_or(())?;
        if h & 0x80 != 0 { return Err(()) }
        match h & 31 {
            1..=23 if self.fragment.is_none() => { self.append(&[0,0,0,1])?; self.append(p) }
            24 if self.fragment.is_none() => {
                let mut rest = &p[1..]; if rest.is_empty() { return Err(()) }
                while !rest.is_empty() {
                    if rest.len() < 2 { return Err(()) }
                    let n = u16::from_be_bytes([rest[0], rest[1]]) as usize; rest = &rest[2..];
                    if n == 0 || rest.len() < n || rest[0] & 0x80 != 0 || !(1..=23).contains(&(rest[0] & 31)) { return Err(()) }
                    self.append(&[0,0,0,1])?; self.append(&rest[..n])?; rest = &rest[n..];
                } Ok(())
            }
            28 if p.len() > 2 => {
                let f = p[1]; let kind = f & 31; let start = f & 128 != 0; let end = f & 64 != 0;
                if f & 32 != 0 || kind == 0 || kind > 23 || (start && end) { return Err(()) }
                let header = (h & 0xe0) | kind;
                if start {
                    if self.fragment.is_some() { return Err(()) }
                    self.append(&[0,0,0,1,header])?; self.fragment = Some(u16::from(header));
                } else if self.fragment != Some(u16::from(header)) { return Err(()) }
                self.append(&p[2..])?;
                if end { self.fragment = None; } Ok(())
            }
            _ => Err(()),
        }
    }
    fn hevc_header(p:&[u8])->Result<u16,()> {
        if p.len()<2 || p[0]&0x80!=0 || p[1]&7==0 {return Err(())}
        Ok(u16::from_be_bytes([p[0],p[1]]))
    }
    fn hevc(&mut self,p:&[u8])->Result<(),()> {
        Self::hevc_header(p)?;
        match (p[0]>>1)&63 {
            0..=47 if self.fragment.is_none()=>{self.append(&[0,0,0,1])?;self.append(p)},
            48 if self.fragment.is_none()=>{
                let mut rest=&p[2..];if rest.is_empty(){return Err(())}
                while !rest.is_empty() {
                    if rest.len()<2 {return Err(())}
                    let n=u16::from_be_bytes([rest[0],rest[1]]) as usize;rest=&rest[2..];
                    if n<2 || rest.len()<n {return Err(())}
                    Self::hevc_header(&rest[..n])?;
                    if (rest[0]>>1)&63>=48 {return Err(())}
                    self.append(&[0,0,0,1])?;self.append(&rest[..n])?;rest=&rest[n..];
                }Ok(())
            },
            49 if p.len()>3=>{
                let f=p[2];let kind=f&63;let start=f&128!=0;let end=f&64!=0;
                if kind>=48 || (start&&end){return Err(())}
                // RFC 7798 4.4.3: preserve LayerId and TID, replacing only FU's NAL type.
                let header=[(p[0]&0x81)|(kind<<1),p[1]];let key=u16::from_be_bytes(header);
                if start {
                    if self.fragment.is_some(){return Err(())}
                    self.append(&[0,0,0,1])?;self.append(&header)?;self.fragment=Some(key);
                }else if self.fragment!=Some(key){return Err(())}
                self.append(&p[3..])?;if end{self.fragment=None;}Ok(())
            },
            _=>Err(()),
        }
    }
}
/// Drop the oldest item on overflow; never block the RTP reader on a decoder/UI.
pub struct Latest<T>(Arc<Mutex<VecDeque<T>>>, usize);
impl<T> Clone for Latest<T> { fn clone(&self) -> Self { Self(self.0.clone(), self.1) } }
impl<T> Latest<T> {
    pub fn new(capacity: usize) -> Self { assert!(capacity > 0); Self(Arc::new(Mutex::new(VecDeque::new())), capacity) }
    pub fn push(&self, value: T) -> bool { let mut q = self.0.lock().unwrap(); let dropped = q.len() == self.1; if dropped { q.pop_front(); } q.push_back(value); dropped }
    pub fn pop(&self) -> Option<T> { self.0.lock().unwrap().pop_front() }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn p(seq:u16, ts:u32, marker:bool, payload:&[u8])->Packet { Packet{seq,timestamp:ts,marker,payload:payload.into()} }
    #[test] fn single_stap_and_boundary() {
        let mut r=Receiver::default();
        assert!(r.push(p(1,1,false,&[0x67,1])).units.is_empty());
        assert_eq!(r.push(p(2,1,true,&[24,0,2,0x68,2,0,2,0x65,3])).units[0],vec![0,0,0,1,0x67,1,0,0,0,1,0x68,2,0,0,0,1,0x65,3]);
        r.push(p(3,2,false,&[0x61,1]));
        let o=r.push(p(4,3,true,&[0x65,4])); assert!(o.pli); assert_eq!(o.units.len(),1);
    }
    #[test] fn fu_reorder_wrap_duplicate_and_loss() {
        let mut r=Receiver::default(); r.push(p(65535,1,false,&[0x7c,0x85,1]));
        assert!(r.push(p(1,1,true,&[0x7c,0x45,3])).units.is_empty());
        let o=r.push(p(0,1,false,&[0x7c,5,2])); assert_eq!(o.units[0],vec![0,0,0,1,0x65,1,2,3]);
        assert!(r.push(p(0,1,false,&[0x61])).units.is_empty());
        r.push(p(2,2,false,&[0x7c,0x85,1]));r.push(p(4,2,true,&[0x7c,0x45,3]));
        let o=r.expire_at(Instant::now()+Duration::from_millis(50));assert!(o.pli);assert!(o.units.is_empty());
        assert_eq!(r.push(p(5,3,true,&[0x65])).units.len(),1);
    }
    #[test] fn missing_tail_and_reorder_window_are_bounded() {
        let mut r=Receiver::default();r.push(p(0,1,false,&[0x7c,0x85,1]));
        assert!(!r.expire().pli);assert!(r.expire_at(Instant::now()+Duration::from_millis(201)).pli);
        let mut pli=false;for seq in 2..34{pli|=r.push(p(seq,seq as u32,true,&[0x65])).pli;}
        assert!(pli);assert!(r.pending.len()<32);
    }
    #[test] fn malformed_and_bounded() {
        for bytes in [vec![],vec![24,0,4,0x65],vec![0x7c,0xc5,1],vec![0xff]] {
            assert!(Receiver::default().push(p(1,1,true,&bytes)).pli);
        }
        let q=Latest::new(2);assert!(!q.push(1));q.push(2);assert!(q.push(3));assert_eq!(q.pop(),Some(2));assert_eq!(q.pop(),Some(3));
    }
}

#[cfg(test)] mod hevc_tests {
    use super::*;
    fn packet(seq:u16,ts:u32,marker:bool,payload:Vec<u8>)->Packet {Packet{seq,timestamp:ts,marker,payload}}
    fn sample()->Vec<u8>{
        let mut au=vec![0,0,0,1,64,1,9,0,0,0,1,66,1,8,0,0,0,1,68,1,7,0,0,0,1,0x27,0x2b];
        au.extend((0..2501).map(|i|(i%251+1)as u8));au
    }
    #[test] fn host_payloader_round_trip_single_fu_and_reorder(){
        for mtu in [4,5,7,1190,9000] {
            let au=sample();let payloads=crate::hevc::payload(mtu,&au).unwrap();let n=payloads.len();
            let mut packets:Vec<_>=payloads.into_iter().enumerate().map(|(i,b)|packet(65534u16.wrapping_add(i as u16),10,i+1==n,b.to_vec())).collect();
            // Start at the first sequence, then reorder adjacent packets across wraparound.
            for i in (1..n.saturating_sub(1)).step_by(2){packets.swap(i,i+1);}
            let mut r=Receiver::new(Codec::H265);let mut units=Vec::new();
            for p in packets {let o=r.push(p);assert!(!o.pli);units.extend(o.units);}
            assert_eq!(units,vec![au]);
        }
    }
    #[test] fn aggregation_and_malformed_packets(){
        let ap=vec![96,1,0,3,64,1,9,0,3,38,1,7];
        let mut r=Receiver::new(Codec::H265);
        assert_eq!(r.push(packet(0,0,true,ap)).units,vec![vec![0,0,0,1,64,1,9,0,0,0,1,38,1,7]]);
        for payload in [vec![],vec![64],vec![128,1],vec![64,0],vec![96,1],vec![96,1,0,4,38,1,7],
            vec![96,1,0,2,96,1],vec![98,1,0xc1,3],vec![98,1,0x41,3],vec![98,1,0xb0,3],vec![100,1,7]] {
            let o=Receiver::new(Codec::H265).push(packet(1,1,true,payload));assert!(o.pli);assert!(o.units.is_empty());
        }
    }
    #[test] fn loss_duplicates_incomplete_tail_and_header_mismatch(){
        let payloads=crate::hevc::payload(1190,&sample()).unwrap();
        let mut r=Receiver::new(Codec::H265);
        // Parameter sets followed by a fragmented IDR; lose its middle fragment.
        for (i,p) in payloads.iter().enumerate().take(4){r.push(packet(i as u16,1,false,p.to_vec()));}
        let tail=packet(5,1,true,payloads[5].to_vec());
        assert!(r.push(tail.clone()).units.is_empty());assert!(r.push(tail).units.is_empty());
        let o=r.expire_at(Instant::now()+Duration::from_millis(50));assert!(o.pli);assert!(o.units.is_empty());
        assert_eq!(r.push(packet(6,2,true,vec![38,1,9])).units.len(),1);
        r.push(packet(7,3,false,vec![98,1,0x93,7]));
        assert!(r.expire_at(Instant::now()+Duration::from_millis(201)).pli);
        let mut r=Receiver::new(Codec::H265);r.push(packet(0,0,false,vec![98,1,0x93,7]));
        let o=r.push(packet(1,0,true,vec![98,2,0x53,8]));assert!(o.pli);assert!(o.units.is_empty());
    }
}
