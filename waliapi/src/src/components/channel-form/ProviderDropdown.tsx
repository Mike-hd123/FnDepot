import { useEffect, useId, useMemo, useRef, useState } from "react";
import type { ChannelPreset, ChannelProvider, ChannelRegionGroup } from "../../types";
import { CHANNEL_CATEGORIES, CHANNEL_PROVIDER_ICONS } from "../../lib/constants";

const REGION_ORDER: ChannelRegionGroup[] = ["custom", "international", "domestic", "local"];

export function ProviderDropdown({
  presets,
  current,
  onSelect,
}: {
  presets: ChannelPreset[];
  current: ChannelProvider;
  onSelect: (p: ChannelProvider) => void;
}) {
  const [open, setOpen] = useState(false);
  const [focusIdx, setFocusIdx] = useState(-1);
  const rootRef = useRef<HTMLDivElement>(null);

  // onSelect 经 ref 暴露，避免父组件每次渲染换新闭包时重建 keydown effect。
  const onSelectRef = useRef(onSelect);
  useEffect(() => {
    onSelectRef.current = onSelect;
  }, [onSelect]);

  // 分组与扁平列表都上提到组件体：鼠标 hover 与键盘 Enter 共用同一个全局索引，
  // 避免组内索引（i）与扁平索引（focusIdx）跨组错位。
  const groups = useMemo(
    () =>
      REGION_ORDER.map(region => ({ region, presets: presets.filter(p => p.region === region) }))
        .filter(g => g.presets.length > 0),
    [presets]
  );
  const flat = useMemo(() => groups.flatMap(g => g.presets), [groups]);

  // 扁平列表 id → 索引 映射，避免渲染时逐个 indexOf（O(n²)）。
  const flatIndexById = useMemo(() => {
    const m = new Map<string, number>();
    flat.forEach((p, i) => m.set(p.id, i));
    return m;
  }, [flat]);

  const listboxId = useId();

  const currentPreset = presets.find(p => p.provider === current) ?? presets[0];

  useEffect(() => {
    if (!open) return;
    const onDocClick = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("click", onDocClick);
    return () => document.removeEventListener("click", onDocClick);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (flat.length === 0) return;
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setFocusIdx(i => (i + 1) % flat.length);
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setFocusIdx(i => (i <= 0 ? flat.length - 1 : i - 1));
      } else if (e.key === "Enter") {
        const f = flat[focusIdx];
        if (f) {
          onSelectRef.current(f.provider);
          setOpen(false);
        }
      } else if (e.key === "Escape") {
        setOpen(false);
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
    // flat 由 groups 派生，仅随 groups 变化而变化，故 deps 只列 groups 即可。
  }, [open, focusIdx, groups]);

  // 空 presets 守卫（父组件数据未就绪时）。
  if (!currentPreset) return null;

  return (
    <div ref={rootRef} className="relative">
      <button
        type="button"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={listboxId}
        aria-activedescendant={open ? flat[focusIdx]?.id : undefined}
        onClick={() => { setOpen(o => !o); setFocusIdx(-1); }}
        className={`flex w-full items-center gap-2.5 rounded-2xl border bg-background/70 px-4 py-3 text-left transition-all ${
          open ? "border-primary shadow-[0_0_0_3px_rgba(47,111,237,0.15)]" : "border-border hover:border-primary/40"
        }`}
      >
        <span className="flex h-5 w-5 shrink-0 items-center justify-center">
          <span className="h-5 w-5" dangerouslySetInnerHTML={{ __html: CHANNEL_PROVIDER_ICONS[currentPreset.icon_key] ?? "❓" }} />
        </span>
        <span className="min-w-0 flex-1">
          <span className="block truncate text-sm font-semibold">{currentPreset.display_name}</span>
          <span className="block truncate text-xs text-muted-foreground">{currentPreset.description}</span>
        </span>
        <span className={`shrink-0 text-muted-foreground transition-transform ${open ? "rotate-180" : ""}`}>▾</span>
      </button>

      {open && (
        <div
          id={listboxId}
          role="listbox"
          className="absolute left-0 right-0 top-[calc(100%+6px)] z-50 max-h-80 overflow-y-auto rounded-2xl border border-border bg-white p-1.5 shadow-[0_16px_40px_rgba(15,23,42,0.16)]"
        >
          {groups.map(g => (
            <div key={g.region} className={g.region !== "custom" ? "mt-1 border-t border-border pt-1" : ""}>
              <div className="px-2.5 pb-1 pt-2 text-[11px] font-bold tracking-wider text-muted-foreground">
                {CHANNEL_CATEGORIES[g.region]?.icon} {CHANNEL_CATEGORIES[g.region]?.label}
              </div>
              <div className={`grid gap-1 ${g.region === "custom" ? "grid-cols-1" : "grid-cols-2"}`}>
                {g.presets.map(p => {
                  const flatIdx = flatIndexById.get(p.id) ?? 0;
                  const isCurrent = p.provider === current;
                  const isFocused = flatIdx === focusIdx;
                  return (
                    <button
                      key={p.id}
                      id={p.id}
                      type="button"
                      role="option"
                      aria-selected={isCurrent}
                      title={p.description}
                      onClick={() => { onSelect(p.provider); setOpen(false); }}
                      onMouseEnter={() => setFocusIdx(flatIdx)}
                      className={`flex items-center gap-2.5 rounded-xl px-2.5 py-2 text-left transition-colors ${
                        isFocused
                          ? "bg-muted/70 ring-1 ring-primary/30"
                          : isCurrent
                            ? "bg-primary/10"
                            : "hover:bg-muted/50"
                      }`}
                    >
                      <span className="flex h-[18px] w-[18px] shrink-0 items-center justify-center">
                        <span className="h-[18px] w-[18px]" dangerouslySetInnerHTML={{ __html: CHANNEL_PROVIDER_ICONS[p.icon_key] ?? "❓" }} />
                      </span>
                      <span className="min-w-0 flex-1">
                        <span className={`block truncate text-[13.5px] font-semibold ${isCurrent ? "text-primary" : ""}`}>
                          {p.display_name}
                        </span>
                      </span>
                      <span className="shrink-0 font-bold text-primary">{isCurrent ? "✓" : ""}</span>
                    </button>
                  );
                })}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
