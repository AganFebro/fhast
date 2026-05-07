# Troubleshooting

## Daemon Issues

### `fhast daemon start` says "address already in use"

A daemon is already running. Stop it first:

```bash
fhast daemon stop
```

Or kill it manually:

```bash
pkill fhast-daemon
```

Then remove the stale socket:

```bash
rm /run/user/$(id -u)/fhast/fhast.sock
```

### `fhast add` says "connection refused"

The daemon isn't running. Start it:

```bash
fhast daemon start
```

The `add` command autostarts the daemon, but if it fails, start it explicitly.

### Downloads not progressing in TUI

1. Check the daemon is running: `ls /run/user/$(id -u)/fhast/fhast.sock`
2. Check the daemon logs: start with `RUST_LOG=debug fhast daemon start`
3. Check if the URL is reachable: `curl -I <url>`

## Chrome Extension Issues

### Service worker registration failed (status 15)

The `manifest.json` is missing `"type": "module"` in the background config. Ensure:

```json
"background": {
    "service_worker": "dist/background.js",
    "type": "module"
}
```

Reload the extension after fixing.

### "Specified native messaging host not found"

Chrome can't find the native host binary or manifest.

1. Check the manifest exists: `cat ~/.config/google-chrome/NativeMessagingHosts/fhast_native_host.json`
2. Check the binary exists at the path in the manifest
3. Check the binary is executable: `chmod +x <path>`
4. Reload the extension at `chrome://extensions/`

### "Access to native messaging host is forbidden"

The extension ID in `allowed_origins` doesn't match. Get the correct ID from `chrome://extensions/` and reinstall:

```bash
export fhast_EXTENSION_ID="<actual-id>"
cargo run -p fhast-native-host -- --install-host
```

### Error: "Native Messaging host tried sending a message that is 1832016667 bytes long"

The native host is writing ANSI escape codes to stdout (from tracing). Ensure the native host has `.with_writer(io::stderr)` on the tracing subscriber and that logging is suppressed by default. Rebuild:

```bash
cargo build -p fhast-native-host
```

### Popup shows no response, nothing happens

The daemon might not be running. Ensure `fhast daemon start` is running, then try again.

## TUI Issues

### TUI shows "Disconnected" in status bar

The daemon isn't running or the socket is inaccessible. Check with `fhast daemon start`.

### Keyboard input not working

The TUI captures terminal input. Make sure the terminal window has focus. If `j/k` navigation works but other keys don't, check the keybinding table in `docs/tui.md`.

### Search/filter not working

Press `/` to enter filter mode, type your filter text (matches against download ID and URL), then press Enter or Escape to exit. The download list will show only matching items.

## Download Issues

### Download marked `needs_refresh`

The remote file changed or the URL expired. For extension-captured links, re-open the source page and capture the link again.

### Download marked `failed` with checksum mismatch

The downloaded file doesn't match the expected checksum. The temp data is kept for inspection at the job's temp directory.

### Segmented download fallback to single connection

The server doesn't support Range requests. This is normal behavior — fhast always tests range support and falls back gracefully.

### Download speed seems slow

- Check connection limits with `fhast info <id> --json`
- Some servers limit connections per IP
- Try `fhast add <url> --connections 4` to adjust

## General

### Database corruption

The SQLite database is at `~/.local/state/fhast/fhast.db`. Back up and rebuild:

```bash
fhast daemon stop
mv ~/.local/state/fhast/fhast.db ~/.local/state/fhast/fhast.db.bak
fhast daemon start
```

This starts fresh — all download history is lost.

### Logs

Daemon logs go to stderr. To capture them:

```bash
RUST_LOG=debug cargo run -p fhast-cli -- daemon start 2> fhast-daemon.log
```

### Clean reset

Remove all state and start fresh:

```bash
fhast daemon stop
rm -rf ~/.local/state/fhast/
rm -f /run/user/$(id -u)/fhast/fhast.sock
fhast daemon start
```
