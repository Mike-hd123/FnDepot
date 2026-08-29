/**
 * `@tauri-apps/api/core` 的 Web 替身。
 * 与 Tauri invoke 语义 1:1 对应：POST /admin/api/invoke { cmd, args }。
 * 401 时清除本地 token 并跳转登录页。
 */
import { clearToken, getToken } from "./auth";

export async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    "X-Requested-With": "XMLHttpRequest",
  };
  const token = getToken();
  if (token) headers["Authorization"] = `Bearer ${token}`;

  let res: Response;
  try {
    res = await fetch("/admin/api/invoke", {
      method: "POST",
      headers,
      body: JSON.stringify({ cmd, args: args ?? {} }),
    });
  } catch {
    throw new Error("网络错误，无法连接服务器");
  }

  if (res.status === 401) {
    clearToken();
    if (!location.pathname.startsWith("/login")) {
      location.assign("/login");
    }
    throw new Error("未登录或会话已过期");
  }

  const text = await res.text();
  let data: unknown = null;
  if (text) {
    try {
      data = JSON.parse(text);
    } catch {
      data = text;
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
