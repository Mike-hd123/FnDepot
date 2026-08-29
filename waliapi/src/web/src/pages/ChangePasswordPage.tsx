import { useState, type FormEvent } from "react";
import { useNavigate } from "react-router-dom";
import { Loader2, KeyRound } from "lucide-react";
import { changePassword } from "../lib/auth";

export function ChangePasswordPage() {
  const navigate = useNavigate();
  const [oldPassword, setOldPassword] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (pending) return;
    if (newPassword.length < 8) {
      setError("新密码长度至少 8 位");
      return;
    }
    if (newPassword !== confirmPassword) {
      setError("两次输入的新密码不一致");
      return;
    }
    setPending(true);
    setError(null);
    try {
      await changePassword(oldPassword, newPassword);
      navigate("/", { replace: true });
    } catch (err) {
      setError(err instanceof Error ? err.message : "修改密码失败");
      setPending(false);
    }
  };

  return (
    <div className="flex min-h-screen items-center justify-center bg-[#eef3f8] p-4">
      <div className="surface w-full max-w-sm rounded-[24px] p-8">
        <div className="mb-6 flex flex-col items-center gap-3">
          <div className="flex h-12 w-12 items-center justify-center rounded-2xl bg-blue-50 text-blue-600">
            <KeyRound size={22} />
          </div>
          <div className="text-center">
            <h1 className="text-lg font-bold tracking-tight text-slate-900">设置新密码</h1>
            <p className="mt-1 text-xs text-slate-500">首次登录必须修改临时密码</p>
          </div>
        </div>

        <form onSubmit={submit} className="space-y-4">
          {error && (
            <div role="alert" className="rounded-2xl border border-destructive/25 bg-destructive/10 px-4 py-3 text-sm text-destructive">
              {error}
            </div>
          )}
          {(
            [
              ["当前密码（临时密码）", oldPassword, setOldPassword, "current-password"],
              ["新密码（至少 8 位）", newPassword, setNewPassword, "new-password"],
              ["确认新密码", confirmPassword, setConfirmPassword, "new-password"],
            ] as const
          ).map(([label, value, setter, autoComplete]) => (
            <label key={label} className="block">
              <span className="mb-1.5 block text-xs font-medium text-slate-500">{label}</span>
              <input
                type="password"
                value={value}
                onChange={(e) => setter(e.target.value)}
                autoComplete={autoComplete}
                required
                className="w-full rounded-xl border border-slate-200 bg-white px-3 py-2.5 text-sm outline-none focus:border-blue-400 focus:ring-2 focus:ring-blue-100"
              />
            </label>
          ))}
          <button type="submit" disabled={pending} className="action-primary w-full justify-center py-2.5">
            {pending ? <Loader2 size={16} className="animate-spin" /> : null}
            保存并进入面板
          </button>
        </form>
      </div>
    </div>
  );
}
