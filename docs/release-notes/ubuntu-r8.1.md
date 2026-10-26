# Ubuntu R8.1 — optional saturation preference

Adds `UU_VAAPI_SATURATION=1.05` for a subtle color boost in native VAAPI H.264.
Default `1.0` preserves R8 output and the original single VPP conversion pass.
Intel iHD requires a second GPU NV12 pass; no CPU pixel processing is added.
Unsupported initialization or invalid values retain original color. Other encoders
and HEVC are unchanged. Existing bitrate, pacing and direct-routing settings are retained.

Four generated-patch combinations (BT.709 / full-range BT.601, upload / offscreen DMA)
confirm the intended chroma gain, unchanged luma/grayscale and unchanged metadata.
272 host tests pass (8 ignored), and the release build passes. Portal error assertions
now handle both missing runtime plugins and unbuilt support, fixing the R8 CI failure.

A paired isolated 3120×2080 Xvfb motion test measured median local capture/encode throughput
of 37.07 fps for R8 and 32.72 fps with gain 1.05 (about 11.7% lower, startup included).
This is not phone-delivered FPS or end-to-end latency. Real iPhone/iPad color and latency
acceptance is pending. Set gain back to 1.0 and restart to remove the extra processing.

Attribution and upstream GPL licensing are preserved; see NOTICE.md and LICENSE.
