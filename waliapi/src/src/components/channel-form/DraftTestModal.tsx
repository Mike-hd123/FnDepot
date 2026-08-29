import { useEffect } from "react";
import { Loader2, X, Check, AlertTriangle, SkipForward } from "lucide-react";
import type { DraftChannelTestResult } from "../../types";
import { ENDPOINT_LABELS, ENDPOINT_TEST_CATEGORY_LABELS } from "../../lib/constants";

// 保存前草稿测试结果弹窗（T07 消费端）。
// phase "running" = 正在逐端点测试；"failed" = 至少一个端点失败/skipped，
// 用户需选择「修改配置」或「仍然保存」。
export function DraftTestModal({
  phase,
  result,
  saving,
  saveError,
  onModify,
  onForceSave,
}: {
  phase: "running" | "failed";
  result: DraftChannelTestResult | null;
  saving: boolean;
  saveError?: string | null;
  onModify: () => void;
  onForceSave: () => void;
}) {
  // Q4：失败/skipped 结果页支持 Escape 返回表单（running 阶段无取消路径）。
  useEffect(() => {
    if (phase !== "failed") return;
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onModify(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [phase, onModify]);

  if (phase === "running") {
    return (
      <div className="fixed inset-0 z-[60] flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm">
        <div className="surface w-full max-w-md rounded-[28px] p-6" onClick={e => e.stopPropagation()}>
          <div className="flex items-center justify-between">
            <h3 className="text-base font-semibold">正在测试渠道端点</h3>
            <span className="rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground">不落库</span>
          </div>
          <div className="mt-4 flex items-center gap-3 rounded-2xl border border-border bg-background/50 px-4 py-4">
            <Loader2 size={20} className="animate-spin text-primary" />
            <div className="text-sm text-muted-foreground">正在逐个端点发送最小推理请求（stream: false）…</div>
          </div>
          <p className="mt-3 text-xs text-muted-foreground">
            测试可能产生极少上游费用。结果仅用于本次保存前验证，不写入生产请求日志或配额。
          </p>
        </div>
      </div>
    );
  }

  const results = result?.results ?? [];

  return (
    <div className="fixed inset-0 z-[60] flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm">
      <div
        className="surface w-full max-w-lg max-h-[85vh] overflow-auto rounded-[28px] p-6"
        onClick={e => e.stopPropagation()}
      >
        <div className="flex items-center justify-between">
          <h3 className="text-base font-semibold">端点测试结果</h3>
          <span className="flex items-center gap-1 rounded-full bg-amber-50 px-2 py-0.5 text-xs font-medium text-amber-700">
            <AlertTriangle size={12} /> 连接未验证
          </span>
        </div>

        <div className="mt-4 space-y-2">
          {results.length === 0 && (
            <div className="rounded-2xl border border-dashed border-border bg-background/40 px-4 py-5 text-center text-sm text-muted-foreground">
              没有可测试的端点
            </div>
          )}
          {results.map((r, i) => (
            <div key={i} className={`rounded-2xl border px-4 py-3 ${
              r.status === "passed"
                ? "border-emerald-200 bg-emerald-50/60"
                : r.status === "failed"
                  ? "border-red-200 bg-red-50/60"
                  : "border-amber-200 bg-amber-50/60"
            }`}>
              <div className="flex items-center gap-2">
                {r.status === "passed" && <Check size={15} className="shrink-0 text-emerald-600" />}
                {r.status === "failed" && <X size={15} className="shrink-0 text-red-600" />}
                {r.status === "skipped" && <SkipForward size={15} className="shrink-0 text-amber-600" />}
                <span className="text-sm font-semibold">
                  {ENDPOINT_LABELS[r.endpoint] ?? r.endpoint}
                </span>
                {r.category && (
                  <span className="rounded-full bg-white/70 px-2 py-0.5 text-[11px] font-medium text-slate-600">
                    {ENDPOINT_TEST_CATEGORY_LABELS[r.category] ?? r.category}
                  </span>
                )}
                <span className="ml-auto text-xs text-slate-500 tabular-nums">
                  {r.latency_ms > 0 ? `${r.latency_ms}ms` : "—"}
                </span>
              </div>
              <p className="mt-1 text-xs leading-5 text-slate-600">{r.message}</p>
              <div className="mt-1.5 flex flex-wrap items-center gap-2 text-[11px] text-slate-500">
                <span className="rounded bg-white/70 px-1.5 py-0.5 font-mono">
                  探测模型：{r.tested_model ?? "未指定"}
                </span>
                <span className="rounded bg-white/70 px-1.5 py-0.5">
                  {r.cost_possible ? "可能产生极少费用" : "无费用"}
                </span>
              </div>
            </div>
          ))}
        </div>

        {results.some(r => r.endpoint === "responses" && r.status === "failed" && r.category === "endpoint_unsupported") && (
          <p className="mt-3 rounded-xl bg-blue-50 px-3 py-2 text-xs text-blue-700">
            该上游不支持或未开通 Responses：建议回表单取消勾选 /responses 后重试；若仍要保留两个端点，请选择「仍然保存」。
          </p>
        )}

        {saveError && (
          <p className="mt-3 rounded-xl border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-700">{saveError}</p>
        )}

        <p className="mt-3 text-xs text-muted-foreground">
          测试可能产生极少上游费用。API Key 不会出现在返回、错误文本或日志中。强制保存会把本次测试结果写入渠道的「最近测试」。
        </p>

        <div className="mt-5 flex justify-end gap-2">
          <button type="button" onClick={onModify} disabled={saving} autoFocus className="action-primary">
            <X size={16} /> 修改配置
          </button>
          <button
            type="button"
            onClick={onForceSave}
            disabled={saving}
            className="inline-flex items-center gap-2 rounded-2xl bg-red-600 px-4 py-2.5 text-sm font-medium text-white transition-colors hover:bg-red-700 disabled:opacity-50"
          >
            {saving ? <Loader2 size={16} className="animate-spin" /> : <AlertTriangle size={16} />}
            仍然保存
          </button>
        </div>
      </div>
    </div>
  );
}
