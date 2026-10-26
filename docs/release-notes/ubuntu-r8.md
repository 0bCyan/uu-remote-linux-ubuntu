# Ubuntu R8 — upstream v0.2.32 integration

Unofficial Ubuntu fork of VallierDev/uu-remote-linux; upstream history, authorship and GPL licensing retained.

- Recover stalled signaling using an independent heartbeat watchdog and bounded sends/peer shutdown.
- Read the real DTLS state after ICE recovers, preventing active tool peers from being treated as idle.
- Include opt-in NVENC VBV buffering and Linux viewer pointer/wheel fixes.
- Preserve R6/R7 color modes, capture scheduling, congestion adaptation and video/repair pacing.
- Validation: 272 host tests passed (8 ignored), 163 viewer tests passed in isolated serial execution.
- Release build and a 180-frame generated-motion VAAPI capture/encode smoke check passed.
- Phone interruption recovery and long-term stability are pending; no new delivered-FPS or final-loss claim.
