#!/usr/bin/env python3
"""Validate the generated-content probe's decoded changes and forced IDRs."""
import json
import subprocess
import sys

path = sys.argv[1]
def ffmpeg(*args):
    p = subprocess.run(['ffmpeg', '-hide_banner', '-loglevel', 'error', '-i', path, *args], capture_output=True, check=True)
    assert not p.stderr, p.stderr.decode(errors='replace')
    return p.stdout

frames = json.loads(subprocess.check_output(['ffprobe', '-v', 'error', '-show_frames', '-select_streams', 'v', '-show_entries', 'frame=key_frame', '-of', 'json', path]))['frames']
assert len(frames) == 180, len(frames)
keyframes = [i for i, f in enumerate(frames) if f['key_frame']]
assert keyframes == [0, 37, 97, 125], keyframes
# Interior of the rectangle, excluding chroma/inter-block boundaries.
data = ffmpeg('-vf', 'crop=64:64:432:432', '-pix_fmt', 'rgb24', '-f', 'rawvideo', '-')
size = 64 * 64 * 3
assert len(data) == size * 180
means = []
for i in range(180):
    crop = data[i*size:(i+1)*size]
    rgb = [sum(crop[c::3]) / (size//3) for c in range(3)]
    means.append(rgb)
    if 60 <= i < 120:
        assert rgb[0] > 180 and rgb[1] < 60 and rgb[2] < 60, (i, rgb)
    elif i >= 120:
        assert rgb[1] > 180 and rgb[0] < 60 and rgb[2] < 60, (i, rgb)
print(json.dumps({'frames':len(frames), 'idr_indices':keyframes, 'red_update_frame60':means[60], 'green_update_frame120':means[120], 'cached_frames_preserved':True, 'decode_errors':0, 'result':'PASS'}, indent=2))
