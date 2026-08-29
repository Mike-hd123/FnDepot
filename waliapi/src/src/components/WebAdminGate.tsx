import { FormEvent, type ReactNode, useEffect, useState } from "react";
import { KeyRound, Loader2, Server } from "lucide-react";
import {
  getWebAdminToken,
  invoke,
  isTauriRuntime,
  setWebAdminToken,
  WEB_UNAUTHORIZED_EVENT,
} from "../lib/runtime";

export function WebAdminGate({ children }: { children: ReactNode }) {
  const [authenticated, setAuthenticated] = useState(isTauriRuntime());
  const [token, setToken] = useState(getWebAdminToken());
  const [checking, setChecking] = useState(!isTauriRuntime() && Boolean(getWebAdminToken()));
  const [error, setError] = useState("");

  const verify = async (candidate: string) => {
    setChecking(true);
    setError("");
    setWebAdminToken(candidate);
    try {
      await invoke("get_server_status");
      const settings = await invoke<{ ui_theme?: string; ui_language?: string }>("get_settings");
      document.documentElement.setAttribute("data-theme", settings.ui_theme || "dark");
      document.documentElement.lang = settings.ui_language || "zh-CN";
      setAuthenticated(true);
    } catch (cause) {
      setWebAdminToken("");
      setAuthenticated(false);
      setError(cause instanceof Error ? cause.message : "管理员令牌验证失败");
    } finally {
      setChecking(false);
    }
  };

  useEffect(() => {
    if (isTauriRuntime()) return;
    const unauthorized = () => {
      setWebAdminToken("");
      setAuthenticated(false);
      setToken("");
      setError("登录已失效，请重新输入管理员令牌");
    };
    window.addEventListener(WEB_UNAUTHORIZED_EVENT, unauthorized);
    if (token) void verify(token);
    return () => window.removeEventListener(WEB_UNAUTHORIZED_EVENT, unauthorized);
    // Only verify the persisted token once on mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if (isTauriRuntime() || authenticated) return children;

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (token.trim()) void verify(token);
  };

  return (
    <main className="flex min-h-screen items-center justify-center bg-[#eef3f8] p-6">
      <section className="surface w-full max-w-md rounded-[28px] p-8">
        <div className="mb-7 flex items-center gap-4">
          <span className="flex h-12 w-12 items-center justify-center rounded-2xl bg-blue-600 text-white shadow-[0_12px_24px_rgba(47,111,237,0.24)]">
            <Server size={23} />
          </span>
          <div>
            <h1 className="text-2xl font-bold tracking-[-0.03em] text-slate-900">WaLiAPI Web</h1>
            <p className="mt-1 text-sm text-slate-500">连接此 Linux 实例的管理后台</p>
          </div>
        </div>

        <form onSubmit={submit} className="space-y-4">
          <label className="block">
            <span className="mb-2 block text-sm font-medium text-slate-700">管理员令牌</span>
            <span className="relative block">
              <KeyRound className="absolute left-3.5 top-1/2 -translate-y-1/2 text-slate-400" size={17} />
              <input
                autoFocus
                type="password"
                autoComplete="current-password"
                value={token}
                onChange={(event) => setToken(event.target.value)}
                placeholder="WALIAPI_ADMIN_TOKEN"
                className="h-12 w-full rounded-2xl border border-slate-200 bg-white pl-11 pr-4 text-sm"
              />
            </span>
          </label>
          {error && <p className="rounded-xl bg-rose-50 px-3 py-2 text-sm text-rose-600">{error}</p>}
          <button
            type="submit"
            disabled={checking || !token.trim()}
            className="action-primary flex h-12 w-full items-center justify-center gap-2 rounded-2xl"
          >
            {checking && <Loader2 size={17} className="animate-spin" />}
            {checking ? "正在验证" : "进入管理后台"}
          </button>
        </form>

        <p className="mt-5 text-xs leading-5 text-slate-400">
          令牌仅保存在当前标签页会话中。生产环境请同时通过 HTTPS 或可信反向代理访问。
        </p>
      </section>
    </main>
  );
}
