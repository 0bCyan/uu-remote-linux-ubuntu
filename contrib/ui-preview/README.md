# Offline host UI regression preview

`preview.c` includes the real GTK host window, but never starts Rust, cloud login,
networking or the tray. All account/connection values are fixtures; its QR image
is deliberately non-scannable and says PREVIEW. Never use a live account for screenshots.

Build with GTK3 development files:

```sh
cc -Wno-deprecated-declarations $(pkg-config --cflags gtk+-3.0) \
  contrib/ui-preview/preview.c -o /tmp/uu-ui-preview \
  $(pkg-config --libs gtk+-3.0) -ldl -lpthread
mkdir -p /tmp/uu-ui-previews
```

On an **isolated Xvfb display**, run:

```sh
DISPLAY=:98 G_DEBUG=fatal-warnings NO_AT_BRIDGE=1 \
  /tmp/uu-ui-preview /tmp/uu-ui-previews
```

The display should be at least 1600×1200. The harness checks CSS parsing,
credential text allocation, actual copy/show/hide/switch callbacks, verification
and refresh command FIFO, QR image/retry states, large-font scaling, and
hide-on-close behavior. It renders signed-in, disabled, expanded settings,
loading, QR fixture, expired QR, login-only, compact and 150% text states.
Screenshots of small windows intentionally require vertical scrolling.

Design reference: [Apple: Meet Liquid Glass](https://developer.apple.com/cn/videos/play/wwdc2025/219/).
This is a GTK simulation using gradients/highlights; it uses no Apple assets,
compositor background blur or repeating decorative animation.
