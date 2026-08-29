import { useEffect, useState } from "react";
import { Save, X } from "lucide-react";
import type { AuthAccount } from "../../types";
import { MappingSection } from "../MappingSection";
import type { ModelMapping } from "../../hooks/useModelMappings";

export function EditModal({ account, pending, onClose, onSave }: { account: AuthAccount; pending: boolean; onClose: () => void; onSave: (input: Pick<AuthAccount, "id" | "label" | "priority" | "weight" | "model_mapping">) => Promise<void> }) {
  const [label, setLabel] = useState(account.label);
  const [priority, setPriority] = useState(String(account.priority));
  const [weight, setWeight] = useState(String(account.weight));
  const [modelMapping, setModelMapping] = useState<ModelMapping>(account.model_mapping ?? {});
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const listener = (event: KeyboardEvent) => event.key === "Escape" && onClose();
    document.addEventListener("keydown", listener);
    return () => document.removeEventListener("keydown", listener);
  }, [onClose]);

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    const nextPriority = Number(priority);
    const nextWeight = Number(weight);
    if (!label.trim()) return setError("账号名称不能为空");
    if (!Number.isInteger(nextPriority) || nextPriority < 0) return setError("优先级必须是不小于 0 的整数");
    if (!Number.isInteger(nextWeight) || nextWeight < 1) return setError("权重必须是不小于 1 的整数");

    setError(null);
    await onSave({ id: account.id, label: label.trim(), priority: nextPriority, weight: nextWeight, model_mapping: modelMapping });
  };

  // Available target models = auth account's synced models
  const availableTargets = account.models.map(m => m.id);

  return <div className="fixed inset-0 z-50 flex items-center justify-center bg-foreground/35 p-4" role="dialog" aria-modal="true" aria-labelledby="edit-auth-title">
    <form onSubmit={submit} className="surface w-full max-w-md rounded-[24px] p-6 shadow-2xl max-h-[90vh] overflow-y-auto">
      <div className="flex items-start justify-between gap-3"><div><h2 id="edit-auth-title" className="text-lg font-semibold">编辑 Auth 账号</h2><p className="mt-1 text-sm text-muted-foreground">{account.email || account.account_id} · plan: {account.plan_type || "未知"} · 账号级限额</p></div><button type="button" onClick={onClose} aria-label="关闭编辑弹窗" className="rounded-lg p-1 text-muted-foreground hover:bg-muted"><X size={18} /></button></div>
      <div className="mt-5 space-y-4">
        <label className="block text-sm font-medium">账号名称<input value={label} onChange={event => setLabel(event.target.value)} className="mt-1.5 w-full rounded-xl border border-border px-3 py-2.5" autoFocus /></label>
        <div className="grid grid-cols-2 gap-3">
          <div>
            <label className="mb-2 block text-sm font-medium">优先级</label>
            <input value={priority} onChange={event => setPriority(event.target.value)} type="number" min="0" step="1" className="mt-1.5 w-full rounded-xl border border-border px-3 py-2.5" />
            <p className="mt-1.5 text-xs text-muted-foreground">数字越大优先级越高</p>
          </div>
          <div>
            <label className="mb-2 block text-sm font-medium">权重</label>
            <input value={weight} onChange={event => setWeight(event.target.value)} type="number" min="1" step="1" className="mt-1.5 w-full rounded-xl border border-border px-3 py-2.5" />
            <p className="mt-1.5 text-xs text-muted-foreground">同优先级间的负载比例</p>
          </div>
        </div>

        {/* 模型映射 — 共享组件 */}
        <MappingSection
          value={modelMapping}
          availableTargets={availableTargets}
          onChange={setModelMapping}
          hint="左侧填映射名，右侧选账号实际模型"
        />

        {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
      </div>
      <div className="mt-6 flex justify-end gap-2"><button type="button" onClick={onClose} className="action-secondary">取消</button><button disabled={pending} className="action-primary">{pending ? "保存中…" : <><Save size={16} />保存</>}</button></div>
    </form>
  </div>;
}
