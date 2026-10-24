# X11 repeat regression

Run `key-repeat.c` only on a private Xvfb display `:98` (the harness refuses other
displays). It focuses a GTK entry containing generated fixture text. It never
starts the host or networking and prints counts only.

```sh
cc -Wno-deprecated-declarations $(pkg-config --cflags gtk+-3.0) \
  contrib/input-tests/key-repeat.c -o /tmp/uu-key-repeat \
  $(pkg-config --libs gtk+-3.0) -lX11 -lXext -lXtst -lXfixes -lpthread
```

Start a private `Xvfb :98 -screen 0 800x600x24 -nolisten tcp`, then run:

```sh
DISPLAY=:98 G_DEBUG=fatal-warnings NO_AT_BRIDGE=1 /tmp/uu-key-repeat
```

Stop only the Xvfb process you started when finished. Checks: five symbolic and
seven raw repeated Backspace downs; repeated letters/arrows; an 800 ms delayed
key-up with no extra deletion; no intermediate modifier release; Ctrl+A; and
session cleanup restores auto-repeat and releases held input. The pre-fix
engine deletes one character for five Backspace downs and fails the assertion.
This covers explicit viewer repeats; it does not emulate a viewer that sends
only one down/up pair for a long hold.
