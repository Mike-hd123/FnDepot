/**
 * Web 管理面板运行时辅助。
 * 桌面端（Tauri Webview）注入 __TAURI_INTERNALS__，浏览器环境没有，
 * 以此区分两种运行时；桌面端行为完全不变。
 */

export function isWebRuntime(): boolean {
  return typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window);
}

/** 浏览器文件选择器：读取选中文件的文本内容，取消时 resolve null。 */
export function pickFileAsText(accept: string): Promise<string | null> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = accept;
    input.style.display = "none";
    input.onchange = async () => {
      const file = input.files?.[0];
      input.remove();
      if (!file) {
        resolve(null);
        return;
      }
      try {
        resolve(await file.text());
      } catch {
        resolve(null);
      }
    };
    input.oncancel = () => {
      input.remove();
      resolve(null);
    };
    document.body.appendChild(input);
    input.click();
  });
}

/** 浏览器下载：把文本内容保存为本地文件。 */
export function downloadTextFile(content: string, filename: string) {
  const blob = new Blob([content], { type: "application/json;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
}

const ADMIN_TOKEN_KEY = "waliapi_admin_token";

/**
 * Web 管理面板管理接口请求（仅 Web 运行时调用；桌面端不会触发）。
 * 自动附带 Authorization 头，非 2xx 时抛出携带服务端 error 信息的 Error。
 */
export async function webAdminFetch<T>(path: string, options?: {
  method?: string;
  body?: unknown;
}): Promise<T> {
  const token = localStorage.getItem(ADMIN_TOKEN_KEY);
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    "X-Requested-With": "XMLHttpRequest",
  };
  if (token) headers["Authorization"] = `Bearer ${token}`;
  const res = await fetch(`/admin/api${path}`, {
    method: options?.method ?? "GET",
    headers,
    body: options?.body !== undefined ? JSON.stringify(options.body) : undefined,
  });
  const text = await res.text();
  let data: unknown = null;
  if (text) {
    try {
      data = JSON.parse(text);
    } catch {
      data = null;
    }
  }
  if (!res.ok) {
    const message =
      data && typeof data === "object" && "error" in data
        ? String((data as { error: unknown }).error)
        : `请求失败 (${res.status})`;
    throw new Error(message);
  }
  return data as T;
}
