# Chrome Extension & Native Host

The Chrome extension captures download links from the browser and sends them to fhast-daemon through the native messaging host.

## Extension (`extension/`)

### Architecture

```text
manifest.json (Manifest V3)
  |
  +-- background.ts (service worker, type: module)
  |     Register context menu, handle link clicks
  |
  +-- popup.html + popup.ts (toolbar popup)
  |     Manual URL entry, send current page URL
  |
  +-- message_types.ts (shared types)
        AddDownloadMessage, NativeResponse, NATIVE_HOST, VERSION
```

### Permissions

**MVP (always granted):**
- `contextMenus` — right-click link capture
- `nativeMessaging` — communicate with native host
- `activeTab` — read current page URL
- `storage` — persist auto-grab preferences and capture history

**Optional (user-initiated via popup toggle):**
- `downloads` — observe and cancel Chrome downloads for auto-grab
- `webRequest` — capture request/response headers including cookies
- `<all_urls>` host permission — needed for webRequest to observe all requests

No cookies, webRequest, or broad host permissions in default install — all are opt-in via the "Enable Auto-Grab" button.

### Capture Methods

1. **Right-click a link** → "Send link to fhast"  
   Captures `linkUrl` and `pageUrl`, sends to native host.

2. **Toolbar popup** → type a URL → "Send to fhast"  
   Sends manually entered URL.

3. **Toolbar popup** → "Send current page URL"  
   Captures active tab URL and sends it.

4. **Auto-Grab** (opt-in)  
   When enabled via popup toggle:
   - Intercepts all browser downloads via `downloads.onDeterminingFilename`
   - Cancels Chrome's download and sends to fhast instead
   - Captures request headers (cookies, referer, user-agent) via `webRequest.onBeforeSendHeaders`
   - Captures response headers (Content-Disposition filenames, Content-Length, Content-Type, etc.)
   - Tracks redirect chains: cookies from the original URL follow through 302 redirects to CDN URLs
   - Deduplicates: same URL won't be queued twice within 5 seconds
   - Shows recent captures in the popup with 🍪 cookie indicator, header counts, redirect info

### Response Header Capture

`onHeadersReceived` captures all response headers and forwards them to fhast as `x-fhast-rh:HeaderName` prefixed headers. The `Content-Disposition` filename is extracted and used as the download filename (priority #1, over Chrome's guess or URL parsing).

### Debugging

Open the service worker console at `chrome://extensions/` → fhast → "Service Worker" to see:
- `fhast: redirect tracked domain1 → domain2 + N headers + M sensitive` — redirect chain tracking
- `fhast: grabbed filename | N headers | WITH/NO cookies | CD-filename | using CDN/original URL` — download capture details
- `fhast: skipping duplicate ...` — dedup in action
- `fhast: queued <uuid>` — native host confirmed download queued

The popup shows per-capture debug: 🍪 = cookies present, `↩ hostname` = redirect chain, `13h +1s` = 13 normal + 1 sensitive header.

### Message Schema

Extension sends this to the native host:

```json
{
  "type": "add_download",
  "version": 1,
  "url": "https://cdn.example.com/file.zip",
  "page_url": "https://example.com/download"
}
```

### Development

```bash
cd extension
npm install
npm run build    # Compile TypeScript to dist/
npm run lint     # Type check only
npm run format   # Prettier
```

### Loading in Chrome

1. Go to `chrome://extensions/`
2. Enable "Developer mode"
3. Click "Load unpacked"
4. Select the `extension/` directory

Reload after changes by clicking the refresh icon.

## Native Host (`crates/fhast-native-host`)

The native host is a Rust binary that bridges Chrome's Native Messaging protocol to the daemon's Unix socket IPC.

### Protocol

Chrome communicates with native hosts via **32-bit length-prefixed JSON** on stdin/stdout:

```text
[4 bytes: message length, uint32, native byte order (LE)]
[JSON payload]
```

The native host:
1. Reads 4-byte length from stdin
2. Reads that many bytes as JSON
3. Validates the message schema
4. Forwards to daemon via Unix socket
5. Writes 4-byte length + JSON response to stdout

### Building

```bash
cargo build -p fhast-native-host
```

### Installation

```bash
cargo run -p fhast-native-host -- --install-host
```

Or manually:

```bash
# Generate manifest
cargo run -p fhast-native-host -- --manifest

# Write to Chrome's native messaging directory
# (replace EXTENSION_ID with the actual one from chrome://extensions/)
mkdir -p ~/.config/google-chrome/NativeMessagingHosts/
cat > ~/.config/google-chrome/NativeMessagingHosts/fhast_native_host.json << EOF
{
  "name": "fhast_native_host",
  "description": "fhast Native Messaging Host",
  "path": "/absolute/path/to/fhast-native-host",
  "type": "stdio",
  "allowed_origins": ["chrome-extension://EXTENSION_ID/"]
}
EOF
```

### Debugging

The native host outputs to stderr. To see logs:

```bash
RUST_LOG=info fhast-native-host
```

Common issues:

- **"Specified native messaging host not found"**: manifest not installed or path is wrong
- **"Access to the specified native messaging host is forbidden"**: extension ID doesn't match `allowed_origins`
- **Service worker registration failed (status 15)**: missing `"type": "module"` in manifest background config
- **1832016667 bytes long**: native host wrote tracing output to stdout; ensure `.with_writer(io::stderr)` on tracing subscriber
