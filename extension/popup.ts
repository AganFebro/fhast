import {
  AddDownloadMessage,
  AutoGrabEvent,
  NativeResponse,
  NATIVE_HOST,
  VERSION,
} from "./message_types.js";

interface AutoGrabStatus {
  enabled: boolean;
  hasDownloads: boolean;
  hasWebRequest: boolean;
}

document.addEventListener("DOMContentLoaded", () => {
  const urlInput = document.getElementById("url") as HTMLInputElement;
  const sendUrlBtn = document.getElementById("send-url") as HTMLButtonElement;
  const sendPageBtn = document.getElementById("send-page") as HTMLButtonElement;
  const statusDiv = document.getElementById("status") as HTMLDivElement;
  const autoGrabToggle = document.getElementById(
    "auto-grab-toggle",
  ) as HTMLButtonElement;
  const autoGrabStatus = document.getElementById(
    "auto-grab-status",
  ) as HTMLDivElement;
  const capturesList = document.getElementById(
    "captures-list",
  ) as HTMLDivElement;

  urlInput.focus();

  loadAutoGrabState();

  async function loadAutoGrabState(): Promise<void> {
    const status: AutoGrabStatus = await chrome.runtime.sendMessage({
      action: "getAutoGrabStatus",
    });

    updateAutoGrabUI(status);
    if (status.enabled) {
      loadRecentCaptures();
    }
  }

  function updateAutoGrabUI(status: AutoGrabStatus): void {
    if (status.enabled) {
      autoGrabToggle.textContent = "Disable Auto-Grab";
      autoGrabToggle.className = "btn-danger";
      autoGrabStatus.textContent = "Auto-grab active — watching for downloads";
      autoGrabStatus.className = "auto-grab-active";
    } else {
      autoGrabToggle.textContent = "Enable Auto-Grab";
      autoGrabToggle.className = "btn-toggle";
      autoGrabStatus.textContent =
        "Auto-grab off. Toggle to intercept browser downloads.";
      autoGrabStatus.className = "auto-grab-inactive";
    }
  }

  autoGrabToggle.addEventListener("click", async () => {
    const status: AutoGrabStatus = await chrome.runtime.sendMessage({
      action: "getAutoGrabStatus",
    });

    if (status.enabled) {
      await chrome.runtime.sendMessage({ action: "disableAutoGrab" });
      status.enabled = false;
      capturesList.innerHTML = "";
    } else {
      const result = await chrome.runtime.sendMessage({
        action: "enableAutoGrab",
      });
      status.enabled = result.success;
      if (!result.success && result.error) {
        showStatus(result.error, "error");
      }
    }

    updateAutoGrabUI(status);
    if (status.enabled) {
      loadRecentCaptures();
    }
  });

  async function loadRecentCaptures(): Promise<void> {
    const result = await chrome.runtime.sendMessage({
      action: "getRecentCaptures",
    });
    const captures: AutoGrabEvent[] = result.captures ?? [];
    renderCaptures(captures);
  }

  function renderCaptures(captures: AutoGrabEvent[]): void {
    if (captures.length === 0) {
      capturesList.innerHTML =
        '<div class="capture-empty">No downloads captured yet. Start a download in a tab.</div>';
      return;
    }

    capturesList.innerHTML = captures
      .slice(0, 10)
      .map((c) => {
        const name = escapeHtml(
          c.filename ?? c.url.split("/").pop() ?? c.url.slice(0, 60),
        );
        const size = c.fileSize ? formatBytes(c.fileSize) : (c.mime ?? "");
        const time = new Date(c.capturedAt).toLocaleTimeString();
        const cookieIcon = c.hasCookies ? "🍪" : "";
        const redirectInfo = c.parentUrl
          ? ' <span class="capture-debug">↩ ' +
            escapeHtml(new URL(c.parentUrl).hostname) +
            "</span>"
          : "";
        const headerN = c.headers ? Object.keys(c.headers).length : 0;
        const debug =
          '<span class="capture-debug">' +
          headerN +
          "h" +
          (c.sensitiveHeaders
            ? " +" + Object.keys(c.sensitiveHeaders).length + "s"
            : "") +
          "</span>";

        return (
          '<div class="capture-item">' +
          '<span class="capture-name">' +
          cookieIcon +
          name +
          redirectInfo +
          "</span>" +
          '<span class="capture-meta">' +
          debug +
          " " +
          escapeHtml(size) +
          " · " +
          time +
          "</span>" +
          "</div>"
        );
      })
      .join("");
  }

  sendUrlBtn.addEventListener("click", () => {
    const url = urlInput.value.trim();
    if (!url) {
      showStatus("Enter a URL", "error");
      return;
    }
    sendMessage(url);
  });

  sendPageBtn.addEventListener("click", () => {
    chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => {
      const tab = tabs[0];
      if (tab?.url) {
        sendMessage(tab.url);
      } else {
        showStatus("Could not get current page URL", "error");
      }
    });
  });

  urlInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      sendUrlBtn.click();
    }
  });

  function sendMessage(url: string): void {
    const message: AddDownloadMessage = {
      type: "add_download",
      version: VERSION,
      url,
    };

    chrome.runtime.sendNativeMessage(
      NATIVE_HOST,
      message,
      (response: NativeResponse | undefined) => {
        if (chrome.runtime.lastError) {
          showStatus(
            "Native host error: " + chrome.runtime.lastError.message,
            "error",
          );
          return;
        }
        if (response) {
          if (response.type === "success") {
            showStatus(response.message, "success");
          } else if (response.type === "error") {
            showStatus(response.message, "error");
          }
        }
      },
    );
  }

  function showStatus(message: string, kind: "success" | "error"): void {
    statusDiv.textContent = message;
    statusDiv.className =
      kind === "success" ? "status-success" : "status-error";
    statusDiv.style.display = "block";
    setTimeout(() => {
      statusDiv.style.display = "none";
    }, 4000);
  }
});

function escapeHtml(str: string): string {
  return str
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  const v = bytes / Math.pow(1024, i);
  return v.toFixed(i === 0 ? 0 : 1) + " " + units[i];
}
