import {
  AddDownloadMessage,
  AutoGrabEvent,
  NativeResponse,
  NATIVE_HOST,
  VERSION,
  STORAGE_KEYS,
} from "./message_types.js";
import { detectCandidate } from "./candidates.js";

interface CachedRequest {
  url: string;
  headers: Record<string, string>;
  sensitiveHeaders: Record<string, string>;
  responseHeaders: Record<string, string>;
  contentDispositionFilename?: string;
  timestamp: number;
  parentUrl?: string;
  requestMethod?: string;
  originalRequestMethod?: string;
}

const HEADER_CACHE_TTL_MS = 120_000;
const MAX_RECENT_CAPTURES = 20;
const DEDUP_WINDOW_MS = 5_000;

const sensitiveHeaderNames = new Set([
  "cookie",
  "authorization",
  "proxy-authorization",
  "x-api-key",
]);

const headerCache = new Map<string, CachedRequest>();
const redirectMap = new Map<string, string>();

let autoGrabEnabled = true;
let webRequestInstalled = false;
let downloadsObserverInstalled = false;

const recentDownloads = new Map<string, number>();

function isDuplicateDownload(url: string): boolean {
  const now = Date.now();
  const entry = recentDownloads.get(url);
  if (entry && now - entry < DEDUP_WINDOW_MS) {
    return true;
  }
  recentDownloads.set(url, now);
  pruneDownloads();
  return false;
}

function pruneDownloads(): void {
  const now = Date.now();
  for (const [key, time] of recentDownloads.entries()) {
    if (now - time > DEDUP_WINDOW_MS) {
      recentDownloads.delete(key);
    }
  }
}

installAutoGrabListeners();

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create({
    id: "fhast-capture-link",
    title: "Send link to fhast",
    contexts: ["link"],
  });
});

async function initAutoGrabState(): Promise<void> {
  const result = await chrome.storage.local.get(STORAGE_KEYS.AUTO_GRAB_ENABLED);
  const storedState = result[STORAGE_KEYS.AUTO_GRAB_ENABLED];

  if (typeof storedState === "boolean") {
    autoGrabEnabled = storedState;
    return;
  }

  autoGrabEnabled = true;
  await chrome.storage.local.set({
    [STORAGE_KEYS.AUTO_GRAB_ENABLED]: true,
  });
}

chrome.contextMenus.onClicked.addListener((info, tab) => {
  if (info.menuItemId === "fhast-capture-link" && info.linkUrl) {
    const message: AddDownloadMessage = {
      type: "add_download",
      version: VERSION,
      url: info.linkUrl,
      page_url: tab?.url ?? info.pageUrl ?? undefined,
    };
    sendToNativeHost(message);
  }
});

chrome.runtime.onMessage.addListener((request, _sender, sendResponse) => {
  if (request.action === "getAutoGrabStatus") {
    sendResponse({
      enabled: autoGrabEnabled,
      hasDownloads: hasDownloadsPermission(),
      hasWebRequest: hasWebRequestPermission(),
    });
    return true;
  }
  if (request.action === "enableAutoGrab") {
    enableAutoGrab().then((result) => sendResponse(result));
    return true;
  }
  if (request.action === "disableAutoGrab") {
    disableAutoGrab().then(() => sendResponse({ success: true }));
    return true;
  }
  if (request.action === "getRecentCaptures") {
    chrome.storage.local.get(STORAGE_KEYS.RECENT_CAPTURES).then((result) => {
      sendResponse({
        captures: result[STORAGE_KEYS.RECENT_CAPTURES] ?? [],
      });
    });
    return true;
  }
  if (request.action === "clearRecentCaptures") {
    chrome.storage.local
      .remove(STORAGE_KEYS.RECENT_CAPTURES)
      .then(() => sendResponse({ success: true }));
    return true;
  }
});

void initAutoGrabState().catch((error) => {
  console.error("fhast: failed to restore auto-grab state", error);
});

function hasDownloadsPermission(): boolean {
  return chrome.downloads !== undefined;
}

function hasWebRequestPermission(): boolean {
  return chrome.webRequest !== undefined;
}

async function enableAutoGrab(): Promise<{
  success: boolean;
  error?: string;
}> {
  if (!hasDownloadsPermission() || !hasWebRequestPermission()) {
    return {
      success: false,
      error: "Extension permissions are missing. Reload the extension and try again.",
    };
  }

  autoGrabEnabled = true;
  await chrome.storage.local.set({
    [STORAGE_KEYS.AUTO_GRAB_ENABLED]: true,
  });

  return { success: true };
}

async function disableAutoGrab(): Promise<void> {
  autoGrabEnabled = false;
  await chrome.storage.local.set({
    [STORAGE_KEYS.AUTO_GRAB_ENABLED]: false,
  });
  clearAutoGrabSession();
}

function installAutoGrabListeners(): void {
  if (hasWebRequestPermission() && !webRequestInstalled) {
    chrome.webRequest.onBeforeSendHeaders.addListener(
      onBeforeSendHeaders,
      { urls: ["<all_urls>"] },
      ["requestHeaders", "extraHeaders"],
    );
    chrome.webRequest.onBeforeRedirect.addListener(
      onBeforeRedirect,
      { urls: ["<all_urls>"] },
      ["responseHeaders", "extraHeaders"],
    );
    chrome.webRequest.onHeadersReceived.addListener(
      onHeadersReceived,
      { urls: ["<all_urls>"] },
      ["responseHeaders", "extraHeaders"],
    );
    webRequestInstalled = true;
  }

  if (hasDownloadsPermission() && !downloadsObserverInstalled) {
    chrome.downloads.onDeterminingFilename.addListener(onDeterminingFilename);
    chrome.downloads.onCreated.addListener(onDownloadCreated);
    downloadsObserverInstalled = true;
  }
}

function clearAutoGrabSession(): void {
  headerCache.clear();
  redirectMap.clear();
  recentDownloads.clear();
}

function sanitizeSensitiveHeadersForReplay(
  sensitiveHeaders: Record<string, string> | undefined,
  stripCookies: boolean,
): Record<string, string> | undefined {
  if (!sensitiveHeaders) {
    return undefined;
  }

  if (!stripCookies) {
    return sensitiveHeaders;
  }

  const filtered = Object.fromEntries(
    Object.entries(sensitiveHeaders).filter(
      ([name]) => name.toLowerCase() !== "cookie",
    ),
  );

  return Object.keys(filtered).length > 0 ? filtered : undefined;
}

function preserveSyntheticHeaders(
  headers: Record<string, string> | undefined,
): Record<string, string> {
  if (!headers) {
    return {};
  }

  return Object.fromEntries(
    Object.entries(headers).filter(([name]) =>
      name.toLowerCase().startsWith("x-fhast-"),
    ),
  );
}

function isReplayableOriginalMethod(method: string | undefined): boolean {
  if (!method) {
    return true;
  }

  const normalized = method.toUpperCase();
  return normalized === "GET" || normalized === "HEAD";
}

function getHeaderValue(
  headers: Record<string, string> | undefined,
  name: string,
): string | undefined {
  if (!headers) {
    return undefined;
  }

  const match = Object.entries(headers).find(
    ([headerName]) => headerName.toLowerCase() === name.toLowerCase(),
  );

  return match?.[1];
}

function sanitizeHeadersForReplay(
  headers: Record<string, string> | undefined,
  stripReferer: boolean,
): Record<string, string> | undefined {
  if (!headers) {
    return undefined;
  }

  if (!stripReferer) {
    return headers;
  }

  const filtered = Object.fromEntries(
    Object.entries(headers).filter(
      ([name]) => name.toLowerCase() !== "referer",
    ),
  );

  return Object.keys(filtered).length > 0 ? filtered : undefined;
}

function shouldStripRefererForOriginalReplay(
  cached: CachedRequest | null | undefined,
  downloadUrl: string,
  usingRedirectMetadata: boolean,
  shouldUseRedirectedUrl: boolean,
): boolean {
  if (!cached?.parentUrl || !usingRedirectMetadata || shouldUseRedirectedUrl) {
    return false;
  }

  if (cached.parentUrl !== downloadUrl) {
    return false;
  }

  const referer = getHeaderValue(cached.headers, "referer");
  return !referer || !referer.includes(downloadUrl);
}

async function replayRedirectFailure(
  details: chrome.webRequest.WebResponseHeadersDetails,
  cached: CachedRequest,
): Promise<void> {
  if (!cached.parentUrl) {
    return;
  }

  if (details.type !== "main_frame") {
    return;
  }

  if (details.statusCode !== 403 && details.statusCode !== 410) {
    return;
  }

  const referer = getHeaderValue(cached.headers, "referer");
  if (referer?.includes(cached.parentUrl)) {
    return;
  }

  if (isDuplicateDownload(cached.parentUrl)) {
    return;
  }

  const replayHeaders = sanitizeHeadersForReplay(cached.headers, true);
  const replaySensitiveHeaders = sanitizeSensitiveHeadersForReplay(
    cached.sensitiveHeaders,
    true,
  );
  const hasCookies =
    replaySensitiveHeaders !== undefined &&
    Object.keys(replaySensitiveHeaders).some(
      (name) => name.toLowerCase() === "cookie",
    );
  const filename =
    cached.contentDispositionFilename ??
    getHeaderValue(cached.headers, "x-fhast-filename-hint");

  console.warn(
    "fhast: redirected request failed before download, replaying original URL",
    details.statusCode,
    details.url,
    "->",
    cached.parentUrl,
  );

  const message: AddDownloadMessage = {
    type: "add_download",
    version: VERSION,
    url: cached.parentUrl,
    page_url: referer,
    filename_hint: filename,
    headers: replayHeaders,
    sensitive_headers: replaySensitiveHeaders,
    response_headers: cached.responseHeaders,
  };

  const grabEvent: AutoGrabEvent = {
    url: cached.parentUrl,
    filename,
    pageUrl: referer,
    headers: replayHeaders,
    sensitiveHeaders: replaySensitiveHeaders,
    responseHeaders: cached.responseHeaders,
    hasCookies,
    parentUrl: cached.parentUrl,
    capturedAt: new Date().toISOString(),
  };

  await storeCapture(grabEvent);
  sendToNativeHost(message);
}

function onBeforeSendHeaders(
  details: chrome.webRequest.WebRequestHeadersDetails,
): void {
  if (!autoGrabEnabled) return;

  const existing = headerCache.get(details.url);
  const parsed = parseHeaders(details.requestHeaders ?? []);
  const preservedHeaders = preserveSyntheticHeaders(existing?.headers);

  const cacheEntry: CachedRequest = {
    url: details.url,
    headers: { ...preservedHeaders, ...parsed.normal },
    sensitiveHeaders: parsed.sensitive,
    responseHeaders: existing?.responseHeaders ?? {},
    contentDispositionFilename: existing?.contentDispositionFilename,
    timestamp: Date.now(),
    parentUrl: existing?.parentUrl,
    requestMethod: details.method,
    originalRequestMethod: existing?.originalRequestMethod ?? details.method,
  };

  headerCache.set(details.url, cacheEntry);
  pruneCache();
}

function onHeadersReceived(
  details: chrome.webRequest.WebResponseHeadersDetails,
): void {
  if (!autoGrabEnabled) return;

  const responseHeaders: Record<string, string> = {};
  let cdFilename: string | undefined;

  if (details.responseHeaders) {
    for (const h of details.responseHeaders) {
      if (h.name && h.value) {
        responseHeaders[h.name] = h.value;
        const lower = h.name.toLowerCase();
        if (lower === "content-disposition" && !cdFilename) {
          cdFilename = parseContentDispositionFilename(h.value);
        }
      }
    }
  }

  const existing = headerCache.get(details.url);
  const meta: CachedRequest = existing
    ? {
        ...existing,
        responseHeaders: { ...existing.responseHeaders, ...responseHeaders },
        contentDispositionFilename:
          cdFilename ?? existing.contentDispositionFilename,
        timestamp: Date.now(),
      }
    : {
        url: details.url,
        headers: {},
        sensitiveHeaders: {},
        responseHeaders,
        contentDispositionFilename: cdFilename,
        timestamp: Date.now(),
      };

  const candidate = detectCandidate(
    details.url,
    undefined,
    details.responseHeaders,
  );
  if (candidate?.filename) {
    meta.headers["x-fhast-filename-hint"] = candidate.filename;
  }

  headerCache.set(details.url, meta);
  void replayRedirectFailure(details, meta);
}

function parseContentDispositionFilename(header: string): string | undefined {
  const starMatch = header.match(/filename\*=UTF-8''(.+?)(?:;|$)/i);
  if (starMatch) {
    try {
      return decodeURIComponent(starMatch[1]);
    } catch {
      // fall through
    }
  }
  const nameMatch = header.match(/filename=["']?([^"';]+)["']?/i);
  return nameMatch ? nameMatch[1].trim() : undefined;
}

function onBeforeRedirect(
  details: chrome.webRequest.WebRedirectionResponseDetails,
): void {
  if (!autoGrabEnabled) return;

  const source = headerCache.get(details.url);
  if (!source) return;

  const target: CachedRequest = {
    url: details.redirectUrl,
    headers: { ...preserveSyntheticHeaders(source.headers) },
    sensitiveHeaders: {},
    responseHeaders: { ...source.responseHeaders },
    contentDispositionFilename: source.contentDispositionFilename,
    timestamp: Date.now(),
    parentUrl: details.url,
    requestMethod: undefined,
    originalRequestMethod: source.originalRequestMethod,
  };

  headerCache.set(details.redirectUrl, target);
  redirectMap.set(details.url, details.redirectUrl);

  console.log(
    "fhast: redirect tracked",
    new URL(details.url).hostname,
    "\u2192",
    new URL(details.redirectUrl).hostname,
    "+",
    Object.keys(target.headers).length,
    "headers",
    "+",
    Object.keys(target.sensitiveHeaders).length,
    "sensitive",
  );
}

async function onDownloadCreated(
  downloadItem: chrome.downloads.DownloadItem,
): Promise<void> {
  if (!autoGrabEnabled) return;

  chrome.downloads.cancel(downloadItem.id, () => {
    if (chrome.runtime.lastError) {
      console.warn(
        "fhast: could not cancel download:",
        chrome.runtime.lastError.message,
      );
    }
  });

  await handleGrabbedDownload(downloadItem);
}

function onDeterminingFilename(
  downloadItem: chrome.downloads.DownloadItem,
  suggest: (suggestion?: chrome.downloads.DownloadFilenameSuggestion) => void,
): void {
  if (!autoGrabEnabled) {
    suggest({
      filename: downloadItem.filename || "download",
      conflictAction: "uniquify",
    });
    return;
  }

  chrome.downloads.cancel(downloadItem.id, () => {
    if (chrome.runtime.lastError) {
      console.warn(
        "fhast: could not cancel download at determining-filename:",
        chrome.runtime.lastError.message,
      );
    }
  });

  suggest({
    filename: downloadItem.filename || "download",
    conflictAction: "uniquify",
  });

  handleGrabbedDownload(downloadItem);
}

async function handleGrabbedDownload(
  downloadItem: chrome.downloads.DownloadItem,
): Promise<void> {
  try {
    if (isDuplicateDownload(downloadItem.url)) {
      console.log("fhast: skipping duplicate", downloadItem.url.slice(-40));
      return;
    }

    let cached = lookupCachedHeaders(downloadItem.url);
    let redirectedTargetUrl: string | null = null;
    let usingRedirectMetadata = false;
    let originalSourceUrl = downloadItem.url;

    const redirectedTo = redirectMap.get(downloadItem.url);
    if (redirectedTo) {
      const redirected = lookupCachedHeaders(redirectedTo);
      if (redirected) {
        cached = redirected;
        redirectedTargetUrl = redirectedTo;
        usingRedirectMetadata = true;
      }
    } else if (cached?.parentUrl) {
      originalSourceUrl = cached.parentUrl;
      redirectedTargetUrl = downloadItem.url;
      usingRedirectMetadata = true;
    }

    if (!cached && downloadItem.referrer) {
      cached = lookupCachedHeaders(downloadItem.referrer);
    }

    if (cached?.parentUrl) {
      originalSourceUrl = cached.parentUrl;
    }

    const cdFilename = cached?.contentDispositionFilename;
    const candidate = detectCandidate(
      downloadItem.url,
      downloadItem.mime ?? "",
    );

    if (!candidate && !downloadItem.filename && !cdFilename) {
      console.log(
        "fhast: ignoring non-candidate download",
        downloadItem.url.slice(-80),
      );
      return;
    }

    const filename =
      cdFilename ??
      downloadItem.filename?.split("/").pop() ??
      candidate?.filename;
    const sensitiveHeaderCount = Object.keys(cached?.sensitiveHeaders ?? {}).length;
    const shouldForceRedirectedUrl =
      usingRedirectMetadata &&
      redirectedTargetUrl !== null &&
      !isReplayableOriginalMethod(cached?.originalRequestMethod);
    const shouldUseOriginalUrl =
      !shouldForceRedirectedUrl &&
      (usingRedirectMetadata ||
        originalSourceUrl !== downloadItem.url ||
        sensitiveHeaderCount > 0);
    const shouldUseRedirectedUrl =
      redirectedTargetUrl !== null &&
      (shouldForceRedirectedUrl ||
        (!usingRedirectMetadata && !shouldUseOriginalUrl));
    const downloadUrl =
      shouldUseRedirectedUrl && redirectedTargetUrl !== null
        ? redirectedTargetUrl
        : originalSourceUrl;
    const shouldStripReplayCookies =
      usingRedirectMetadata &&
      !shouldUseRedirectedUrl &&
      cached?.parentUrl === downloadUrl;
    const shouldStripReplayReferer = shouldStripRefererForOriginalReplay(
      cached,
      downloadUrl,
      usingRedirectMetadata,
      shouldUseRedirectedUrl,
    );
    const replayHeaders = sanitizeHeadersForReplay(
      cached?.headers,
      shouldStripReplayReferer,
    );

    const replaySensitiveHeaders = sanitizeSensitiveHeadersForReplay(
      cached?.sensitiveHeaders,
      shouldStripReplayCookies,
    );

    const hasCookies =
      replaySensitiveHeaders &&
      Object.keys(replaySensitiveHeaders).some(
        (k) => k.toLowerCase() === "cookie",
      );

    console.log(
      "fhast: grabbed",
      filename ?? downloadUrl.slice(-40),
      "|",
      replayHeaders ? Object.keys(replayHeaders).length : 0,
      "headers |",
      hasCookies ? "WITH cookies" : "NO cookies",
      "|",
      cdFilename ? "CD-filename" : "",
      "|",
      cached?.parentUrl
        ? `redirect from ${new URL(cached.parentUrl).hostname}`
        : "direct",
      "|",
      cached?.originalRequestMethod ?? "GET",
      "origin method |",
      shouldUseRedirectedUrl
        ? "using redirected URL"
        : shouldUseOriginalUrl && usingRedirectMetadata
          ? "using original URL + redirected headers"
          : "using original URL",
      "|",
      sensitiveHeaderCount,
      "sensitive headers",
      shouldStripReplayReferer ? "| stripped replay referer" : "",
      shouldStripReplayCookies ? "| stripped replay cookies" : "",
    );

    const message: AddDownloadMessage = {
      type: "add_download",
      version: VERSION,
      url: downloadUrl,
      page_url: downloadItem.referrer ?? undefined,
      filename_hint: filename,
      headers: replayHeaders,
      sensitive_headers: replaySensitiveHeaders,
      response_headers: cached?.responseHeaders,
    };

    const grabEvent: AutoGrabEvent = {
      url: downloadUrl,
      filename,
      mime: downloadItem.mime ?? undefined,
      fileSize: downloadItem.fileSize ?? undefined,
      pageUrl: downloadItem.referrer ?? undefined,
      headers: replayHeaders,
      sensitiveHeaders: replaySensitiveHeaders,
      responseHeaders: cached?.responseHeaders,
      hasCookies,
      parentUrl: cached?.parentUrl,
      capturedAt: new Date().toISOString(),
    };

    await storeCapture(grabEvent);
    sendToNativeHost(message);

    if (downloadItem.filename || filename) {
      console.log(
        "fhast: auto-grabbed",
        downloadItem.filename ?? filename,
        "from",
        downloadItem.url,
      );
    }
  } catch (err) {
    console.error("fhast auto-grab error:", err);
  }
}

async function storeCapture(event: AutoGrabEvent): Promise<void> {
  const result = await chrome.storage.local.get(STORAGE_KEYS.RECENT_CAPTURES);
  const captures: AutoGrabEvent[] = result[STORAGE_KEYS.RECENT_CAPTURES] ?? [];
  captures.unshift(event);
  if (captures.length > MAX_RECENT_CAPTURES) {
    captures.length = MAX_RECENT_CAPTURES;
  }
  await chrome.storage.local.set({
    [STORAGE_KEYS.RECENT_CAPTURES]: captures,
  });
}

function parseHeaders(headers: chrome.webRequest.HttpHeader[]): {
  normal: Record<string, string>;
  sensitive: Record<string, string>;
} {
  const normal: Record<string, string> = {};
  const sensitive: Record<string, string> = {};

  for (const h of headers) {
    const name = h.name.toLowerCase();
    if (h.value) {
      if (sensitiveHeaderNames.has(name)) {
        sensitive[h.name] = h.value;
      } else {
        normal[h.name] = h.value;
      }
    }
  }

  return { normal, sensitive };
}

function lookupCachedHeaders(url: string): CachedRequest | null {
  const entry = headerCache.get(url);
  if (!entry) return null;

  if (Date.now() - entry.timestamp > HEADER_CACHE_TTL_MS) {
    headerCache.delete(url);
    return null;
  }

  return entry;
}

function pruneCache(): void {
  const now = Date.now();
  for (const [key, entry] of headerCache.entries()) {
    if (now - entry.timestamp > HEADER_CACHE_TTL_MS) {
      headerCache.delete(key);
    }
  }
}

function sendToNativeHost(message: AddDownloadMessage): void {
  chrome.runtime.sendNativeMessage(
    NATIVE_HOST,
    message,
    (response: NativeResponse | undefined) => {
      if (chrome.runtime.lastError) {
        console.error(
          "fhast native host error:",
          chrome.runtime.lastError.message,
        );
        return;
      }
      if (response) {
        if (response.type === "success") {
          console.log("fhast:", response.message);
        } else if (response.type === "error") {
          console.error("fhast error:", response.message);
        }
      }
    },
  );
}
