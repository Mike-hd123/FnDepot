import { useEffect, useState, useMemo } from "react";
import { apiKeyApi, channelApi, authApi } from "../lib/api";
import type { ApiKey, CreateApiKeyInput, ApiKeyStats, Channel, AuthAccount } from "../types";
import { formatTime } from "../lib/constants";
import { Plus, Key, Trash2, Power, X, Check, Copy, CalendarClock, Database, Activity, Clock, Zap, ChevronDown, Pencil } from "lucide-react";
import { writeClipboard } from "../lib/runtime";

function formatNumber(n: number): string {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + "M";
  if (n >= 1_000) return (n / 1_000).toFixed(1) + "K";
  return String(n);
}

function SuccessRateRing({ rate }: { rate: number }) {
  const r = 28;
  const c = 2 * Math.PI * r;
  const offset = c - (rate / 100) * c;
  const color = rate >= 95 ? "#10b981" : rate >= 80 ? "#f59e0b" : "#ef4444";
  return (
    <div className="relative flex h-16 w-16 items-center justify-center">
      <svg className="h-16 w-16 -rotate-90" viewBox="0 0 70 70">
        <circle cx="35" cy="35" r={r} fill="none" stroke="currentColor" strokeWidth="5" className="text-slate-200/60" />
        <circle
          cx="35" cy="35" r={r} fill="none" stroke={color} strokeWidth="5"
          strokeLinecap="round"
          strokeDasharray={c}
          strokeDashoffset={offset}
          style={{ transition: "stroke-dashoffset 0.6s ease" }}
        />
      </svg>
      <div className="absolute flex flex-col items-center">
        <span className="text-sm font-bold tabular-nums" style={{ color }}>{rate.toFixed(0)}%</span>
      </div>
    </div>
  );
}

function LatencyBar({ latencyMs }: { latencyMs: number }) {
  // 0-500ms = green, 500-2000ms = amber, 2000ms+ = red
  const pct = Math.min(latencyMs / 3000, 1) * 100;
  const color = latencyMs === 0 ? "#94a3b8" : latencyMs < 500 ? "#10b981" : latencyMs < 2000 ? "#f59e0b" : "#ef4444";
  const label = latencyMs > 0 ? `${Math.round(latencyMs)}ms` : "—";
  return (
    <div className="flex items-center gap-2">
      <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-slate-200/60">
        <div
          className="h-full rounded-full"
          style={{ width: `${pct}%`, backgroundColor: color, transition: "width 0.6s ease" }}
        />
      </div>
      <span className="text-sm font-semibold tabular-nums" style={{ color: latencyMs === 0 ? "#94a3b8" : color }}>{label}</span>
    </div>
  );
}

export function ApiKeysPage() {
  const [keys, setKeys] = useState<ApiKey[]>([]);
  const [stats, setStats] = useState<Record<string, ApiKeyStats>>({});
  const [showForm, setShowForm] = useState(false);
  const [editKey, setEditKey] = useState<ApiKey | null>(null);
  const [copied, setCopied] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<ApiKey | null>(null);
  const [deleting, setDeleting] = useState(false);

  const load = async () => {
    const [ks, st] = await Promise.all([
      apiKeyApi.getAll().catch(() => []),
      apiKeyApi.getStats().catch(() => []),
    ]);
    setKeys(ks);
    setStats(Object.fromEntries(st.map(s => [s.api_key_id, s])));
  };

  useEffect(() => { load(); }, []);

  const handleDelete = async () => {
    if (!deleteTarget) return;
    setDeleting(true);
    try {
      await apiKeyApi.delete(deleteTarget.id);
      setDeleteTarget(null);
      load();
    } catch (e) {
      alert(`删除失败: ${e}`);
    } finally {
      setDeleting(false);
    }
  };

  const handleToggle = async (k: ApiKey) => {
    await apiKeyApi.update({ id: k.id, status: k.status === 1 ? 0 : 1 });
    load();
  };

  const copyKey = (key: string) => {
    void writeClipboard(key);
    setCopied(key);
    setTimeout(() => setCopied(null), 2000);
  };

  return (
    <div className="page-shell space-y-6">
      <div className="page-header">
        <div>
          <h1 className="page-title">密钥管理</h1>
          <p className="page-subtitle">为下游应用生成访问凭证，并跟踪配额与有效期</p>
        </div>
        <button onClick={() => setShowForm(true)} className="action-primary">
          <Plus size={16} /> 新建密钥
        </button>
      </div>

      {keys.length === 0 ? (
        <div className="surface empty-state">
          <Key className="h-12 w-12 text-muted-foreground/70" />
          <p className="text-base font-medium">还没有创建任何密钥</p>
          <p className="text-sm text-muted-foreground">创建后即可让客户端通过 OpenAI 兼容协议接入 WaLiAPI</p>
        </div>
      ) : (
        <div className="grid grid-cols-1 gap-4 xl:grid-cols-2">
          {keys.map(k => (
            <div key={k.id} className="surface rounded-[24px] p-5">
              <div className="flex items-start justify-between gap-4">
                <div className="min-w-0 flex-1">
                  <div className="mb-3 flex items-center gap-2">
                    <span className={`h-2.5 w-2.5 rounded-full ${k.status === 1 ? "bg-emerald-400 shadow-[0_0_16px_rgba(52,211,153,0.8)]" : "bg-zinc-500"}`} />
                    <h3 className="text-lg font-semibold tracking-tight">{k.name}</h3>
                  </div>

                  <div className="flex items-center gap-2 rounded-2xl border border-white/8 bg-black/16 px-3 py-3">
                    <code className="min-w-0 flex-1 truncate text-xs font-mono text-foreground/90">{k.key}</code>
                    <button onClick={() => copyKey(k.key)} className="action-secondary px-3 py-2" title="复制">
                      {copied === k.key ? <Check size={14} className="text-emerald-300" /> : <Copy size={14} />}
                    </button>
                  </div>

                  {/* 元信息行 — 紧凑标签式 */}
                  <div className="mt-4 flex flex-wrap items-center gap-2">
                    <span className={`inline-flex items-center gap-1 rounded-full px-2.5 py-1 text-xs font-medium ${k.status === 1 ? "bg-emerald-50 text-emerald-700" : "bg-zinc-100 text-zinc-500"}`}>
                      <span className={`h-1.5 w-1.5 rounded-full ${k.status === 1 ? "bg-emerald-500" : "bg-zinc-400"}`} />
                      {k.status === 1 ? "已启用" : "已禁用"}
                    </span>
                    <span className="inline-flex items-center gap-1 rounded-full bg-slate-100 px-2.5 py-1 text-xs text-slate-600">
                      <Database size={11} />
                      {k.quota_limit > 0 ? `${k.quota_used} / ${k.quota_limit}` : "无限制"}
                    </span>
                    {k.expires_at && (
                      <span className="inline-flex items-center gap-1 rounded-full bg-amber-50 px-2.5 py-1 text-xs text-amber-700">
                        <CalendarClock size={11} />
                        {formatTime(k.expires_at)}
                      </span>
                    )}
                    <span className="inline-flex items-center gap-1 rounded-full bg-slate-100 px-2.5 py-1 text-xs text-slate-400">
                      {formatTime(k.created_at)}
                    </span>
                  </div>

                  {/* 使用量统计 — 仪表盘风格 */}
                  {stats[k.id] && (() => {
                    const s = stats[k.id];
                    const successRate = s.total_calls > 0 ? (s.success_calls / s.total_calls * 100) : 0;
                    return (
                      <div className="mt-4 rounded-2xl border border-slate-200/60 bg-gradient-to-br from-slate-50/80 to-white p-4">
                        <div className="mb-3 flex items-center justify-between">
                          <span className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400">
                            <Activity size={12} /> 使用统计
                          </span>
                          {s.last_call_at && (
                            <span className="text-[11px] text-slate-400">最后调用 {formatTime(s.last_call_at)}</span>
                          )}
                        </div>

                        {/* 主指标行：成功率环 + 三列数据 */}
                        <div className="flex items-center gap-4">
                          {/* 成功率环 */}
                          <div className="flex flex-col items-center gap-1">
                            <SuccessRateRing rate={successRate} />
                            <span className="text-[10px] font-medium text-slate-400">成功率</span>
                          </div>

                          {/* 分隔线 */}
                          <div className="h-14 w-px bg-slate-200/70" />

                          {/* 三列数据 */}
                          <div className="flex-1 grid grid-cols-3 gap-2.5">
                            <div className="flex flex-col gap-0.5">
                              <div className="flex items-center gap-1 text-[11px] text-slate-400">
                                <Activity size={11} /> 调用
                              </div>
                              <div className="text-lg font-bold tabular-nums text-slate-800">
                                {formatNumber(s.total_calls)}
                              </div>
                              <div className="text-[10px] text-slate-400">
                                成功 {formatNumber(s.success_calls)} / 失败 {formatNumber(s.failed_calls)}
                              </div>
                            </div>

                            <div className="flex flex-col gap-0.5">
                              <div className="flex items-center gap-1 text-[11px] text-slate-400">
                                <Zap size={11} /> Token
                              </div>
                              <div className="text-lg font-bold tabular-nums text-slate-800">
                                {formatNumber(s.total_tokens)}
                              </div>
                              <div className="text-[10px] text-slate-400">
                                ↑{formatNumber(s.prompt_tokens)} ↓{formatNumber(s.completion_tokens)}
                              </div>
                            </div>

                            <div className="flex flex-col gap-0.5">
                              <div className="flex items-center gap-1 text-[11px] text-slate-400">
                                <Clock size={11} /> 延迟
                              </div>
                              <div className="mt-1">
                                <LatencyBar latencyMs={s.avg_latency_ms} />
                              </div>
                            </div>
                          </div>
                        </div>
                      </div>
                    );
                  })()}

                  {k.allowed_models.length > 0 && (
                    <div className="mt-4 flex flex-wrap gap-2">
                      {k.allowed_models.map(model => (
                        <span key={model} className="rounded-full bg-primary/12 px-2.5 py-1 text-xs text-primary">{model}</span>
                      ))}
                    </div>
                  )}
                </div>

                <div className="flex flex-col gap-2">
                  <button onClick={() => handleToggle(k)} className="action-secondary px-3 py-2" title={k.status === 1 ? "禁用" : "启用"}>
                    <Power size={16} className={k.status === 1 ? "text-emerald-300" : "text-zinc-400"} />
                  </button>
                  <button onClick={() => setEditKey(k)} className="action-secondary px-3 py-2" title="编辑">
                    <Pencil size={16} />
                  </button>
                  <button onClick={() => setDeleteTarget(k)} className="action-secondary px-3 py-2 text-red-300" title="删除">
                    <Trash2 size={16} />
                  </button>
                </div>
              </div>
            </div>
          ))}
        </div>
      )}

      {showForm && (
        <ApiKeyForm
          onClose={() => setShowForm(false)}
          onSaved={() => { setShowForm(false); load(); }}
        />
      )}

      {editKey && (
        <ApiKeyForm
          editKey={editKey}
          onClose={() => setEditKey(null)}
          onSaved={() => { setEditKey(null); load(); }}
        />
      )}

      <DeleteConfirmDialog
        target={deleteTarget}
        onClose={() => setDeleteTarget(null)}
        onConfirm={handleDelete}
        deleting={deleting}
      />
    </div>
  );
}

function ApiKeyForm({ editKey, onClose, onSaved }: { editKey?: ApiKey; onClose: () => void; onSaved: () => void }) {
  const isEdit = !!editKey;
  const [name, setName] = useState(editKey?.name ?? "");
  const [quotaLimit, setQuotaLimit] = useState(editKey?.quota_limit ?? -1);
  const [channels, setChannels] = useState<Channel[]>([]);
  const [authAccounts, setAuthAccounts] = useState<AuthAccount[]>([]);

  // Rules: each rule = one channel + one model (null means "all")
  type Rule = { channel: string | null; model: string | null };
  // Reconstruct rules from editKey's allowed/denied lists.
  function rulesFromKey(key: ApiKey | undefined): { wl: Rule[]; bl: Rule[] } {
    if (!key) return { wl: [], bl: [] };
    // Each channel → rule with model=null; each model → rule with channel=null.
    const wl: Rule[] = [];
    key.allowed_channels.forEach(ch => wl.push({ channel: ch, model: null }));
    key.allowed_models.forEach(m => wl.push({ channel: null, model: m }));
    const bl: Rule[] = [];
    key.denied_channels.forEach(ch => bl.push({ channel: ch, model: null }));
    key.denied_models.forEach(m => bl.push({ channel: null, model: m }));
    return { wl, bl };
  }
  const initialRules = rulesFromKey(editKey);
  const [whitelistRules, setWhitelistRules] = useState<Rule[]>(initialRules.wl);
  const [blacklistRules, setBlacklistRules] = useState<Rule[]>(initialRules.bl);

  useEffect(() => {
    Promise.all([
      channelApi.getAll().catch(() => []),
      authApi.accountsList().catch(() => []),
    ]).then(([chs, accts]) => {
      setChannels(chs as Channel[]);
      setAuthAccounts(accts as AuthAccount[]);
    });
  }, []);

  const activeChannels = channels.filter(c => c.status === 1);
  const activeAuthAccounts = authAccounts.filter(a => !a.disabled);

  // Unified channel options for dropdown
  const channelOptions = useMemo(() => {
    const opts: { id: string; label: string; group: string; models: string[] }[] = [];
    activeChannels.forEach(c => {
      const models = [
        ...c.models,
        ...(c.model_mapping ? Object.keys(c.model_mapping) : []),
      ];
      opts.push({ id: c.id, label: c.name, group: "API 渠道", models: [...new Set(models)] });
    });
    activeAuthAccounts.forEach(a => {
      const models = [
        ...a.models.filter(m => !m.unavailable).map(m => m.id),
        ...(a.model_mapping ? Object.keys(a.model_mapping) : []),
      ];
      opts.push({ id: a.id, label: a.label, group: "Auth 账号", models: [...new Set(models)] });
    });
    return opts;
  }, [activeChannels, activeAuthAccounts]);

  const channelLabel = (id: string) => channelOptions.find(c => c.id === id)?.label ?? id;
  const channelModels = (id: string) => channelOptions.find(c => c.id === id)?.models ?? [];

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    const trimmedName = name.trim();
    const wlChannels = [...new Set(whitelistRules.filter(r => r.channel).map(r => r.channel!))];
    const wlModels = [...new Set(whitelistRules.filter(r => r.model).map(r => r.model!))];
    const blChannels = [...new Set(blacklistRules.filter(r => r.channel).map(r => r.channel!))];
    const blModels = [...new Set(blacklistRules.filter(r => r.model).map(r => r.model!))];
    if (isEdit && editKey) {
      // Edit mode: call update with all updatable fields.
      // Always pass arrays (even empty) so update clears them.
      await apiKeyApi.update({
        id: editKey.id,
        name: trimmedName,
        quota_limit: quotaLimit,
        allowed_channels: wlChannels,
        allowed_models: wlModels,
        denied_channels: blChannels,
        denied_models: blModels,
      });
    } else {
      // Create mode: only pass non-empty arrays.
      const input: CreateApiKeyInput = {
        name: trimmedName,
        quota_limit: quotaLimit,
        ...(wlChannels.length ? { allowed_channels: wlChannels } : {}),
        ...(wlModels.length ? { allowed_models: wlModels } : {}),
        ...(blChannels.length ? { denied_channels: blChannels } : {}),
        ...(blModels.length ? { denied_models: blModels } : {}),
      };
      await apiKeyApi.create(input);
    }
    onSaved();
  };

  const noRestriction = whitelistRules.length === 0 && blacklistRules.length === 0;

  // --- Multi-select Dropdown ---
  function MultiDropdown({
    selected, onToggle, onClear, options, placeholder, grouped,
  }: {
    selected: string[]; onToggle: (id: string) => void; onClear: () => void;
    options: { id: string; label: string; group?: string }[];
    placeholder: string; grouped?: boolean;
  }) {
    const [open, setOpen] = useState(false);
    const activeCls = "bg-primary/8 text-primary font-semibold";
    const idleCls = "text-foreground hover:bg-muted/60";
    const count = selected.length;
    const displayLabel = count === 0 ? placeholder : ("已选 " + count + " 项");
    return (
      <div className="relative flex-1 min-w-0">
        <button type="button" onClick={() => setOpen(!open)}
          className="flex w-full items-center justify-between rounded-2xl border border-border bg-background/70 px-3 py-2.5 text-sm focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary cursor-pointer">
          <span className={count > 0 ? "text-foreground truncate" : "text-muted-foreground truncate"}>{displayLabel}</span>
          <div className="flex items-center gap-1.5 ml-2 shrink-0">
            {count > 0 && (
              <span onClick={(e) => { e.stopPropagation(); onClear(); }}
                className="rounded-full bg-muted px-1.5 text-[11px] text-muted-foreground hover:bg-muted/80 cursor-pointer">
                {count}×
              </span>
            )}
            <ChevronDown size={14} className="text-muted-foreground" />
          </div>
        </button>
        {open && (
          <>
            <div className="fixed inset-0 z-40" onClick={() => setOpen(false)} />
            <div className="absolute left-0 right-0 top-full z-50 mt-1.5 rounded-2xl border border-border bg-white p-1.5 shadow-xl max-h-[240px] overflow-auto">
              {grouped ? (
                [...new Set(options.map(o => o.group))].map(g => (
                  <div key={g} className="mb-0.5">
                    <div className="px-2 py-1 text-[11px] font-semibold text-muted-foreground/70 uppercase tracking-wide">{g}</div>
                    {options.filter(o => o.group === g).map(o => {
                      const isSelected = selected.includes(o.id);
                      return (
                        <button key={o.id} type="button"
                          onClick={() => onToggle(o.id)}
                          className={"flex w-full items-center justify-between rounded-xl px-3 py-2 text-sm transition-all " + (isSelected ? activeCls : idleCls)}>
                          <span className="truncate">{o.label}</span>
                          {isSelected && <Check size={14} className="shrink-0" />}
                        </button>
                      );
                    })}
                  </div>
                ))
              ) : (
                options.map(o => {
                  const isSelected = selected.includes(o.id);
                  return (
                    <button key={o.id} type="button"
                      onClick={() => onToggle(o.id)}
                      className={"flex w-full items-center justify-between rounded-xl px-3 py-2 text-sm font-mono transition-all " + (isSelected ? activeCls : idleCls)}>
                      <span className="truncate">{o.label}</span>
                      {isSelected && <Check size={14} className="shrink-0" />}
                    </button>
                  );
                })
              )}
            </div>
          </>
        )}
      </div>
    );
  }

  // --- Rule table ---
  function RuleTable({ rules, onRemove, variant }: { rules: Rule[]; onRemove: (i: number) => void; variant: "whitelist" | "blacklist" }) {
    const wl = variant === "whitelist";
    const headCls = wl ? "text-emerald-600" : "text-rose-600";
    const rowCls = wl ? "hover:bg-emerald-50/30" : "hover:bg-rose-50/30";
    const chipCls = wl ? "bg-emerald-100/60 text-emerald-700" : "bg-rose-100/60 text-rose-700";
    const dotCls = wl ? "bg-emerald-500" : "bg-rose-500";
    if (rules.length === 0) return null;
    return (
      <div className="overflow-hidden rounded-xl border border-border/60">
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b border-border/60 bg-muted/30">
              <th className={"px-3 py-2 text-left text-xs font-semibold " + headCls}>渠道</th>
              <th className={"px-3 py-2 text-left text-xs font-semibold " + headCls}>模型</th>
              <th className="w-10"></th>
            </tr>
          </thead>
          <tbody>
            {rules.map((r, i) => (
              <tr key={i} className={"border-b border-border/40 last:border-0 " + rowCls}>
                <td className="px-3 py-2">
                  {r.channel ? (
                    <span className={"inline-flex items-center gap-1.5 rounded-lg " + chipCls + " px-2 py-0.5 text-xs font-medium"}>
                      <span className={"h-1.5 w-1.5 rounded-full " + dotCls} />
                      {channelLabel(r.channel)}
                    </span>
                  ) : (
                    <span className="text-xs text-muted-foreground">全部</span>
                  )}
                </td>
                <td className="px-3 py-2">
                  {r.model ? (
                    <span className="rounded-md border border-border bg-background/60 px-2 py-0.5 text-xs font-mono">{r.model}</span>
                  ) : (
                    <span className="text-xs text-muted-foreground">全部</span>
                  )}
                </td>
                <td className="px-2 py-2 text-right">
                  <button type="button" onClick={() => onRemove(i)}
                    className="rounded-lg p-1 text-muted-foreground/50 hover:text-red-500 hover:bg-red-500/8 transition-colors">
                    <Trash2 size={13} />
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    );
  }

  // --- Section: whitelist / blacklist ---
  function RestrictionSection({
    title, icon, variant, rules, setRules,
  }: {
    title: string; icon: React.ReactNode; variant: "whitelist" | "blacklist";
    rules: Rule[]; setRules: React.Dispatch<React.SetStateAction<Rule[]>>;
  }) {
    const [selChannels, setSelChannels] = useState<string[]>([]);
    const [selModels, setSelModels] = useState<string[]>([]);

    // Models linked to selected channels (union of all selected channels' models)
    const modelOptions = useMemo(() => {
      if (selChannels.length === 0) return [];
      const modelSet = new Set<string>();
      selChannels.forEach(chId => {
        channelModels(chId).forEach(m => modelSet.add(m));
      });
      return [...modelSet].map(m => ({ id: m, label: m }));
    }, [selChannels]);

    const canAdd = selChannels.length > 0 || selModels.length > 0;

    // Dedup helper: check if a rule already exists
    function ruleExists(ch: string | null, model: string | null, list: Rule[]): boolean {
      return list.some(r => r.channel === ch && r.model === model);
    }

    function handleAdd() {
      if (!canAdd) return;
      // Generate all combinations of selected channels x selected models
      const channelsToAdd = selChannels.length > 0 ? selChannels : [null];
      const modelsToAdd = selModels.length > 0 ? selModels : [null];
      const newRules: Rule[] = [];
      for (const ch of channelsToAdd) {
        for (const model of modelsToAdd) {
          if (!ruleExists(ch, model, rules)) {
            newRules.push({ channel: ch, model });
          }
        }
      }
      if (newRules.length > 0) {
        setRules(prev => [...prev, ...newRules]);
      }
      setSelChannels([]);
      setSelModels([]);
    }

    const wl = variant === "whitelist";
    const badgeCls = wl ? "bg-emerald-500" : "bg-rose-500";
    const borderCls = wl ? "border-emerald-200/50" : "border-rose-200/50";
    const bgCls = wl ? "bg-emerald-50/20" : "bg-rose-50/20";
    const textCls = wl ? "text-emerald-600" : "text-rose-600";

    return (
      <div className={"rounded-2xl border " + borderCls + " " + bgCls + " p-4"}>
        {/* Header */}
        <div className="mb-3 flex items-center gap-2">
          <span className={"flex h-6 w-6 items-center justify-center rounded-lg " + badgeCls + " text-white text-xs"}>{icon}</span>
          <span className="text-sm font-semibold">{title}</span>
          {rules.length > 0 && (
            <span className={"rounded-full bg-muted px-2 py-0.5 text-xs font-medium " + textCls}>{rules.length} 条规则</span>
          )}
        </div>

        {/* Add row: channel multi-dropdown + model multi-dropdown + add button */}
        <div className="flex items-center gap-2">
          <MultiDropdown
            selected={selChannels}
            onToggle={(id) => {
              setSelChannels(prev => prev.includes(id) ? prev.filter(x => x !== id) : [...prev, id]);
              setSelModels([]); // reset model selection when channels change
            }}
            onClear={() => { setSelChannels([]); setSelModels([]); }}
            options={channelOptions.map(c => ({ id: c.id, label: c.label, group: c.group }))}
            placeholder="选择渠道"
            grouped
          />
          <MultiDropdown
            selected={selModels}
            onToggle={(id) => setSelModels(prev => prev.includes(id) ? prev.filter(x => x !== id) : [...prev, id])}
            onClear={() => setSelModels([])}
            options={modelOptions}
            placeholder={selChannels.length > 0 ? "选择模型" : "先选渠道"}
          />
          <button type="button" onClick={handleAdd} disabled={!canAdd}
            className="action-primary shrink-0 px-3 py-2.5 text-sm disabled:opacity-40 disabled:cursor-not-allowed">
            <Plus size={15} />
          </button>
        </div>

        {/* Rules table */}
        <div className="mt-3">
          {rules.length > 0 ? (
            <RuleTable rules={rules} onRemove={(i) => setRules(prev => prev.filter((_, idx) => idx !== i))} variant={variant} />
          ) : (
            <p className="text-xs text-muted-foreground/60 py-1">未配置，不限制</p>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm" onClick={onClose}>
      <div className="surface w-full max-w-2xl rounded-[28px] max-h-[90vh] overflow-y-auto" onClick={e => e.stopPropagation()}>
        <div className="flex items-center justify-between border-b border-border px-5 py-4 sticky top-0 surface z-10">
          <h2 className="text-lg font-semibold">{isEdit ? "编辑密钥" : "新建密钥"}</h2>
          <button onClick={onClose} className="action-secondary px-3 py-2"><X size={18} /></button>
        </div>
        <form onSubmit={handleSubmit} className="space-y-5 p-5" onKeyDown={e => { if (e.key === "Enter" && (e.nativeEvent.isComposing || e.keyCode === 229)) e.preventDefault(); }}>
          {/* 名称 + 配额 */}
          <div className="flex gap-4">
            <div className="flex-1">
              <label className="mb-2 block text-sm font-medium">名称</label>
              <input value={name} onChange={e => setName(e.target.value)} className="w-full rounded-2xl border border-border bg-background/70 px-4 py-3 text-sm" placeholder="密钥名称" required />
            </div>
            <div className="w-40">
              <label className="mb-2 block text-sm font-medium">配额 (-1 无限)</label>
              <input type="number" value={quotaLimit} onChange={e => setQuotaLimit(parseInt(e.target.value) || -1)} className="w-full rounded-2xl border border-border bg-background/70 px-4 py-3 text-sm" />
            </div>
          </div>

          {/* 限制配置区 */}
          <div>
            <div className="mb-2 flex items-center gap-2">
              <label className="text-sm font-medium">访问限制</label>
              {noRestriction && (
                <span className="rounded-full bg-amber-100/60 px-2 py-0.5 text-xs text-amber-600">未配置则不限制</span>
              )}
            </div>
            <div className="space-y-3">
              <RestrictionSection
                title="白名单"
                icon={<Check size={12} />}
                variant="whitelist"
                rules={whitelistRules}
                setRules={setWhitelistRules}
              />
              <RestrictionSection
                title="黑名单"
                icon={<X size={12} />}
                variant="blacklist"
                rules={blacklistRules}
                setRules={setBlacklistRules}
              />
            </div>
          </div>

          <div className="flex justify-end gap-2 pt-2">
            <button type="button" onClick={onClose} className="action-secondary">取消</button>
            <button type="submit" className="action-primary"><Check size={16} /> {isEdit ? "保存" : "创建"}</button>
          </div>
        </form>
      </div>
    </div>
  );
}

function DeleteConfirmDialog({
  target,
  onClose,
  onConfirm,
  deleting,
}: {
  target: ApiKey | null;
  onClose: () => void;
  onConfirm: () => void;
  deleting: boolean;
}) {
  if (!target) return null;
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className="surface w-full max-w-sm rounded-[28px] p-6"
        onClick={e => e.stopPropagation()}
      >
        <div className="flex items-center gap-3">
          <div className="rounded-2xl border border-red-200 bg-red-50 p-2.5">
            <Trash2 className="h-5 w-5 text-red-600" />
          </div>
          <div>
            <h3 className="text-base font-semibold">删除密钥</h3>
            <p className="text-sm text-muted-foreground">此操作不可撤销</p>
          </div>
        </div>
        <div className="mt-4 rounded-2xl border border-border bg-background/50 px-4 py-3 text-sm">
          <div className="text-muted-foreground">密钥名称</div>
          <div className="mt-1 font-medium">{target.name}</div>
          <div className="mt-2 text-xs font-mono text-muted-foreground truncate">{target.key}</div>
        </div>
        <div className="mt-5 flex justify-end gap-2">
          <button onClick={onClose} className="action-secondary">取消</button>
          <button
            onClick={onConfirm}
            disabled={deleting}
            className="inline-flex items-center gap-2 rounded-2xl bg-red-600 px-4 py-2.5 text-sm font-medium text-white transition-colors hover:bg-red-700 disabled:opacity-50"
          >
            {deleting ? "删除中..." : "确认删除"}
          </button>
        </div>
      </div>
    </div>
  );
}
