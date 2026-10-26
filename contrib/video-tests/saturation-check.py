#!/usr/bin/env python3
"""Compare generated color-compat.py patches before/after VAAPI saturation.

Fails if a driver silently accepts but ignores the filter, changes luma/gray,
or changes SDR metadata. Never use desktop recordings for this fixture.
"""
# Ubuntu fork regression fixture by 0bCyan, 2026-10-26; see NOTICE.md.
import json
import statistics
import subprocess
import sys


def samples(path):
    meta = json.loads(subprocess.check_output([
        'ffprobe', '-v', 'error', '-select_streams', 'v:0', '-show_entries',
        'stream=width,height,color_space,color_range,color_transfer,color_primaries',
        '-of', 'json', path]))['streams'][0]
    w, h = meta['width'], meta['height']
    assert w % 8 == 0 and h % 8 == 0
    fmt = 'yuvj420p' if meta['color_range'] == 'pc' else 'yuv420p'
    raw = subprocess.check_output(['ffmpeg', '-v', 'error', '-i', path,
        '-frames:v', '1', '-pix_fmt', fmt, '-f', 'rawvideo', '-'])
    assert len(raw) == w * h * 3 // 2
    values = []
    for i in range(64):
        x, y = i % 8 * w // 8 + w // 16, i // 8 * h // 8 + h // 16
        c = y // 2 * (w // 2) + x // 2
        values.append((raw[y*w+x], raw[w*h+c], raw[w*h*5//4+c]))
    return meta, values


def check(original, adjusted, gain):
    assert 0.5 <= gain <= 1.5 and gain != 1.0
    meta, before = samples(original)
    new_meta, after = samples(adjusted)
    assert new_meta == meta, (meta, new_meta)
    luma = max(abs(a[0]-b[0]) for a, b in zip(before, after))
    gray = max(abs(a[k]-b[k]) for a, b in zip(before[:21], after[:21]) for k in (1, 2))
    errors, ratios = [], []
    for a, b in zip(before[21:], after[21:]):
        for k in (1, 2):
            target = max(0, min(255, 128 + (a[k]-128)*gain))
            errors.append(abs(b[k]-target))
            if 10 < abs(a[k]-128) < 100:
                ratios.append((b[k]-128)/(a[k]-128))
    measured = statistics.median(ratios)
    result = {'requested_gain': gain, 'measured_chroma_gain': round(measured, 4),
        'max_luma_change': luma, 'max_gray_chroma_change': gray,
        'max_chroma_error': round(max(errors), 2), 'metadata_unchanged': True}
    assert luma <= 2 and gray <= 1 and max(errors) <= 3, result
    assert abs(measured-gain) <= 0.015, result
    assert abs(measured-1) >= abs(gain-1)*0.6, result
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    if len(sys.argv) != 4:
        raise SystemExit('usage: saturation-check.py original.h264 adjusted.h264 gain')
    check(sys.argv[1], sys.argv[2], float(sys.argv[3]))
