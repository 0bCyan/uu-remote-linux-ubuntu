"""Audit receiver progress for a bounded VM service journal interval, not contents."""
import json,re,subprocess,sys,time,pathlib

class Audit:
 def __init__(self,start,end):
  self.start=start;self.end=end;self.reports=0;self.sequences={};self.last_progress=start;self.max_gap=0;self.peers=set();self.local_kinds=set();self.turn_errors=0;self.binding_errors=0;self.room_deferrals=0;self.interruptions=0
 def accept(self,stamp,message):
  if not self.start<=stamp<=self.end:return
  if 'Video receiver report:' in message:
   found=re.search(r'ssrc=(\d+) last_sequence=(\d+)',message)
   if found:
    ssrc,sequence=map(int,found.groups());self.reports+=1
    if self.sequences.get(ssrc)!=sequence:
     self.max_gap=max(self.max_gap,stamp-self.last_progress);self.last_progress=stamp
    self.sequences[ssrc]=sequence
  if 'ICE path:' in message:
   found=re.search(r'session=(\d+).*local=\w+/(\w+)',message)
   if found:self.peers.add(found[1]);self.local_kinds.add(found[2])
  if 'fail to refresh permissions:' in message:self.turn_errors+=1
  if 'bind() for refresh failed:' in message:self.binding_errors+=1
  if 'Room renewal deferred while remote connection is active' in message:self.room_deferrals+=1
  if any(text in message for text in ['Remote controller disconnected:','Live desktop stream stopped:','PeerConnection State changed: Failed','PeerConnection State changed: Disconnected','peer connection state changed: closed']):self.interruptions+=1
 def result(self):
  gap=max(self.max_gap,self.end-self.last_progress)
  passed=self.end-self.start>=3600 and self.reports>0 and len(self.sequences)==1 and len(self.peers)==1 and self.local_kinds=={'relay'} and gap<=90 and self.interruptions==0 and self.room_deferrals>0
  return dict(passed=passed,duration_s=round(self.end-self.start,3),video_reports=self.reports,video_ssrc_count=len(self.sequences),peer_count=len(self.peers),local_candidate_kinds=sorted(self.local_kinds),maximum_video_progress_gap_s=round(gap,3),interruptions=self.interruptions,turn_permission_refresh_errors=self.turn_errors,turn_channel_bind_refresh_errors=self.binding_errors,room_renewal_deferrals=self.room_deferrals)

def run(start,end):
 assert pathlib.Path('/proc/sys/kernel/hostname').read_text().strip()=='uu-hyprland-test'
 audit=Audit(start,end)
 proc=subprocess.Popen(['journalctl','--user','-u','uuremote-test.service','--since',f'@{start}','--until',f'@{end}','--no-pager','-o','json'],stdout=subprocess.PIPE,text=True)
 for line in proc.stdout:
  row=json.loads(line);audit.accept(int(row['__REALTIME_TIMESTAMP'])/1e6,row.get('MESSAGE',''))
 if proc.wait()!=0:raise RuntimeError('Journal read failed')
 return audit.result()

if __name__=='__main__':print(json.dumps(run(float(sys.argv[1]),float(sys.argv[2]) if len(sys.argv)>2 else time.time())))
