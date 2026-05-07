const FILE_EXTENSIONS = new Set([
  "zip",
  "rar",
  "7z",
  "tar",
  "gz",
  "bz2",
  "xz",
  "iso",
  "dmg",
  "exe",
  "msi",
  "deb",
  "rpm",
  "appimage",
  "mp4",
  "mkv",
  "avi",
  "mov",
  "wmv",
  "flv",
  "webm",
  "m4v",
  "mp3",
  "flac",
  "wav",
  "ogg",
  "m4a",
  "opus",
  "aac",
  "wma",
  "pdf",
  "epub",
  "mobi",
  "djvu",
  "doc",
  "docx",
  "xls",
  "xlsx",
  "ppt",
  "pptx",
  "jpg",
  "jpeg",
  "png",
  "gif",
  "bmp",
  "svg",
  "webp",
  "tiff",
  "psd",
  "ai",
  "sketch",
  "csv",
  "json",
  "xml",
  "yaml",
  "toml",
  "sql",
  "db",
  "sqlite",
  "apk",
  "ipa",
  "torrent",
  "magnet",
  "bin",
  "dat",
  "img",
]);

const CANDIDATE_MIME_TYPES = new Set([
  "application/octet-stream",
  "application/zip",
  "application/x-rar-compressed",
  "application/x-7z-compressed",
  "application/x-tar",
  "application/gzip",
  "application/x-bzip2",
  "application/x-iso9660-image",
  "application/x-msdownload",
  "application/x-deb",
  "application/x-rpm",
  "application/pdf",
  "application/epub+zip",
  "application/x-mobipocket-ebook",
]);

const MEDIA_MIME_PREFIXES = ["video/", "audio/"];

export interface CandidateInfo {
  url: string;
  reason: string;
  filename?: string;
  mime?: string;
}

export function detectFromUrl(url: string): CandidateInfo | null {
  try {
    const parsed = new URL(url);
    const pathname = parsed.pathname;
    const lastDot = pathname.lastIndexOf(".");
    if (lastDot !== -1) {
      const ext = pathname.slice(lastDot + 1).toLowerCase();
      if (FILE_EXTENSIONS.has(ext)) {
        return {
          url,
          reason: `file extension .${ext}`,
          filename: pathname.slice(pathname.lastIndexOf("/") + 1),
        };
      }
    }
    return null;
  } catch {
    return null;
  }
}

export function detectFromContentDisposition(
  url: string,
  headers: chrome.webRequest.HttpHeader[],
): CandidateInfo | null {
  const cd = headers.find(
    (h) => h.name.toLowerCase() === "content-disposition",
  );
  if (!cd?.value) return null;

  const lower = cd.value.toLowerCase();
  if (lower.includes("attachment")) {
    const nameMatch = cd.value.match(/filename[^*]=["']?([^"';\s]+)["']?/i);
    const starMatch = cd.value.match(/filename\*=UTF-8''(.+)/i);
    const filename = starMatch
      ? decodeURIComponent(starMatch[1])
      : nameMatch
        ? nameMatch[1]
        : undefined;

    return {
      url,
      reason: "Content-Disposition: attachment",
      filename,
    };
  }
  return null;
}

export function detectFromMimeType(
  url: string,
  mimeType: string,
): CandidateInfo | null {
  const lower = mimeType.toLowerCase();

  if (CANDIDATE_MIME_TYPES.has(lower)) {
    return { url, reason: `MIME type ${mimeType}`, mime: mimeType };
  }

  for (const prefix of MEDIA_MIME_PREFIXES) {
    if (lower.startsWith(prefix)) {
      return { url, reason: `media type ${mimeType}`, mime: mimeType };
    }
  }

  return null;
}

export function detectCandidate(
  url: string,
  mimeType?: string,
  responseHeaders?: chrome.webRequest.HttpHeader[],
): CandidateInfo | null {
  const fromUrl = detectFromUrl(url);
  if (fromUrl) return fromUrl;

  if (mimeType) {
    const fromMime = detectFromMimeType(url, mimeType);
    if (fromMime) return fromMime;
  }

  if (responseHeaders) {
    const fromCD = detectFromContentDisposition(url, responseHeaders);
    if (fromCD) return fromCD;
  }

  return null;
}
