#!/usr/bin/env python3
"""Generate or validate a 640x640 SDR color/range fixture."""
from pathlib import Path
import json
import subprocess
import sys
colors = [(255,0,0),(0,255,0),(0,0,255),(128,128,128),(0,0,0),(255,255,255),(255,255,0),(0,255,255)]
if sys.argv[1] == 'generate':
    w,h = (map(int,sys.argv[3:5]) if len(sys.argv)>4 else (640,640))
    assert w%4==0 and h%2==0
    with Path(sys.argv[2]).open('wb') as f:
        f.write(f'P6\n{w} {h}\n255\n'.encode())
        for row in range(2):
            scanline=b''.join(bytes(colors[row*4+i])*(w//4) for i in range(4))
            for _ in range(h//2): f.write(scanline)
else:
    path = sys.argv[2]
    stream=json.loads(subprocess.check_output(['ffprobe','-v','error','-show_entries','stream=width,height,color_range,color_space,color_transfer,color_primaries','-of','json',path]))['streams'][0]
    w,h=stream.pop('width'),stream.pop('height')
    result = subprocess.run(['ffmpeg','-v','error','-i',path,'-frames:v','1','-pix_fmt','rgb24','-f','rawvideo','-'],capture_output=True,check=True)
    assert not result.stderr, result.stderr.decode(errors='replace')
    assert len(result.stdout)==w*h*3
    decoded=[]
    for i in range(8):
        x,y=(i%4)*(w//4)+w//8,(i//4)*(h//2)+h//4
        k=(y*w+x)*3
        decoded.append(list(result.stdout[k:k+3]))
    errors=[abs(a-b) for ref,rgb in zip(colors,decoded) for a,b in zip(ref,rgb)]
    metadata=stream
    data={'decoded_rgb':decoded,'mean_abs_channel_error':sum(errors)/len(errors),'max_channel_error':max(errors),'metadata':metadata}
    print(json.dumps(data,indent=2))
    if sys.argv[1]=='check':
        assert max(errors)<=6, data
        assert metadata=={'color_range':'tv','color_space':'bt709','color_transfer':'bt709','color_primaries':'bt709'},metadata
