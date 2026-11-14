# Window switcher

Jump directly to an open window instead of cycling through applications. The
picker lists window titles with their application, and accepting a row raises
that window.

## Usage

```sh
mm -o switcher
```

The preset selects `macos.main.toml` or `linux.main.toml` for the current
platform. The Windows implementation is not available.

Requirements on macOS: Screen Recording permission to read window titles and
Accessibility permission to focus other applications, both granted to your
terminal. The bundled `list` and `switch` helpers are arm64 builds; rebuild them
from the `.swift` sources if needed. The preview uses `mdfind`, `defaults`,
`sips`, and `launchctl`.

Requirements on Linux: `xdotool` and `wmctrl` on `PATH`, running under X11 or a
compatible window manager. The `list.sh` and `switch.sh` helpers are the
platform implementation used by the preset.

Each row is one window. On Linux the currently focused window is omitted, since
switching to it would be a no-op.

`<enter>` focuses the selected window and exits Matchmaker. On Linux, `@return`
focuses the selection and minimizes the window that was active before the
switch; on macOS it is a plain accept. `@copy` copies the macOS application
bundle path or the Linux window id, and `alt-r` reveals the macOS application
in Finder. The macOS preview renders the application icon; the Linux preview
shows the title, application, PID, and window id.
