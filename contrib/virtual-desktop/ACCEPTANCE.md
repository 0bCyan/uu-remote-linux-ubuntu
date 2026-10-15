# Virtual user-bus desktop acceptance

Install Xvfb, xauth, xdpyinfo, Xfce and dbus-user-session (Debian) or dbus-broker (Arch).
The default is `UU_VIRTUAL_BUS=user`; use it on a dedicated headless account with no real
GUI session. Its D-Bus and systemd activation environment is shared by all that user's apps.
`private` isolates ordinary D-Bus activation but cannot redirect systemd-activated programs.

1. Install the script in `~/.local/bin`, the unit in `~/.config/systemd/user`, and
   `virtual-display.conf` in the host service's `.service.d/` directory.
2. `sudo loginctl enable-linger "$USER"`; set `UU_VIRTUAL_SESSION=startxfce4` in the desktop
   unit. `systemctl --user daemon-reload`; enable both units. A different display needs a
   matching `DISPLAY` and `XAUTHORITY=%t/uu-virtual-N.Xauthority` in the host drop-in.
3. Reboot a disposable VM with graphical login disabled. Before logging into a desktop,
   check both user units are active, Xvfb is `:99`, no real GUI login session exists, and
   host availability is `ready`. The desktop unit uses notify readiness after authenticated
   X11 access succeeds, so the host does not race the X socket.
4. `DISPLAY=:99 XAUTHORITY="$XDG_RUNTIME_DIR/uu-virtual-99.Xauthority" xdpyinfo` must work.
   `gnome-terminal` launched over the user bus must open on `:99`. Check its window via
   `xdotool search --class gnome-terminal-server`; do not log terminal text.
5. Restart/stop the desktop unit: its Xvfb, desktop children and authority file must be
   removed. A stale pre-existing display is rejected. The TCP X11 listener is disabled.

## Official Mac client checklist (human acceptance)

- [ ] Select the isolated VM device; confirm its identity before connecting.
- [ ] After VM boot, connect without a physical/graphical login; Xfce is visible.
- [ ] Open GNOME Terminal through its application launcher; it appears on the same `:99` desktop.
- [ ] Move/click/drag, scroll, type a synthetic test marker and verify key releases (no repeats).
- [ ] Verify both clipboard directions using synthetic text; keep the remote window focused.
- [ ] Toggle 静音运行 during playback: remote sound remains audible; VM physical output is silent.
- [ ] Disconnect/reconnect; confirm desktop and applications remain usable.
- [ ] End the last session: audio default/modules restore; auto-lock follows the saved setting.
- [ ] Repeat after reboot with no GUI login. Do not enter production credentials in test fixtures.
