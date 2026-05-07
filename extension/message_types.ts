export interface AddDownloadMessage {
  type: "add_download";
  version: number;
  url: string;
  page_url?: string;
  filename_hint?: string;
  headers?: Record<string, string>;
  sensitive_headers?: Record<string, string>;
  response_headers?: Record<string, string>;
  options?: Record<string, unknown>;
}

export interface AutoGrabEvent {
  url: string;
  filename?: string;
  mime?: string;
  fileSize?: number;
  pageUrl?: string;
  headers?: Record<string, string>;
  sensitiveHeaders?: Record<string, string>;
  responseHeaders?: Record<string, string>;
  hasCookies?: boolean;
  parentUrl?: string;
  capturedAt: string;
}

export type NativeResponse =
  | { type: "success"; version: number; message: string }
  | { type: "error"; version: number; message: string };

export const NATIVE_HOST = "fhast_native_host";
export const VERSION = 1;

export const STORAGE_KEYS = {
  AUTO_GRAB_ENABLED: "autoGrabEnabled",
  RECENT_CAPTURES: "recentCaptures",
} as const;
