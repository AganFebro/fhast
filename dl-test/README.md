# fhast hostile download lab

A local test bench for fhast-like download managers. It mimics annoying real-world download behavior: temporary redirected URLs, one-shot final links, cookies, referers, expiring tokens, range edge cases, throttling, retry failures, and broken streams.

## Run

```bash
npm install
npm start
```

Then open <http://127.0.0.1:8787> in Chrome.

No npm dependency is required; the server uses Node's built-in `http` module.

## Main bug reproduction

Open this in Chrome:

```text
http://127.0.0.1:8787/case/original-302-oneshot?size=32mb&ttl=60
```

Expected behavior:

1. Browser requests the original `/case/original-302-oneshot` URL.
2. Server returns `302` to a temporary `/token/oneshot/...` final URL.
3. Browser reaches the final URL and consumes it.
4. If fhast auto-grab replays the final `/token/...` URL after Chrome consumed it, the lab returns `410 Gone`.
5. If fhast stores/replays the original `/case/...` URL, the server issues a fresh token and fhast should succeed.

If fhast owns a fresh token first, segmented `Range` requests are allowed as one range group. That prevents the lab from falsely failing a correct segmented downloader.

## Useful endpoints

- `/case/direct?size=32mb` stable direct ranged file.
- `/case/no-range?size=32mb` ignores `Range` and returns full `200`.
- `/case/lie-range?size=32mb` advertises `Accept-Ranges` but still returns full `200`.
- `/case/chunked?size=32mb` stream with no `Content-Length`.
- `/case/original-302-oneshot?size=32mb&ttl=60` original URL redirects to one-shot final URL.
- `/case/original-cookie-302?size=32mb&ttl=60` sets cookie then redirects to protected final URL.
- `/case/original-referer-302?size=32mb&ttl=60` final URL requires `Referer`.
- `/case/original-expiring-302?size=32mb&ttl=5` final URL expires quickly.
- `/case/range-single?size=32mb&bps=512kb` only one concurrent range request allowed.
- `/case/429-then-ok?size=32mb&failFirst=2` returns `429` twice, then succeeds.
- `/case/slow?size=32mb&bps=256kb` throttled download.
- `/case/fail-midstream?size=32mb&failAfter=4mb` drops connection after 4 MiB.
- `/case/head-forbidden?size=32mb` rejects `HEAD`, but `GET` works.

## Inspect behavior

- `/logs` returns recent request logs as JSON.
- `/cases` returns the scenario list as JSON.
- `/reset` clears token and request state.

## Manual checks

```bash
curl -L -OJ 'http://127.0.0.1:8787/case/direct?size=8mb'
curl -L -OJ 'http://127.0.0.1:8787/case/original-302-oneshot?size=8mb'
curl -H 'Range: bytes=0-99' -v 'http://127.0.0.1:8787/case/direct?size=8mb' -o range.bin
curl 'http://127.0.0.1:8787/logs'
```
