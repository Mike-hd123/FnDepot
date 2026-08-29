import { useEffect, useMemo, useState } from "react";
import { X, Check } from "lucide-react";
import type { UpstreamModelsResult } from "../../types";

// 同步上游模型弹窗（T14）。交互对齐已验收原型 14-model-sync-prototype.html：
// 搜索过滤 / 全选 / 已有标记 / 计数，勾选后 onApply(selected) 由父组件合并去重。
// 后端 sync_upstream_models 绝不写库，本弹窗也不写库，只负责选择。
export function ModelSyncModal({
  result,
  channelName,
  existingModels,
  onApply,
  onClose,
}: {
  result: UpstreamModelsResult;
  channelName: string;
  existingModels: string[];
  onApply: (selected: string[]) => void;
  onClose: () => void;
}) {
  const existingSet = useMemo(() => new Set(existingModels), [existingModels]);

  // 已有模型默认勾选；新增的不默认勾选。
  const [selected, setSelected] = useState<Set<string>>(
    () => new Set(result.models.filter(m => existingSet.has(m)))
  );
  const [kw, setKw] = useState("");

  // Escape 关闭（与 DraftTestModal 一致）。
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const filtered = useMemo(() => {
    const q = kw.trim().toLowerCase();
    return result.models.filter(m => !q || m.toLowerCase().includes(q));
  }, [kw, result.models]);

  const toAddCount = useMemo(
    () => [...selected].filter(m => !existingSet.has(m)).length,
    [selected, existingSet],
  );

  const allChecked = result.models.length > 0 && selected.size === result.models.length;

  function toggle(m: string, checked: boolean) {
    setSelected(prev => {
      const next = new Set(prev);
      if (checked) next.add(m); else next.delete(m);
      return next;
    });
  }

  function toggleAll(checked: boolean) {
    setSelected(checked ? new Set(result.models) : new Set());
  }

  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className="surface flex max-h-[82vh] w-full max-w-[460px] flex-col overflow-hidden rounded-[20px]"
        onClick={e => e.stopPropagation()}
      >
        {/* 头部 */}
        <div className="flex items-start justify-between gap-3 px-5 pt-5">
          <div>
            <h3 className="text-[15px] font-semibold">同步上游模型 · {channelName}</h3>
            <p className="mt-1 text-xs leading-5 text-muted-foreground">
              {result.protocol} 协议 · 已从上游获取 {result.models.length} 个模型 · 已有的默认勾选
            </p>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label="关闭"
            className="p-1 text-muted-foreground transition-colors hover:text-foreground"
          >
            <X size={18} />
          </button>
        </div>

        {/* 工具栏：搜索 + 全选 */}
        <div className="flex items-center gap-2.5 px-5 pt-4">
          <input
            value={kw}
            onChange={e => setKw(e.target.value)}
            className="min-w-0 flex-1 rounded-[11px] border border-border bg-background/70 px-3 py-2 text-[13px] outline-none transition-colors focus:border-primary"
            placeholder="搜索模型…"
            autoFocus
            autoComplete="off"
          />
          <label className="flex cursor-pointer items-center gap-1.5 whitespace-nowrap text-xs text-muted-foreground select-none">
            <input
              type="checkbox"
              checked={allChecked}
              onChange={e => toggleAll(e.target.checked)}
              className="h-4 w-4 accent-[#2f6fed]"
            />
            全选
          </label>
        </div>

        {/* 计数 */}
        <p className="px-5 pt-2.5 text-xs text-muted-foreground">
          已选 {selected.size} / {result.models.length}，新增 {toAddCount} 个
        </p>

        {/* 列表 */}
        <div className="max-h-[44vh] min-h-[120px] flex-1 overflow-y-auto px-3 py-2">
          {filtered.length === 0 ? (
            <div className="px-4 py-8 text-center text-[13px] text-muted-foreground">
              没有匹配的模型
            </div>
          ) : (
            filtered.map(m => {
              const has = existingSet.has(m);
              const checked = selected.has(m);
              return (
                <label
                  key={m}
                  className="flex cursor-pointer items-center gap-2.5 rounded-[10px] px-2.5 py-2 transition-colors hover:bg-muted"
                >
                  <input
                    type="checkbox"
                    checked={checked}
                    onChange={e => toggle(m, e.target.checked)}
                    className="h-4 w-4 shrink-0 accent-[#2f6fed]"
                  />
                  <span className="min-w-0 break-all font-mono text-[13px]">{m}</span>
                  <span
                    className={`ml-auto shrink-0 rounded-full px-2 py-0.5 text-[11px] ${
                      has ? "bg-muted text-muted-foreground" : "bg-[#e6f7f0] text-[#1f8f5f]"
                    }`}
                  >
                    {has ? "已有" : "新增"}
                  </span>
                </label>
              );
            })
          )}
        </div>

        {/* 底部 */}
        <div className="flex justify-end gap-2.5 border-t border-border px-5 py-4">
          <button type="button" onClick={onClose} className="action-secondary">
            取消
          </button>
          <button
            type="button"
            disabled={toAddCount === 0}
            onClick={() => onApply([...selected])}
            className="inline-flex items-center gap-2 rounded-2xl bg-primary px-4 py-2.5 text-sm font-medium text-white transition-all hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-40"
          >
            <Check size={15} />
            应用到渠道
            {toAddCount > 0 && <span className="font-semibold">+{toAddCount}</span>}
          </button>
        </div>
      </div>
    </div>
  );
}
