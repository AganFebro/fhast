# Configuration

## Config File

Persistent config is created automatically on first `fhast config set` or GUI config save.

| OS | Config path |
|---|---|
| Linux/macOS | `~/.config/fhast/config.json` |
| Windows | `%APPDATA%\fhast\config.json` |

```json
{
  "max_retries": 5,
  "max_connections_per_download": 8,
  "max_connections_global": 16,
  "max_connections_per_host": 8,
  "output_dir": null,
  "sensitive_header_retention": "until_complete",
  "chrome_extension_id": ""
}
```

| Key | Type | Default | Description |
|---|---|---|---|
| `max_retries` | u8 | 5 | Max retry attempts for transient failures |
| `max_connections_per_download` | u16 | 8 | Segments per download |
| `max_connections_global` | u16 | 16 | Global connection cap |
| `max_connections_per_host` | u16 | 8 | Per-domain connection cap |
| `output_dir` | path\|null | null | Override default output directory |
| `sensitive_header_retention` | string | `until_complete` | `never`, `until_complete`, or `encrypted` |
| `chrome_extension_id` | string | empty | Chrome extension ID used when installing the native messaging host |

## CLI Commands

```bash
fhast config get                    # Show all config as JSON
fhast config get max_retries        # Show single key
fhast config set max_retries 10     # Set a value
fhast config set output_dir ~/Downloads
fhast config set sensitive_header_retention never
fhast config set chrome_extension_id abcdefghijklmnopabcdefghijklmnop
```

## Environment Variables

| Variable | Default | Description |
|---|---|---|
| `fhast_EXTENSION_ID` | — | Chrome extension ID for native host manifest |
| `RUST_LOG` | `off` (native host), `info` (daemon) | Tracing log level filter |
| `XDG_RUNTIME_DIR` | `/run/user/<uid>` | Runtime directory for daemon socket |
| `XDG_STATE_HOME` | `~/.local/state` | State directory for database and job files |
| `HOME` | — | Fallback home directory for state path |
| `APPDATA` | `%USERPROFILE%\AppData\Roaming` | Windows config directory base |
| `LOCALAPPDATA` | `%USERPROFILE%\AppData\Local` | Windows state/runtime directory base |
| `USERPROFILE` | — | Windows home directory fallback |

## File Paths

| OS | Path | Purpose |
|---|---|---|
| Linux | `~/.local/state/fhast/fhast.db` | SQLite database |
| Linux | `~/.local/state/fhast/jobs/<id>/` | Job temp directories (part files, metadata) |
| Linux | `/run/user/<uid>/fhast/fhast.sock` | Daemon Unix domain socket |
| Linux | `~/.config/google-chrome/NativeMessagingHosts/fhast_native_host.json` | Chrome native host manifest |
| Windows | `%LOCALAPPDATA%\fhast\fhast.db` | SQLite database |
| Windows | `%LOCALAPPDATA%\fhast\jobs\<id>\` | Job temp directories (part files, metadata) |
| Windows | `\\.\pipe\fhast-<user>` | Daemon named pipe |
| Windows | `HKCU\Software\Google\Chrome\NativeMessagingHosts\fhast_native_host` | Chrome native host registry key |
| Windows | `%APPDATA%\fhast\native-host\fhast_native_host.json` | Chrome native host manifest file |

## Download Defaults

| Setting | Value |
|---|---|
| Connections per download | 8 |
| Global max connections | 16 |
| Max connections per host | 8 |
| Min segment size | 8 MiB |
| Output directory | Current working directory for CLI/TUI; GUI uses configured `output_dir`, then `Downloads`, then executable directory |
| Retry behavior | Exponential backoff, bounded attempts |

## Privacy Defaults

| Setting | Value |
|---|---|
| Sensitive header retention | `until_complete` |
| Sensitive headers logged | Never |
| Telemetry | None |
| Cookie storage | Temporary, deleted after completion |

## Log Levels

Set `RUST_LOG` to control tracing output:

```bash
# Daemon verbose logging
RUST_LOG=debug cargo run -p fhast-cli -- daemon start

# Specific crate
RUST_LOG=fhast_daemon=trace cargo run -p fhast-cli -- daemon start

# Native host debugging
RUST_LOG=info ./target/debug/fhast-native-host
```

Logs use structured `tracing` format and include job IDs and domains without exposing secrets.
