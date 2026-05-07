import http from 'node:http';
import crypto from 'node:crypto';
import { URL } from 'node:url';

const PORT = Number(process.env.LAB_PORT || process.env.PORT || 8787);
const HOST = process.env.LAB_HOST || process.env.HOST || '127.0.0.1';
const DEFAULT_SIZE = 32 * 1024 * 1024;
const tokens = new Map();
const requests = [];
let requestSeq = 0;

function nowIso() { return new Date().toISOString(); }
function baseUrl(req) { return `http://${req.headers.host}`; }
function tokenId() { return crypto.randomBytes(10).toString('hex'); }

function parseBytes(input, fallback = DEFAULT_SIZE) {
  if (!input) return fallback;
  const s = String(input).trim().toLowerCase();
  const m = s.match(/^(\d+(?:\.\d+)?)(b|kb|kib|mb|mib|gb|gib)?$/);
  if (!m) return fallback;
  const n = Number(m[1]);
  const unit = m[2] || 'b';
  const mult = unit === 'gb' || unit === 'gib' ? 1024 ** 3
    : unit === 'mb' || unit === 'mib' ? 1024 ** 2
    : unit === 'kb' || unit === 'kib' ? 1024
    : 1;
  return Math.max(1, Math.floor(n * mult));
}

function selectedHeaders(headers) {
  const names = ['host', 'range', 'user-agent', 'referer', 'cookie', 'authorization', 'accept', 'accept-encoding', 'x-fhast-original-url', 'x-lab-download'];
  const out = {};
  for (const name of names) if (headers[name]) out[name] = String(headers[name]).slice(0, 300);
  return out;
}

function logRequest(req, extra = {}) {
  const row = { id: ++requestSeq, at: nowIso(), method: req.method, url: req.url, headers: selectedHeaders(req.headers), ...extra };
  requests.push(row);
  if (requests.length > 1000) requests.shift();
  console.log(`${row.id} ${row.method} ${row.url} ${extra.status || ''} ${extra.note || ''}`);
  return row;
}

function sendJson(res, status, body, headers = {}) {
  const bytes = Buffer.from(JSON.stringify(body, null, 2));
  res.writeHead(status, { 'Content-Type': 'application/json; charset=utf-8', 'Content-Length': bytes.length, 'Cache-Control': 'no-store', ...headers });
  res.end(bytes);
}

function sendText(res, status, body, headers = {}) {
  const bytes = Buffer.from(body);
  res.writeHead(status, { 'Content-Type': 'text/plain; charset=utf-8', 'Content-Length': bytes.length, 'Cache-Control': 'no-store', ...headers });
  res.end(bytes);
}

function sendHtml(res, body) {
  const bytes = Buffer.from(body);
  res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8', 'Content-Length': bytes.length, 'Cache-Control': 'no-store' });
  res.end(bytes);
}

function redirect(res, status, location, headers = {}) {
  res.writeHead(status, { Location: location, 'Cache-Control': 'no-store', ...headers });
  res.end(`Redirecting to ${location}\n`);
}

function contentDisposition(filename) {
  const safe = filename.replace(/["\\\r\n]/g, '_');
  return `attachment; filename="${safe}"`;
}

function makeToken(kind, opts = {}) {
  const id = tokenId();
  const ttlMs = Number(opts.ttlMs ?? 60_000);
  const tok = {
    id,
    kind,
    createdAt: Date.now(),
    expiresAt: Date.now() + ttlMs,
    maxUses: opts.maxUses ?? Infinity,
    uses: 0,
    attempts: 0,
    size: opts.size ?? DEFAULT_SIZE,
    filename: opts.filename ?? `${kind}-${id}.bin`,
    rangeMode: opts.rangeMode ?? 'range',
    throttleBps: opts.throttleBps ?? 0,
    chunkSize: opts.chunkSize ?? 64 * 1024,
    failFirst: opts.failFirst ?? 0,
    failAfterBytes: opts.failAfterBytes ?? 0,
    requiredCookie: opts.requiredCookie ?? null,
    requiredRefererIncludes: opts.requiredRefererIncludes ?? null,
    requiredHeader: opts.requiredHeader ?? null,
    maxConcurrent: opts.maxConcurrent ?? Infinity,
    active: 0,
    markUsedAtStart: opts.markUsedAtStart ?? true,
    allowRangeGroup: opts.allowRangeGroup ?? false,
    rangeGroupStarted: false,
  };
  tokens.set(id, tok);
  return tok;
}

function deterministicChunk(offset, len) {
  const buf = Buffer.allocUnsafe(len);
  for (let i = 0; i < len; i++) buf[i] = (offset + i) % 251;
  return buf;
}

function wait(ms) { return new Promise(resolve => setTimeout(resolve, ms)); }

function isClientAbort(err) {
  return err && (err.code === 'ECONNRESET' || err.code === 'ECONNABORTED' || err.message === 'response destroyed' || String(err.message || '').includes('aborted'));
}

function writeAsync(res, buf) {
  return new Promise((resolve, reject) => {
    if (res.destroyed || res.writableEnded) return reject(Object.assign(new Error('response destroyed'), { code: 'ECONNABORTED' }));

    let settled = false;
    function done(err) {
      if (settled) return;
      settled = true;
      res.off('error', onError);
      res.off('close', onClose);
      res.off('drain', onDrain);
      err ? reject(err) : resolve();
    }
    function onError(err) { done(err); }
    function onClose() {
      if (!res.writableEnded) done(Object.assign(new Error('client aborted'), { code: 'ECONNABORTED' }));
    }
    function onDrain() { done(); }

    res.once('error', onError);
    res.once('close', onClose);
    const ok = res.write(buf, err => done(err));
    if (!ok) res.once('drain', onDrain);
  });
}

function parseRange(rangeHeader, size) {
  if (!rangeHeader) return null;
  const m = String(rangeHeader).match(/^bytes=(\d*)-(\d*)$/);
  if (!m) return { invalid: true };
  let start;
  let end;
  if (m[1] === '' && m[2] === '') return { invalid: true };
  if (m[1] === '') {
    const suffix = Number(m[2]);
    if (!Number.isFinite(suffix) || suffix <= 0) return { invalid: true };
    start = Math.max(0, size - suffix);
    end = size - 1;
  } else {
    start = Number(m[1]);
    end = m[2] === '' ? size - 1 : Number(m[2]);
  }
  if (!Number.isFinite(start) || !Number.isFinite(end) || start < 0 || end < start || start >= size) return { invalid: true };
  return { start, end: Math.min(end, size - 1) };
}

async function streamBytes(req, res, start, end, opts) {
  let sent = 0;
  let pos = start;
  const chunkSize = opts.chunkSize || 64 * 1024;
  const started = Date.now();

  try {
    while (pos <= end) {
      if (req.aborted || res.destroyed || res.writableEnded) return;
      const len = Math.min(chunkSize, end - pos + 1);
      if (opts.failAfterBytes && sent >= opts.failAfterBytes) {
        res.destroy(new Error(`lab midstream failure after ${sent} bytes`));
        return;
      }
      await writeAsync(res, deterministicChunk(pos, len));
      sent += len;
      pos += len;
      if (opts.throttleBps > 0) {
        const expectedElapsed = (sent / opts.throttleBps) * 1000;
        const actualElapsed = Date.now() - started;
        if (expectedElapsed > actualElapsed) await wait(expectedElapsed - actualElapsed);
      }
    }
    if (!res.destroyed && !res.writableEnded) res.end();
  } catch (err) {
    if (!isClientAbort(err)) throw err;
  }
}


async function serveVirtualFile(req, res, opts = {}) {
  const size = Number(opts.size ?? DEFAULT_SIZE);
  const filename = opts.filename ?? 'lab-file.bin';
  const rangeMode = opts.rangeMode ?? 'range';
  const etag = `"fhast-lab-${size}-${rangeMode}"`;
  const common = {
    'Content-Type': 'application/octet-stream',
    'Content-Disposition': contentDisposition(filename),
    'ETag': etag,
    'Last-Modified': new Date('2026-01-01T00:00:00Z').toUTCString(),
    'Cache-Control': 'no-store',
  };
  if (rangeMode !== 'no-range') common['Accept-Ranges'] = 'bytes';

  if (rangeMode === 'chunked') {
    res.writeHead(200, common);
    if (req.method === 'HEAD') return res.end();
    return streamBytes(req, res, 0, size - 1, opts);
  }

  const range = parseRange(req.headers.range, size);
  if (range?.invalid && rangeMode === 'range') {
    res.writeHead(416, { ...common, 'Content-Range': `bytes */${size}` });
    return res.end();
  }

  if (range && !range.invalid && rangeMode === 'range') {
    const length = range.end - range.start + 1;
    res.writeHead(206, { ...common, 'Content-Length': length, 'Content-Range': `bytes ${range.start}-${range.end}/${size}` });
    if (req.method === 'HEAD') return res.end();
    return streamBytes(req, res, range.start, range.end, opts);
  }

  res.writeHead(200, { ...common, 'Content-Length': size });
  if (req.method === 'HEAD') return res.end();
  return streamBytes(req, res, 0, size - 1, opts);
}

function queryOpts(url) {
  return {
    size: parseBytes(url.searchParams.get('size'), DEFAULT_SIZE),
    ttlMs: Number(url.searchParams.get('ttl') || 60) * 1000,
    filename: url.searchParams.get('name') || undefined,
    throttleBps: parseBytes(url.searchParams.get('bps'), 0),
    failFirst: Number(url.searchParams.get('failFirst') || 0),
    failAfterBytes: parseBytes(url.searchParams.get('failAfter'), 0),
  };
}

function caseLinks(origin) {
  return [
    ['Direct stable ranged file', '/case/direct?size=32mb'],
    ['No Range support: ignores segmentation', '/case/no-range?size=32mb'],
    ['Lies about Range: Accept-Ranges but returns 200', '/case/lie-range?size=32mb'],
    ['Chunked stream: no Content-Length', '/case/chunked?size=32mb'],
    ['Original URL redirects to one-shot final URL', '/case/original-302-oneshot?size=32mb&ttl=60'],
    ['Original URL creates a fresh final token per request', '/case/original-per-request-token?size=32mb&ttl=60'],
    ['Cookie + one-shot final URL', '/case/original-cookie-302?size=32mb&ttl=60'],
    ['Referer-required final URL', '/case/original-referer-302?size=32mb&ttl=60'],
    ['Token expires quickly', '/case/original-expiring-302?size=32mb&ttl=5'],
    ['Only one concurrent range allowed', '/case/range-single?size=32mb&bps=512kb'],
    ['429 twice, then success', '/case/429-then-ok?size=32mb&failFirst=2'],
    ['Slow throttled download', '/case/slow?size=32mb&bps=256kb'],
    ['Midstream connection drop', '/case/fail-midstream?size=32mb&failAfter=4mb'],
    ['HEAD forbidden, GET works', '/case/head-forbidden?size=32mb'],
  ].map(([title, path]) => ({ title, path, url: origin + path }));
}

function escapeHtml(s) { return String(s).replace(/[&<>"']/g, ch => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[ch])); }

function dashboard(req, res) {
  const origin = baseUrl(req);
  const rows = caseLinks(origin).map(c => `<tr><td>${escapeHtml(c.title)}</td><td><a href="${c.path}">${escapeHtml(c.path)}</a></td><td><code>curl -L -OJ '${c.url}'</code></td></tr>`).join('\n');
  sendHtml(res, `<!doctype html><html><head><meta charset="utf-8"><title>fhast Download Lab</title><style>body{font-family:system-ui,sans-serif;max-width:1100px;margin:2rem auto;line-height:1.4}table{border-collapse:collapse;width:100%}td,th{border:1px solid #ddd;padding:.5rem;vertical-align:top}code{white-space:pre-wrap}.hint{background:#f6f6f6;padding:1rem}</style></head><body><h1>fhast Download Lab</h1><p>Local server for testing redirect handling, one-shot URLs, temporary tokens, cookies, range requests, throttling, retries, and broken streams.</p><p class="hint"><b>Main bug reproduction:</b> click <code>/case/original-302-oneshot</code>. If fhast stores/replays the redirected <code>/token/...</code> URL after Chrome already reached it, the lab returns <code>410 Gone</code>. If fhast stores the original <code>/case/...</code> URL, it receives a fresh token and should work. If fhast owns that fresh token first, multiple Range segment requests are allowed as one range group.</p><p><a href="/logs">View logs</a> | <a href="/cases">JSON cases</a> | <a href="/reset">Reset tokens/logs</a></p><table><thead><tr><th>Scenario</th><th>Browser link</th><th>curl command</th></tr></thead><tbody>${rows}</tbody></table></body></html>`);
}

async function handleToken(req, res, kind, id) {
  const tok = tokens.get(id);
  if (!tok) {
    logRequest(req, { status: 410, note: 'unknown token' });
    return sendText(res, 410, '410 Gone: unknown token\n');
  }
  tok.attempts += 1;
  if (Date.now() > tok.expiresAt) {
    logRequest(req, { status: 410, token: id, note: 'expired token' });
    return sendText(res, 410, '410 Gone: token expired\n');
  }
  const isRangeGet = req.method === 'GET' && Boolean(req.headers.range);
  const rangeGroupBypass = tok.allowRangeGroup && tok.rangeGroupStarted && isRangeGet;
  if (tok.uses >= tok.maxUses && !rangeGroupBypass) {
    logRequest(req, { status: 410, token: id, note: 'token already used' });
    return sendText(res, 410, '410 Gone: token already used\n');
  }
  if (tok.requiredCookie && !String(req.headers.cookie || '').includes(tok.requiredCookie)) {
    logRequest(req, { status: 403, token: id, note: 'missing required cookie' });
    return sendText(res, 403, `403 Forbidden: missing cookie ${tok.requiredCookie}\n`);
  }
  if (tok.requiredRefererIncludes && !String(req.headers.referer || '').includes(tok.requiredRefererIncludes)) {
    logRequest(req, { status: 403, token: id, note: 'missing required referer' });
    return sendText(res, 403, `403 Forbidden: referer must include ${tok.requiredRefererIncludes}\n`);
  }
  if (tok.requiredHeader) {
    const got = req.headers[tok.requiredHeader.name.toLowerCase()];
    if (got !== tok.requiredHeader.value) {
      logRequest(req, { status: 403, token: id, note: 'missing required header' });
      return sendText(res, 403, `403 Forbidden: ${tok.requiredHeader.name}: ${tok.requiredHeader.value} required\n`);
    }
  }
  if (tok.failFirst > 0 && tok.attempts <= tok.failFirst) {
    logRequest(req, { status: 429, token: id, note: `planned 429 ${tok.attempts}/${tok.failFirst}` });
    return sendText(res, 429, '429 Too Many Requests: planned lab failure\n', { 'Retry-After': '2' });
  }
  if (tok.active >= tok.maxConcurrent) {
    logRequest(req, { status: 429, token: id, note: 'max concurrent reached' });
    return sendText(res, 429, '429 Too Many Requests: only one concurrent stream allowed\n', { 'Retry-After': '1' });
  }
  if (tok.markUsedAtStart && req.method === 'GET' && !rangeGroupBypass) {
    tok.uses += 1;
    if (tok.allowRangeGroup && isRangeGet) tok.rangeGroupStarted = true;
  }
  tok.active += 1;
  logRequest(req, { status: 'stream', token: id, note: `${kind} uses=${tok.uses} active=${tok.active}` });
  try {
    await serveVirtualFile(req, res, tok);
    if (!tok.markUsedAtStart && req.method === 'GET') tok.uses += 1;
  } finally {
    tok.active -= 1;
  }
}

async function handleCase(req, res, url, name) {
  const origin = baseUrl(req);
  const opts = queryOpts(url);
  if (name === 'direct') { logRequest(req, { status: 'stream', note: 'direct ranged file' }); return serveVirtualFile(req, res, { ...opts, rangeMode: 'range', filename: opts.filename || 'direct-ranged.bin' }); }
  if (name === 'no-range') { logRequest(req, { status: 'stream', note: 'no range support' }); return serveVirtualFile(req, res, { ...opts, rangeMode: 'no-range', filename: 'no-range.bin' }); }
  if (name === 'lie-range') { logRequest(req, { status: 'stream', note: 'lies about ranges' }); return serveVirtualFile(req, res, { ...opts, rangeMode: 'lie-range', filename: 'lie-range.bin' }); }
  if (name === 'chunked') { logRequest(req, { status: 'stream', note: 'chunked no content-length' }); return serveVirtualFile(req, res, { ...opts, rangeMode: 'chunked', filename: 'chunked.bin' }); }
  if (name === 'original-302-oneshot') { const tok = makeToken('oneshot', { ...opts, maxUses: 1, markUsedAtStart: true, allowRangeGroup: true, filename: 'oneshot-final.bin' }); const loc = `${origin}/token/oneshot/${tok.id}`; logRequest(req, { status: 302, token: tok.id, note: 'redirect to one-shot final' }); return redirect(res, 302, loc); }
  if (name === 'original-per-request-token') { const tok = makeToken('per-request', { ...opts, maxUses: 1, markUsedAtStart: true, allowRangeGroup: true, filename: 'fresh-token-per-request.bin' }); const loc = `${origin}/token/per-request/${tok.id}`; logRequest(req, { status: 302, token: tok.id, note: 'fresh final token for every original request' }); return redirect(res, 302, loc); }
  if (name === 'original-cookie-302') { const sid = `lab_session=${tokenId()}`; const tok = makeToken('cookie', { ...opts, maxUses: 1, allowRangeGroup: true, requiredCookie: sid, filename: 'cookie-required.bin' }); const loc = `${origin}/token/cookie/${tok.id}`; logRequest(req, { status: 302, token: tok.id, note: 'sets cookie and redirects to one-shot final' }); return redirect(res, 302, loc, { 'Set-Cookie': `${sid}; Path=/; SameSite=Lax` }); }
  if (name === 'original-referer-302') { const tok = makeToken('referer', { ...opts, maxUses: 3, requiredRefererIncludes: '/case/original-referer-302', filename: 'referer-required.bin' }); const loc = `${origin}/token/referer/${tok.id}`; logRequest(req, { status: 302, token: tok.id, note: 'referer required on final' }); return redirect(res, 302, loc); }
  if (name === 'original-expiring-302') { const tok = makeToken('expiring', { ...opts, maxUses: 3, filename: 'expires-fast.bin' }); const loc = `${origin}/token/expiring/${tok.id}`; logRequest(req, { status: 302, token: tok.id, note: `token expires in ${opts.ttlMs}ms` }); return redirect(res, 302, loc); }
  if (name === 'range-single') { const tok = makeToken('range-single', { ...opts, maxUses: Infinity, markUsedAtStart: false, maxConcurrent: 1, filename: 'one-range-at-a-time.bin' }); const loc = `${origin}/token/range-single/${tok.id}`; logRequest(req, { status: 302, token: tok.id, note: 'only one concurrent range allowed' }); return redirect(res, 302, loc); }
  if (name === '429-then-ok') { const tok = makeToken('retry', { ...opts, maxUses: Infinity, markUsedAtStart: false, filename: 'retry-after.bin' }); const loc = `${origin}/token/retry/${tok.id}`; logRequest(req, { status: 302, token: tok.id, note: `first ${tok.failFirst} attempts return 429` }); return redirect(res, 302, loc); }
  if (name === 'slow') { logRequest(req, { status: 'stream', note: `slow bps=${opts.throttleBps}` }); return serveVirtualFile(req, res, { ...opts, rangeMode: 'range', filename: 'slow.bin' }); }
  if (name === 'fail-midstream') { logRequest(req, { status: 'stream', note: `fail after ${opts.failAfterBytes} bytes` }); return serveVirtualFile(req, res, { ...opts, rangeMode: 'range', filename: 'fail-midstream.bin' }); }
  if (name === 'head-forbidden') { if (req.method === 'HEAD') { logRequest(req, { status: 403, note: 'HEAD forbidden by scenario' }); return sendText(res, 403, '403 Forbidden: HEAD not allowed in this lab scenario\n'); } logRequest(req, { status: 'stream', note: 'GET works even though HEAD forbidden' }); return serveVirtualFile(req, res, { ...opts, rangeMode: 'range', filename: 'head-forbidden-get-ok.bin' }); }
  logRequest(req, { status: 404, note: 'unknown case' });
  return sendText(res, 404, `Unknown case: ${name}\n`);
}

const server = http.createServer(async (req, res) => {
  try {
    const url = new URL(req.url, `http://${req.headers.host}`);
    const path = url.pathname;
    if (path === '/') return dashboard(req, res);
    if (path === '/cases') return sendJson(res, 200, caseLinks(baseUrl(req)));
    if (path === '/logs') return sendJson(res, 200, requests.slice(-250));
    if (path === '/reset') { tokens.clear(); requests.length = 0; requestSeq = 0; return sendText(res, 200, 'Reset tokens and logs.\n'); }
    const caseMatch = path.match(/^\/case\/([a-z0-9-]+)$/);
    if (caseMatch) return await handleCase(req, res, url, caseMatch[1]);
    const tokenMatch = path.match(/^\/token\/([a-z0-9-]+)\/([a-f0-9]+)$/);
    if (tokenMatch) return await handleToken(req, res, tokenMatch[1], tokenMatch[2]);
    logRequest(req, { status: 404, note: 'not found' });
    return sendText(res, 404, 'Not found\n');
  } catch (err) {
    if (isClientAbort(err) || req.aborted || res.destroyed) {
      logRequest(req, { status: 'abort', note: 'client aborted connection' });
      return;
    }
    console.error(err);
    if (!res.headersSent) sendText(res, 500, `Internal lab error: ${err.message}\n`);
    else res.destroy(err);
  }
});

process.on('uncaughtException', err => {
  if (isClientAbort(err)) return;
  console.error(err);
});

process.on('unhandledRejection', err => {
  if (isClientAbort(err)) return;
  console.error(err);
});

server.listen(PORT, HOST, () => {
  console.log(`fhast download lab running at http://${HOST}:${PORT}`);
});
