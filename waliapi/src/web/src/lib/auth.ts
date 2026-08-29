/** Web 管理面板会话管理：localStorage 存 token（API 用），Cookie 由后端 Set-Cookie（SSE 用）。 */

const TOKEN_KEY = "waliapi_admin_token";

export function getToken(): string | null {
  return localStorage.getItem(TOKEN_KEY);
}

export function setToken(token: string) {
  localStorage.setItem(TOKEN_KEY, token);
}

export function clearToken() {
  localStorage.removeItem(TOKEN_KEY);
}

export interface LoginResult {
  token: string;
  username: string;
  must_change_password: boolean;
}

export interface CheckResult {
  username: string;
  must_change_password: boolean;
}

async function parseError(res: Response): Promise<Error> {
  try {
    const data = await res.json();
    if (data && typeof data.error === "string") return new Error(data.error);
  } catch {
    // ignore
  }
  return new Error(`请求失败 (${res.status})`);
}

const CSRF_HEADERS = { "Content-Type": "application/json", "X-Requested-With": "XMLHttpRequest" } as const;

export async function login(username: string, password: string): Promise<LoginResult> {
  const res = await fetch("/admin/api/auth/login", {
    method: "POST",
    headers: { ...CSRF_HEADERS },
    body: JSON.stringify({ username, password }),
  });
  if (!res.ok) throw await parseError(res);
  const data = (await res.json()) as LoginResult;
  setToken(data.token);
  return data;
}

export async function logout() {
  const token = getToken();
  try {
    await fetch("/admin/api/auth/logout", {
      method: "POST",
      headers: token
        ? { Authorization: `Bearer ${token}`, "X-Requested-With": "XMLHttpRequest" }
        : { "X-Requested-With": "XMLHttpRequest" },
    });
  } finally {
    clearToken();
  }
}

export async function checkAuth(): Promise<CheckResult | null> {
  if (!getToken()) return null;
  const res = await fetch("/admin/api/auth/check", {
    headers: { Authorization: `Bearer ${getToken()}` },
  });
  if (res.status === 401) {
    clearToken();
    return null;
  }
  if (!res.ok) throw await parseError(res);
  return (await res.json()) as CheckResult;
}

export async function changePassword(oldPassword: string, newPassword: string): Promise<void> {
  const res = await fetch("/admin/api/auth/change-password", {
    method: "POST",
    headers: {
      ...CSRF_HEADERS,
      Authorization: `Bearer ${getToken() ?? ""}`,
    },
    body: JSON.stringify({ old_password: oldPassword, new_password: newPassword }),
  });
  if (!res.ok) throw await parseError(res);
}
