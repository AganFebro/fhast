# Contributing

## Prerequisites

- Read `docs/architecture.md` for the system design
- Read `AGENTS.md` for code conventions and design rules
- Read `project.md` for the product specification

## Development Workflow

```bash
# Fork and clone
git clone <your-fork> fhast
cd fhast

# Build everything
cargo build

# Run all tests
cargo test

# Format and lint before committing
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings

# Extension checks
cd extension
npm install
npm run lint
npm run format
cd ..
```

## Commit Convention

Use [Conventional Commits](https://www.conventionalcommits.org/):

```
feat(core): add segmented range planner
fix(extension): redact cookie header from logs
docs(architecture): document IPC protocol
refactor(daemon): extract download scheduler
test(core): add resume-after-restart integration test
```

Prefix the scope with the crate or area:
- `core` — fhast-core (engine, storage, models)
- `ipc` — fhast-ipc (messages, transport)
- `daemon` — fhast-daemon
- `cli` — fhast-cli
- `tui` — fhast-tui
- `native-host` — fhast-native-host
- `extension` — Chrome extension

## PR Guidelines

- Keep PRs **small** — one feature or fix per PR
- Include **tests** for new behavior
- Update **roadmap.md** if the change completes a roadmap item
- Add **migration notes** in the PR description if schemas, config, or IPC messages change
- Ensure CI-style checks pass: `cargo fmt`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`
- For extension changes: ensure `npm run lint` and `npm run format` pass

## Extension Debugging

**Service worker console**: `chrome://extensions/` → fhast → "Service Worker" shows real-time logs:
- `fhast: redirect tracked domain1 → domain2 + N headers + M sensitive`
- `fhast: grabbed filename | N headers | WITH/NO cookies | CD-filename | using redirected URL/original URL + redirected headers/original URL | N sensitive headers`
- `fhast: skipping duplicate ...` (dedup within 5-second window)
- `fhast: ignoring non-candidate download ...` (download did not match extension, MIME, filename, or Content-Disposition checks)
- `fhast: queued <uuid>` (native host confirmed)

**Header flow trace**:
1. Extension popup: shows 🍪 (cookies present), header counts (`13h +1s`), redirect info (`↩ cdn.example.com`)
2. Native host stderr: `forwarding download with 13 normal headers + 1 sensitive headers`
3. Daemon stderr (`RUST_LOG=info`): `stored download headers normal=13 sensitive=1`
4. TUI detail view (Enter on download): Headers panel shows all headers with color coding

**Common issues**:
- **NO cookies** in log: ensure `"extraHeaders"` is in webRequest listener extraInfoSpec
- **Headers 0 in TUI**: restart daemon after `cargo build`, ensure download ID matches
- **410/403 after auto-grab redirect**: check the `fhast: grabbed ...` URL mode. Redirect-chain captures should keep the original URL and reuse redirected headers, otherwise one-shot or referer-protected final URLs can fail after Chrome already reached them.
- **Service worker status 15**: add `"type": "module"` to manifest background config

## Code Style

### Rust

- `rustfmt` defaults
- `snake_case` for crates/files/modules/functions/variables
- `UpperCamelCase` for types/traits/enums
- `SCREAMING_SNAKE_CASE` for constants/statics
- Avoid `unwrap`/`expect` outside tests — return typed errors
- Public items need `///` docs
- Internal comments explain **why**, not what

### TypeScript

- `strict: true` in tsconfig
- Avoid `any`
- `camelCase` for values/functions
- `PascalCase` for types/classes
- `CONSTANT_CASE` for constants

### SQL

- `snake_case` table/column names
- Prepared queries only
- No string-concatenated user input

### Shell

- POSIX `sh` unless Bash is required
- Bash: `set -euo pipefail`, quote variables

## Security Rules

- Never log cookies, auth headers, signed URLs, or tokens
- Sanitize filenames; reject path traversal
- Protect save directories
- Extension: minimal permissions only, explicit user action for capture
- Native host: validate schema and origin, redact sensitive data

## Testing

**Unit tests**: co-located with source in `#[cfg(test)] mod tests { }` blocks.

**Integration tests**: in `crates/fhast-core/tests/` using the embedded test server (`test_harness/mod.rs`). The test server supports 11 modes:
- Normal, RangeSupported, RangeIgnored
- Slow, Flaky, ConnectionReset
- ForbiddenAfterN, RateLimited
- ChangingEtag, ChangedData, RangeThenIgnore

To add a new test server mode, add a variant to `ServerMode` and implement its handler in `handle_connection`.

Coverage targets for the downloader:
- Range math, resume validation
- Retry/backoff behavior
- File merge correctness
- Checksum success and mismatch
- Sensitive header redaction and cleanup
- Crash recovery of queued and active jobs
