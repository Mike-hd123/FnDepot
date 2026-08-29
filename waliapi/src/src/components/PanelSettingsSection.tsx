import { useEffect, useState, type FormEvent } from "react";
import { Check, KeyRound, Loader2, UserRound } from "lucide-react";
import { webAdminFetch } from "../lib/web";

interface AdminProfile {
  username: string;
  must_change_password: boolean;
}

/**
 * Web 管理面板专属设置：修改登录用户名与密码。
 * 仅在 Web 运行时渲染（由 SettingsPage 按 isWebRuntime() 控制），桌面端不展示。
 */
export function PanelSettingsSection() {
  const [profile, setProfile] = useState<AdminProfile | null>(null);
  const [username, setUsername] = useState("");
  const [usernameMsg, setUsernameMsg] = useState<{ kind: "ok" | "err"; text: string } | null>(null);
  const [usernamePending, setUsernamePending] = useState(false);

  const [oldPassword, setOldPassword] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [passwordMsg, setPasswordMsg] = useState<{ kind: "ok" | "err"; text: string } | null>(null);
  const [passwordPending, setPasswordPending] = useState(false);

  useEffect(() => {
    webAdminFetch<AdminProfile>("/auth/check")
      .then((p) => {
        setProfile(p);
        setUsername(p.username);
      })
      .catch(() => {});
  }, []);

  const inputCls =
    "w-full rounded-2xl border border-border bg-background/70 px-4 py-3 text-sm focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary";

  const submitUsername = async (e: FormEvent) => {
    e.preventDefault();
    if (usernamePending) return;
    setUsernamePending(true);
    setUsernameMsg(null);
    try {
      const res = await webAdminFetch<{ ok: boolean; username: string }>("/auth/change-username", {
        method: "POST",
        body: { new_username: username.trim() },
      });
      setProfile((p) => (p ? { ...p, username: res.username } : p));
      setUsernameMsg({ kind: "ok", text: "用户名已更新。" });
    } catch (err) {
      setUsernameMsg({ kind: "err", text: err instanceof Error ? err.message : "修改用户名失败" });
    }
    setUsernamePending(false);
  };

  const submitPassword = async (e: FormEvent) => {
    e.preventDefault();
    if (passwordPending) return;
    if (newPassword.length < 8) {
      setPasswordMsg({ kind: "err", text: "新密码长度至少 8 位。" });
      return;
    }
    if (newPassword !== confirmPassword) {
      setPasswordMsg({ kind: "err", text: "两次输入的新密码不一致。" });
      return;
    }
    setPasswordPending(true);
    setPasswordMsg(null);
    try {
      await webAdminFetch<{ ok: boolean }>("/auth/change-password", {
        method: "POST",
        body: { old_password: oldPassword, new_password: newPassword },
      });
      setOldPassword("");
      setNewPassword("");
      setConfirmPassword("");
      setPasswordMsg({ kind: "ok", text: "密码已更新，当前会话保持有效。" });
    } catch (err) {
      setPasswordMsg({ kind: "err", text: err instanceof Error ? err.message : "修改密码失败" });
    }
    setPasswordPending(false);
  };

  return (
    <div className="surface rounded-[24px] p-6 space-y-8">
      <div className="flex items-center gap-3">
        <div className="rounded-2xl border border-white/8 bg-white/6 p-3">
          <UserRound size={18} className="text-primary" />
        </div>
        <div>
          <h2 className="text-lg font-semibold">面板设置</h2>
          <p className="text-sm text-muted-foreground">Web 管理面板的登录账号与密码（仅 Web 版可见）</p>
        </div>
      </div>

      {/* 修改用户名 */}
      <form onSubmit={submitUsername} className="space-y-4">
        <h3 className="text-sm font-medium text-muted-foreground">登录用户名</h3>
        {usernameMsg && (
          <div
            role="alert"
            className={`rounded-2xl border px-4 py-3 text-sm ${
              usernameMsg.kind === "ok"
                ? "border-emerald-500/40 bg-emerald-50 text-emerald-700 dark:bg-emerald-500/10 dark:text-emerald-300"
                : "border-destructive/25 bg-destructive/10 text-destructive"
            }`}
          >
            {usernameMsg.text}
          </div>
        )}
        <div className="grid grid-cols-1 gap-3 md:grid-cols-[1fr_auto] md:items-end">
          <label className="block">
            <span className="mb-1.5 block text-xs text-muted-foreground">用户名（2-32 个字符）</span>
            <input
              type="text"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              autoComplete="username"
              minLength={2}
              maxLength={32}
              required
              className={inputCls}
            />
          </label>
          <button
            type="submit"
            disabled={usernamePending || !profile || username.trim() === profile.username}
            className="action-primary justify-center whitespace-nowrap"
          >
            {usernamePending ? <Loader2 size={16} className="animate-spin" /> : <Check size={16} />}
            保存用户名
          </button>
        </div>
      </form>

      <div className="border-t border-border" />

      {/* 修改密码 */}
      <form onSubmit={submitPassword} className="space-y-4">
        <h3 className="flex items-center gap-1.5 text-sm font-medium text-muted-foreground">
          <KeyRound size={14} />
          登录密码
        </h3>
        {passwordMsg && (
          <div
            role="alert"
            className={`rounded-2xl border px-4 py-3 text-sm ${
              passwordMsg.kind === "ok"
                ? "border-emerald-500/40 bg-emerald-50 text-emerald-700 dark:bg-emerald-500/10 dark:text-emerald-300"
                : "border-destructive/25 bg-destructive/10 text-destructive"
            }`}
          >
            {passwordMsg.text}
          </div>
        )}
        <div className="grid grid-cols-1 gap-3 md:grid-cols-3">
          <label className="block">
            <span className="mb-1.5 block text-xs text-muted-foreground">当前密码</span>
            <input
              type="password"
              value={oldPassword}
              onChange={(e) => setOldPassword(e.target.value)}
              autoComplete="current-password"
              required
              className={inputCls}
            />
          </label>
          <label className="block">
            <span className="mb-1.5 block text-xs text-muted-foreground">新密码（至少 8 位）</span>
            <input
              type="password"
              value={newPassword}
              onChange={(e) => setNewPassword(e.target.value)}
              autoComplete="new-password"
              minLength={8}
              required
              className={inputCls}
            />
          </label>
          <label className="block">
            <span className="mb-1.5 block text-xs text-muted-foreground">确认新密码</span>
            <input
              type="password"
              value={confirmPassword}
              onChange={(e) => setConfirmPassword(e.target.value)}
              autoComplete="new-password"
              minLength={8}
              required
              className={inputCls}
            />
          </label>
        </div>
        <div>
          <button type="submit" disabled={passwordPending} className="action-primary justify-center">
            {passwordPending ? <Loader2 size={16} className="animate-spin" /> : <Check size={16} />}
            保存密码
          </button>
        </div>
      </form>
    </div>
  );
}
