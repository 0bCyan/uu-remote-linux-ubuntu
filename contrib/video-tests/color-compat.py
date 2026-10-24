#!/usr/bin/env python3
"""Generated SDR patches: standard decode and fixed full-range 601 renderer model.

The model uses the documented coefficients of WebRTC RTCMTLNV12Renderer.
It does not establish which renderer a proprietary iOS viewer actually uses.
"""
# Ubuntu fork regression fixture by 0bCyan, 2026-10-24; see NOTICE.md.
from pathlib import Path
import json
import subprocess
import sys

COLORS = [(v, v, v) for v in [0,8,16,24,32,48,64,80,96,112,128,144,160,176,192,208,224,232,240,248,255]]
COLORS += [(r,g,b) for r in [48,128,208] for g in [48,128,208] for b in [48,128,208]]
COLORS += [(255,0,0),(0,255,0),(0,0,255),(255,255,0),(0,255,255),(255,0,255),(210,160,130),(40,85,150)]
COLORS += [(64,100,180),(180,60,100),(90,170,60),(220,160,60),(20,80,100),(160,130,180),(230,210,195),(100,100,120)]


def check(path, mode):
    assert mode in ('bt709', 'bt601-full')
    full = mode == 'bt601-full'
    meta = json.loads(subprocess.check_output(['ffprobe','-v','error','-select_streams','v:0',
        '-show_entries','stream=width,height,color_space,color_range,color_transfer,color_primaries',
        '-of','json',path]))['streams'][0]
    w,h = meta.pop('width'), meta.pop('height')
    assert w % 8 == 0 and h % 8 == 0
    assert meta == {'color_range':'pc' if full else 'tv','color_space':'smpte170m' if full else 'bt709',
                    'color_transfer':'bt709','color_primaries':'bt709'}, meta
    def decode(fmt):
        result = subprocess.run(['ffmpeg','-v','error','-i',path,'-frames:v','1',
            '-pix_fmt',fmt,'-f','rawvideo','-'], capture_output=True, check=True)
        assert not result.stderr, result.stderr.decode(errors='replace')
        return result.stdout
    rgb = decode('rgb24')
    # Preserve full-range planes; requesting yuv420p can implicitly scale them to TV range.
    raw = decode('yuvj420p' if full else 'yuv420p')
    assert len(rgb) == w*h*3 and len(raw) == w*h*3//2
    errors, shader_errors = [], []
    for i, ref in enumerate(COLORS):
        x,y = (i%8)*w//8+w//16, (i//8)*h//8+h//16
        k = (y*w+x)*3
        errors.extend(abs(a-b) for a,b in zip(ref,rgb[k:k+3]))
        yy = raw[y*w+x]
        u = raw[w*h+(y//2)*(w//2)+x//2]-127.5
        v = raw[w*h*5//4+(y//2)*(w//2)+x//2]-127.5
        rendered = [yy+1.403*v, yy-.344*u-.714*v, yy+1.770*u]
        shader_errors.extend(abs(a-max(0,min(255,b))) for a,b in zip(ref,rendered))
    result = {'mode':mode,'metadata':meta,'decoder_mean_error':round(sum(errors)/len(errors),2),
        'decoder_max_error':max(errors),'fixed_shader_mean_error':round(sum(shader_errors)/len(shader_errors),2),
        'fixed_shader_max_error':round(max(shader_errors),2)}
    print(json.dumps(result, indent=2))
    assert max(errors) <= 6, result
    if full:
        assert max(shader_errors) <= 6, result


if __name__ == '__main__':
    if sys.argv[1] == 'generate':
        w,h = map(int,sys.argv[3:5]) if len(sys.argv)>4 else (640,640)
        assert w % 8 == 0 and h % 8 == 0
        with Path(sys.argv[2]).open('wb') as f:
            f.write(f'P6\n{w} {h}\n255\n'.encode())
            for row in range(8):
                line = b''.join(bytes(COLORS[row*8+i])*(w//8) for i in range(8))
                for _ in range(h//8):
                    f.write(line)
    elif sys.argv[1] == 'check':
        check(sys.argv[2], sys.argv[3] if len(sys.argv)>3 else 'bt709')
    else:
        raise SystemExit('usage: color-compat.py generate output.ppm [width height] | check video [bt709|bt601-full]')
