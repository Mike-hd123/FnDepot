import { useMemo, useState } from "react";
import { X, Plus, Check, ArrowRight, ChevronDown } from "lucide-react";

// 模型映射行：支持重复 from（例如 auto -> model-a, auto -> model-b）。
// 行为与旧版 ChannelForm 完全一致，仅作组件抽取。
export function MappingRow({
  from,
  to,
  availableTargets,
  existingFroms,
  onRemove,
  onChange,
}: {
  from: string;
  to: string;
  availableTargets: string[];
  existingFroms: string[];
  onRemove: () => void;
  onChange: (field: "from" | "to", value: string) => void;
}) {
  const [showFromPicker, setShowFromPicker] = useState(false);
  const [showToPicker, setShowToPicker] = useState(false);
  const [fromInput, setFromInput] = useState("");

  // Target options: currently selected + available
  const targetOptions = useMemo(() => {
    const opts = [...availableTargets];
    if (to && !opts.includes(to)) opts.unshift(to);
    return opts;
  }, [availableTargets, to]);

  return (
    <div className="flex items-center gap-2 rounded-2xl border border-border bg-background/40 px-3 py-2.5">
      {/* Left: mapping model name (what client requests) — input + dropdown */}
      <div className="relative flex-1 min-w-0">
        <input
          value={from}
          onChange={e => onChange("from", e.target.value)}
          onFocus={() => setShowFromPicker(true)}
          placeholder="映射模型名"
          className="w-full rounded-xl border border-border bg-white px-3 py-2.5 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary"
        />
        {showFromPicker && (
          <>
            <div className="fixed inset-0 z-40" onClick={() => setShowFromPicker(false)} />
            <div className="absolute left-0 right-0 top-full z-50 mt-1.5 rounded-2xl border border-border bg-white p-2 shadow-xl max-h-[240px] overflow-auto">
              <div className="px-2 py-1.5 text-[11px] font-semibold text-muted-foreground/70 uppercase tracking-wide">已配置映射名</div>
              {existingFroms.length === 0 && (
                <div className="px-2 py-2 text-sm text-muted-foreground">暂无已配置映射名</div>
              )}
              {existingFroms.map(m => (
                <button
                  key={m}
                  type="button"
                  onClick={() => { onChange("from", m); setShowFromPicker(false); }}
                  className={`flex w-full items-center justify-between rounded-xl px-3 py-2.5 text-sm font-mono transition-all ${
                    from === m
                      ? "bg-primary/8 text-primary font-semibold"
                      : "text-foreground hover:bg-muted/60"
                  }`}
                >
                  {m}
                  {from === m && <Check size={14} />}
                </button>
              ))}
              {/* Add new mapping name */}
              <div className="mt-1 border-t border-border pt-1">
                <div className="flex items-center gap-1 px-1 py-1">
                  <input
                    value={fromInput}
                    onChange={e => setFromInput(e.target.value)}
                    onKeyDown={e => {
                      if (e.key === "Enter" && !e.nativeEvent.isComposing && e.keyCode !== 229) {
                        e.preventDefault();
                        e.stopPropagation();
                        if (fromInput.trim()) {
                          onChange("from", fromInput.trim());
                          setFromInput("");
                          setShowFromPicker(false);
                        }
                      }
                    }}
                    placeholder="新映射名"
                    className="min-w-0 flex-1 rounded-lg border border-border bg-background/60 px-2 py-1.5 text-xs font-mono focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary"
                  />
                  <button
                    type="button"
                    onClick={() => {
                      if (fromInput.trim()) {
                        onChange("from", fromInput.trim());
                        setFromInput("");
                        setShowFromPicker(false);
                      }
                    }}
                    className="shrink-0 rounded-lg bg-primary/10 p-1.5 text-primary hover:bg-primary/20 transition-colors"
                  >
                    <Plus size={14} />
                  </button>
                </div>
              </div>
            </div>
          </>
        )}
      </div>

      {/* Arrow */}
      <div className="flex items-center justify-center shrink-0">
        <ArrowRight size={16} className="text-muted-foreground" />
      </div>

      {/* Right: actual channel model — dropdown */}
      <div className="relative flex-1 min-w-0">
        <button
          type="button"
          onClick={() => setShowToPicker(!showToPicker)}
          className="flex w-full items-center justify-between rounded-xl border border-border bg-white px-3 py-2.5 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/20 focus:border-primary cursor-pointer"
        >
          <span className={to ? "text-foreground truncate" : "text-muted-foreground"}>
            {to || "选择渠道模型"}
          </span>
          <ChevronDown size={14} className="shrink-0 text-muted-foreground" />
        </button>

        {showToPicker && (
          <>
            <div className="fixed inset-0 z-40" onClick={() => setShowToPicker(false)} />
            <div className="absolute left-0 right-0 top-full z-50 mt-1.5 rounded-2xl border border-border bg-white p-2 shadow-xl max-h-[260px] overflow-auto">
              <div className="px-2 py-1.5 text-[11px] font-semibold text-muted-foreground/70 uppercase tracking-wide">渠道模型</div>
              {targetOptions.map(m => (
                <button
                  key={m}
                  type="button"
                  onClick={() => { onChange("to", m); setShowToPicker(false); }}
                  className={`flex w-full items-center justify-between rounded-xl px-3 py-2.5 text-sm font-mono transition-all ${
                    to === m
                      ? "bg-primary/8 text-primary font-semibold"
                      : "text-foreground hover:bg-muted/60"
                  }`}
                >
                  {m}
                  {to === m && <Check size={14} />}
                </button>
              ))}
            </div>
          </>
        )}
      </div>

      {/* Remove button */}
      <button
        type="button"
        onClick={onRemove}
        className="shrink-0 rounded-xl p-2 text-muted-foreground/40 hover:text-red-400 hover:bg-red-500/8 transition-colors"
        title="删除此映射"
      >
        <X size={16} />
      </button>
    </div>
  );
}
