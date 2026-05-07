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

let autoGrabEnabled = false;
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

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create({
    id: "fhast-capture-link",
    title: "Send link to fhast",
    contexts: ["link"],
  });
  initAutoGrabState();
});

chrome.runtime.onStartup.addListener(() => {
  initAutoGrabState();
});

async function initAutoGrabState(): Promise<void> {
  const result = await chrome.storage.local.get(STORAGE_KEYS.AUTO_GRAB_ENABLED);
  autoGrabEnabled = result[STORAGE_KEYS.AUTO_GRAB_ENABLED] === true;
  if (autoGrabEnabled) {
    installAutoGrabListeners();
  }
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
  const granted = await chrome.permissions.request({
    permissions: ["downloads", "webRequest"],
    origins: ["<all_urls>"],
  });

  if (!granted) {
    return { success: false, error: "Permission denied" };
  }

  autoGrabEnabled = true;
  await chrome.storage.local.set({
    [STORAGE_KEYS.AUTO_GRAB_ENABLED]: true,
  });

  installAutoGrabListeners();
  return { success: true };
}

async function disableAutoGrab(): Promise<void> {
  autoGrabEnabled = false;
  await chrome.storage.local.set({
    [STORAGE_KEYS.AUTO_GRAB_ENABLED]: false,
  });
  removeAutoGrabListeners();
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

function removeAutoGrabListeners(): void {
  if (webRequestInstalled && hasWebRequestPermission()) {
    chrome.webRequest.onBeforeSendHeaders.removeListener(onBeforeSendHeaders);
    chrome.webRequest.onBeforeRedirect.removeListener(onBeforeRedirect);
    chrome.webRequest.onHeadersReceived.removeListener(onHeadersReceived);
    webRequestInstalled = false;
  }

  if (downloadsObserverInstalled && hasDownloadsPermission()) {
    chrome.downloads.onDeterminingFilename.removeListener(
      onDeterminingFilename,
    );
    chrome.downloads.onCreated.removeListener(onDownloadCreated);
    downloadsObserverInstalled = false;
  }

  headerCache.clear();
  redirectMap.clear();
}

function onBeforeSendHeaders(
  details: chrome.webRequest.WebRequestHeadersDetails,
): void {
  const existing = headerCache.get(details.url);
  const parsed = parseHeaders(details.requestHeaders ?? []);

  const mergedSensitive =
    existing?.parentUrl && Object.keys(existing.sensitiveHeaders).length > 0
      ? { ...existing.sensitiveHeaders, ...parsed.sensitive }
      : parsed.sensitive;

  const cacheEntry: CachedRequest = {
    url: details.url,
    headers: { ...(existing?.headers ?? {}), ...parsed.normal },
    sensitiveHeaders: mergedSensitive,
    responseHeaders: existing?.responseHeaders ?? {},
    contentDispositionFilename: existing?.contentDispositionFilename,
    timestamp: Date.now(),
    parentUrl: existing?.parentUrl,
  };

  headerCache.set(details.url, cacheEntry);
  pruneCache();
}

function onHeadersReceived(
  details: chrome.webRequest.WebResponseHeadersDetails,
): void {
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
  const source = headerCache.get(details.url);
  if (!source) return;

  const target: CachedRequest = {
    url: details.redirectUrl,
    headers: { ...source.headers },
    sensitiveHeaders: { ...source.sensitiveHeaders },
    responseHeaders: { ...source.responseHeaders },
    contentDispositionFilename: source.contentDispositionFilename,
    timestamp: Date.now(),
    parentUrl: details.url,
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
    const candidate = detectCandidate(
      downloadItem.url,
      downloadItem.mime ?? "",
    );

    if (!candidate && !downloadItem.filename) {
      return;
    }

    let cached = lookupCachedHeaders(downloadItem.url);
    let downloadUrl = downloadItem.url;

    const redirectedTo = redirectMap.get(downloadItem.url);
    if (redirectedTo) {
      const redirected = lookupCachedHeaders(redirectedTo);
      if (redirected) {
        cached = redirected;
        downloadUrl = redirectedTo;
      }
    }

    if (!cached && downloadItem.referrer) {
      cached = lookupCachedHeaders(downloadItem.referrer);
    }

    const cdFilename = cached?.contentDispositionFilename;
    const filename =
      cdFilename ??
      downloadItem.filename?.split("/").pop() ??
      candidate?.filename;

    const hasCookies =
      cached?.sensitiveHeaders &&
      Object.keys(cached.sensitiveHeaders).some(
        (k) => k.toLowerCase() === "cookie",
      );

    console.log(
      "fhast: grabbed",
      filename ?? downloadUrl.slice(-40),
      "|",
      cached ? Object.keys(cached.headers).length : 0,
      "headers |",
      hasCookies ? "WITH cookies" : "NO cookies",
      "|",
      cdFilename ? "CD-filename" : "",
      "|",
      cached?.parentUrl
        ? `redirect from ${new URL(cached.parentUrl).hostname}`
        : "direct",
      "|",
      downloadUrl !== downloadItem.url ? "using CDN URL" : "using original URL",
    );

    const message: AddDownloadMessage = {
      type: "add_download",
      version: VERSION,
      url: downloadUrl,
      page_url: downloadItem.referrer ?? undefined,
      filename_hint: filename,
      headers: cached?.headers,
      sensitive_headers: cached?.sensitiveHeaders,
      response_headers: cached?.responseHeaders,
    };

    const grabEvent: AutoGrabEvent = {
      url: downloadUrl,
      filename,
      mime: downloadItem.mime ?? undefined,
      fileSize: downloadItem.fileSize ?? undefined,
      pageUrl: downloadItem.referrer ?? undefined,
      headers: cached?.headers,
      sensitiveHeaders: cached?.sensitiveHeaders,
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
