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
   `xdotool search --class '[Gg]nome.[Tt]erminal|gnome-terminal-server'` (window
   classes vary by GNOME Terminal version; the server process name is not always
   its window class). Do not log window titles or terminal text.
5. Restart/stop the desktop unit: its Xvfb, desktop children and authority file must be
   removed. A stale pre-existing display is rejected. The TCP X11 listener is disabled.
