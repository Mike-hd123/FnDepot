import { useState, type FormEvent } from "react";
import { useNavigate } from "react-router-dom";
import { Loader2, Lock, User } from "lucide-react";
import { login } from "../lib/auth";

export function LoginPage() {
  const navigate = useNavigate();
  const [username, setUsername] = useState("admin");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (pending) return;
    setPending(true);
    setError(null);
    try {
      const result = await login(username.trim(), password);
      navigate(result.must_change_password ? "/change-password" : "/", { replace: true });
    } catch (err) {
      setError(err instanceof Error ? err.message : "登录失败");
      setPending(false);
    }
  };

  return (
    <div className="flex min-h-screen items-center justify-center bg-[#eef3f8] p-4">
      <div className="surface w-full max-w-sm rounded-[24px] p-8">
        <div className="mb-8 flex flex-col items-center gap-3">
          <div className="flex h-14 w-14 items-center justify-center rounded-2xl bg-white shadow-[0_8px_16px_rgba(47,111,237,0.18)] overflow-hidden">
            <img src="/logo.png" alt="WaLiAPI" className="h-full w-full object-cover" />
          </div>
          <div className="text-center">
            <h1 className="text-xl font-bold tracking-tight text-slate-900">WaLiAPI</h1>
            <p className="mt-1 text-xs text-slate-500">Web 管理面板登录</p>
          </div>
        </div>

        <form onSubmit={submit} className="space-y-4">
          {error && (
            <div role="alert" className="rounded-2xl border border-destructive/25 bg-destructive/10 px-4 py-3 text-sm text-destructive">
              {error}
            </div>
          )}
          <label className="block">
            <span className="mb-1.5 block text-xs font-medium text-slate-500">用户名</span>
            <div className="flex items-center gap-2 rounded-xl border border-slate-200 bg-white px-3 py-2.5 focus-within:border-blue-400 focus-within:ring-2 focus-within:ring-blue-100">
              <User size={15} className="shrink-0 text-slate-400" />
              <input
                type="text"
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                autoComplete="username"
                required
                className="w-full bg-transparent text-sm outline-none"
              />
            </div>
          </label>
          <label className="block">
            <span className="mb-1.5 block text-xs font-medium text-slate-500">密码</span>
            <div className="flex items-center gap-2 rounded-xl border border-slate-200 bg-white px-3 py-2.5 focus-within:border-blue-400 focus-within:ring-2 focus-within:ring-blue-100">
              <Lock size={15} className="shrink-0 text-slate-400" />
              <input
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                autoComplete="current-password"
                required
                className="w-full bg-transparent text-sm outline-none"
              />
            </div>
          </label>
          <button type="submit" disabled={pending} className="action-primary w-full justify-center py-2.5">
            {pending ? <Loader2 size={16} className="animate-spin" /> : null}
            登录
          </button>
        </form>

        <p className="mt-6 text-center text-[11px] leading-5 text-slate-400">
          首次部署请使用容器日志或 /data/INITIAL_PASSWORD 中的临时密码，
          <br />
          登录后系统会强制修改密码。
        </p>
      </div>
    </div>
  );
}
