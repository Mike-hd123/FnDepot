import { invoke as tauriInvoke } from "@tauri-apps/api/core";

const ADMIN_TOKEN_KEY = "waliapi.web.admin-token";
export const WEB_UNAUTHORIZED_EVENT = "waliapi:web-unauthorized";

export interface RuntimeEvent<T> {
  payload: T;
}

export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function getWebAdminToken(): string {
  if (typeof window === "undefined") return "";
  return window.sessionStorage.getItem(ADMIN_TOKEN_KEY) || "";
}

export function setWebAdminToken(token: string): void {
  if (typeof window === "undefined") return;
  const normalized = token.trim();
  if (normalized) window.sessionStorage.setItem(ADMIN_TOKEN_KEY, normalized);
  else window.sessionStorage.removeItem(ADMIN_TOKEN_KEY);
}

/**
 * Unified command transport. Desktop builds keep using Tauri IPC; browser builds
 * send the exact same command name and argument object to the protected Axum
 * admin endpoint.
 */
export async function invoke<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  if (isTauriRuntime()) return tauriInvoke<T>(command, args);

  const token = getWebAdminToken();
  const response = await fetch("/admin/api/invoke", {
    method: "POST",
    credentials: "same-origin",
    headers: {
      "content-type": "application/json",
      "x-requested-with": "XMLHttpRequest",
      ...(token ? { authorization: `Bearer ${token}` } : {}),
    },
    body: JSON.stringify({ cmd: command, args }),
  });

  if (response.status === 401) {
    window.dispatchEvent(new CustomEvent(WEB_UNAUTHORIZED_EVENT));
  }

  const text = await response.text();
  let data: unknown = null;
  if (text) {
    try { data = JSON.parse(text); } catch { /* ignore */ }
  }

  if (!response.ok) {
    const errMsg =
      data && typeof data === "object" && "error" in data
        ? String((data as { error: unknown }).error)
        : `管理 API 请求失败（HTTP ${response.status}）`;
    throw new Error(errMsg);
  }

  return data as T;
}

export async function webFetch<T>(
  path: string,
  init: RequestInit & { json?: unknown } = {},
): Promise<T> {
  const token = getWebAdminToken();
  const headers = new Headers(init.headers);
  if (token) headers.set("authorization", `Bearer ${token}`);
  if (init.json !== undefined) headers.set("content-type", "application/json");
  const response = await fetch(path, {
    ...init,
    credentials: "same-origin",
    headers,
    body: init.json === undefined ? init.body : JSON.stringify(init.json),
  });
  if (response.status === 401) {
    window.dispatchEvent(new CustomEvent(WEB_UNAUTHORIZED_EVENT));
  }
  if (!response.ok) {
    const detail = await response.text().catch(() => "");
    throw new Error(detail || `管理 API 请求失败（HTTP ${response.status}）`);
  }
  if (response.status === 204) return undefined as T;
  return response.json() as Promise<T>;
}

/** Subscribe to native Tauri events or the authenticated headless SSE bridge. */
export async function listen<T>(
  eventName: string,
  handler: (event: RuntimeEvent<T>) => void,
): Promise<() => void> {
  if (isTauriRuntime()) {
    const { listen: tauriListen } = await import("@tauri-apps/api/event");
    return tauriListen<T>(eventName, (event) => handler({ payload: event.payload }));
  }

  const controller = new AbortController();
  const token = getWebAdminToken();
  void (async () => {
    try {
      const response = await fetch("/admin/api/events", {
        headers: token ? { authorization: `Bearer ${token}` } : {},
        credentials: "same-origin",
        signal: controller.signal,
      });
      if (response.status === 401) {
        window.dispatchEvent(new CustomEvent(WEB_UNAUTHORIZED_EVENT));
      }
      if (!response.ok || !response.body) {
        throw new Error(`事件连接失败（HTTP ${response.status}）`);
      }

      const reader = response.body.getReader();
      const decoder = new TextDecoder();
      let buffer = "";
      while (!controller.signal.aborted) {
        const { value, done } = await reader.read();
        if (done) break;
        buffer += decoder.decode(value, { stream: true }).replace(/\r\n/g, "\n");
        let boundary = buffer.indexOf("\n\n");
        while (boundary >= 0) {
          const frame = buffer.slice(0, boundary);
          buffer = buffer.slice(boundary + 2);
          let name = "message";
          const data: string[] = [];
          for (const line of frame.split("\n")) {
            if (line.startsWith("event:")) name = line.slice(6).trim();
            else if (line.startsWith("data:")) data.push(line.slice(5).trimStart());
          }
          if (name === eventName && data.length) {
            try {
              handler({ payload: JSON.parse(data.join("\n")) as T });
            } catch (error) {
              console.warn(`忽略无法解析的事件 ${eventName}`, error);
            }
          }
          boundary = buffer.indexOf("\n\n");
        }
      }
    } catch (error) {
      if (!controller.signal.aborted) console.warn("Web 事件连接已断开", error);
    }
  })();
  return () => controller.abort();
}

export async function openExternalUrl(url: string): Promise<void> {
  if (isTauriRuntime()) {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
    return;
  }
  window.open(url, "_blank", "noopener,noreferrer");
}

export async function writeClipboard(text: string): Promise<void> {
  if (isTauriRuntime()) {
    await tauriInvoke("plugin:clipboard-manager|write_text", { text });
    return;
  }
  if (navigator.clipboard?.writeText && window.isSecureContext) {
    await navigator.clipboard.writeText(text);
    return;
  }
  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.setAttribute("readonly", "");
  textarea.style.position = "fixed";
  textarea.style.opacity = "0";
  document.body.appendChild(textarea);
  textarea.select();
  const copied = document.execCommand("copy");
  textarea.remove();
  if (!copied) throw new Error("浏览器未授予剪贴板权限");
}

export function pickTextFile(accept = ".json"): Promise<{ name: string; content: string } | null> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = accept;
    input.style.display = "none";
    input.addEventListener("change", async () => {
      const file = input.files?.[0];
      input.remove();
      resolve(file ? { name: file.name, content: await file.text() } : null);
    }, { once: true });
    input.addEventListener("cancel", () => { input.remove(); resolve(null); }, { once: true });
    document.body.appendChild(input);
    input.click();
  });
}

export function downloadText(filename: string, content: string, type = "application/json"): void {
  const url = URL.createObjectURL(new Blob([content], { type }));
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  anchor.click();
  URL.revokeObjectURL(url);
}
