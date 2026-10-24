# X11 / VAAPI regression probes

Use generated input only. These probes do not read account settings. `damage-region`
changes the root of its test display: **run it only on an isolated Xvfb**. `vaapi-probe`
uses an offscreen pixmap on the selected X11 display and never paints the desktop.

Dependencies: X11, X11-xcb, XCB DRI3/Damage/XFixes, GBM, VA headers; FFmpeg for decoding.
Run from the repository root:

```sh
cc -O2 -I c_engine contrib/video-tests/damage-region.c \
  -lX11 -lX11-xcb -lxcb -lxcb-dri3 -lxcb-damage -lxcb-xfixes -lgbm -o /tmp/uu-damage-test
# In a shell with an isolated Xvfb at :98, 640x480x24:
DISPLAY=:98 /tmp/uu-damage-test

cc -O2 -I c_engine contrib/video-tests/vaapi-probe.c c_engine/vaapi_engine.c \
  -ldl -lgbm -lX11 -lX11-xcb -lxcb -lxcb-dri3 -o /tmp/uu-vaapi-test
ffmpeg -i generated-desktop.png -pix_fmt bgra -f rawvideo generated.bgra
# width height frames BGRA-input bitstream-output codec (0 H.264, 1 HEVC)
UU_PROBE_DYNAMIC=1 /tmp/uu-vaapi-test 3120 2080 180 generated.bgra upload.h264 0
UU_PROBE_DYNAMIC=1 UU_PROBE_DMA=1 /tmp/uu-vaapi-test 3120 2080 180 generated.bgra dma.h264 0
python3 contrib/video-tests/check-dynamic.py upload.h264
python3 contrib/video-tests/check-dynamic.py dma.h264
```

The VAAPI probe uses the default render node; DMA mode additionally needs authenticated
X11 with DRI3. Dynamic mode needs input at least 528x528. It changes a 128x128 rectangle at
frames 60 and 120, keeps the converted surface between updates, and requests IDRs at 37
and 125. The checker decodes all 180 frames, checks update colors and IDR positions, and
rejects decoder errors. Inspect decoded images and compare against the generated source
as well; color checks alone cannot establish full-frame correctness.

The DMA probe deliberately uses X11 writes, matching capture. CPU mmap writes to a GBM
BO produced intermittent VPP artifacts on Meteor Lake despite DMA_BUF_IOCTL_SYNC and
correct RGB readback; that is not representative of the GPU capture path.

## Edge gestures and color metadata

`pointer-barrier.c` must run only on isolated Xvfb. It checks that absolute remote
targets produce XI2 barrier pressure, while ordinary targets remain exact:

```sh
cc -O2 -I c_engine contrib/video-tests/pointer-barrier.c \
  -lX11 -lXext -lXtst -lXfixes -lXi -lpthread -o /tmp/uu-pointer-test
DISPLAY=:98 /tmp/uu-pointer-test
python3 contrib/video-tests/color-bars.py generate colors.ppm 3120 2080
ffmpeg -i colors.ppm -pix_fmt bgra -f rawvideo colors.bgra
/tmp/uu-vaapi-test 3120 2080 30 colors.bgra colors.h264 0
python3 contrib/video-tests/color-bars.py check colors.h264
```

Color checks use the centers of eight flat SDR patches and require limited-range
BT.709 metadata with at most six levels of error. Test both 640x640 and HD/native
sizes: an unlabelled stream may decode correctly at one size and incorrectly at
another. These tests do not establish calibrated monitor/ICC color accuracy.

`cargo test --locked --features inproc-av pacing_reduces_burst_loss -- --nocapture`
checks actual SRTP packets through a bounded virtual link. It compares an unpaced
picture with a paced one; it is a simulated burst-loss regression, not a measurement
of the user's phone/network.

## Live VAAPI rates and shared repair budget

Set `UU_PROBE_RATE_STEPS=1` on the 180-frame probe to switch the maximum bitrate
to 2 Mbps at frame 50 and 6 Mbps at frame 110, with 250 ms VBV. Combine it with
`UU_PROBE_DYNAMIC=1` and run `check-dynamic.py` to verify that updates, prediction
and forced IDRs remain intact. Test both upload and offscreen DMA paths.
`color-bars.py check` verifies the first frame's BT.709 range/matrix and patch colors.

`UU_PROBE_MOTION=384` adds a generated 384x384 changing noise patch; use
`=1` for the extreme 768x768 case (input must be at least patch size + 400 in each dimension) to load rate control. Use it separately from dynamic-mode color assertions;
compare per-frame byte totals after each rate change and decode the complete stream.
The noise fixture tests rate response, not subjective desktop quality. Extremely
complex content may exceed the encoder budget even at its maximum QP; the transport
limiter still applies, reducing delivered frame rate.

`cargo test --locked --features inproc-av video_network -- --nocapture` checks
feedback backoff/recovery, idle and sparse-report behavior, stream isolation and
actual NACK responder writes through the shared budget. The bounded virtual-link
regression sends a 300 KB picture over 8 Mbps / 12 KB and 3 Mbps / 6 KB virtual links. Neither
test measures the user's phone-network loss or guarantees zero loss in production.
