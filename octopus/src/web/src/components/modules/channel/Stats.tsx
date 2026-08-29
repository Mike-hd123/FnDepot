import { Fragment, useEffect, useMemo, useRef, useState } from 'react';
import {
    Activity,
    CheckCircle2,
    Clock,
    Copy,
    DollarSign,
    Download,
    Eye,
    EyeOff,
    FileText,
    MessageSquare,
    Share2,
    X,
} from 'lucide-react';
import { snapdom } from '@zumer/snapdom';
import { toast } from 'sonner';
import { useTranslations } from 'use-intl';
import { type Channel } from '@/api/channel';
import { type StatsMetricsFormatted } from '@/api/stats';
import { formatCount, formatMoney } from '@/lib/utils';

type FormattedMetric = StatsMetricsFormatted['request_count'];

// 模型统计的排序维度
type ModelSortKey = 'cost' | 'count' | 'tokens';

// 成功率百分比, 无请求时按 0 处理
const successRate = (success: number, failed: number) => {
    const total = success + failed;
    return total > 0 ? (success / total) * 100 : 0;
};

// MetricValue 统一渲染数值与单位
function MetricValue({ metric }: { metric: FormattedMetric }) {
    return (
        <span>
            {metric.formatted.value}
            <span className="ml-0.5 text-xs font-normal text-muted-foreground">{metric.formatted.unit}</span>
        </span>
    );
}

// ChannelStats 展示单个渠道的汇总指标与模型排行, 布局随容器宽度自适应, 并可导出为分享图
export function ChannelStats({ channel, stats }: { channel: Channel; stats: StatsMetricsFormatted }) {
    const t = useTranslations('channel.stats');
    const tCommon = useTranslations('common');
    const [modelSort, setModelSort] = useState<ModelSortKey>('cost');
    // 隐藏渠道名称, 分享图也跟随此状态
    const [isNameHidden, setIsNameHidden] = useState(false);
    const [isSharing, setIsSharing] = useState(false);
    // 分享图预览, url 用于展示, blob 用于复制和下载
    const [preview, setPreview] = useState<{ url: string; blob: Blob } | null>(null);
    // 分享截图的源节点
    const contentRef = useRef<HTMLDivElement>(null);

    // 预览关闭或组件卸载时释放临时对象 URL
    useEffect(() => () => {
        if (preview) URL.revokeObjectURL(preview.url);
    }, [preview]);

    // 渠道汇总指标, 次要行承载成功率与输入/输出明细
    const summary: { icon: React.ReactNode; label: string; value: React.ReactNode; sub?: React.ReactNode }[] = [
        {
            icon: <Activity className="size-3.5 text-chart-1" />,
            label: t('totalRequests'),
            value: <MetricValue metric={stats.request_count} />,
            sub: (
                <>
                    <span className="text-accent">{stats.request_success.formatted.value}</span>
                    <span className="text-muted-foreground/40">/</span>
                    <span className="text-destructive">{stats.request_failed.formatted.value}</span>
                    <span className="text-muted-foreground/40">·</span>
                    <span>{successRate(stats.request_success.raw, stats.request_failed.raw).toFixed(1)}%</span>
                </>
            ),
        },
        {
            icon: <FileText className="size-3.5 text-chart-3" />,
            label: t('totalToken'),
            value: <MetricValue metric={stats.total_token} />,
            sub: (
                <>
                    <span>↓ {stats.input_token.formatted.value}{stats.input_token.formatted.unit}</span>
                    <span>↑ {stats.output_token.formatted.value}{stats.output_token.formatted.unit}</span>
                </>
            ),
        },
        {
            icon: <DollarSign className="size-3.5 text-chart-5" />,
            label: t('totalCost'),
            value: <MetricValue metric={stats.total_cost} />,
            sub: (
                <>
                    <span>↓ {stats.input_cost.formatted.value}{stats.input_cost.formatted.unit}</span>
                    <span>↑ {stats.output_cost.formatted.value}{stats.output_cost.formatted.unit}</span>
                </>
            ),
        },
        {
            icon: <Clock className="size-3.5 text-primary" />,
            label: t('avgWaitTime'),
            value: <MetricValue metric={stats.wait_time} />,
        },
    ];

    // 模型级统计: 现场格式化原始计数, 按当前维度排序并计算占比
    const modelStats = useMemo(() => {
        const items = channel.models.map((model) => {
            const count = model.request_success + model.request_failed;
            const tokens = model.input_token + model.output_token;
            const cost = model.input_cost + model.output_cost;
            return {
                id: model.id,
                name: model.name,
                weight: modelSort === 'cost' ? cost : modelSort === 'tokens' ? tokens : count,
                rate: successRate(model.request_success, model.request_failed),
                count: formatCount(count),
                tokens: formatCount(tokens),
                cost: formatMoney(cost),
            };
        });

        const total = items.reduce((sum, item) => sum + item.weight, 0);
        return items
            .sort((a, b) => b.weight - a.weight || a.name.localeCompare(b.name))
            .map((item) => ({ ...item, share: total > 0 ? (item.weight / total) * 100 : 0 }));
    }, [channel.models, modelSort]);

    const sortOptions: { key: ModelSortKey; label: string }[] = [
        { key: 'cost', label: t('sortByCost') },
        { key: 'count', label: t('sortByCount') },
        { key: 'tokens', label: t('sortByTokens') },
    ];

    // 把统计区克隆进屏外的固定宽度卡片再截图, 容器查询使分享图不受当前屏幕宽度影响
    const handleShare = async () => {
        if (!contentRef.current) return;

        const stage = document.createElement('div');
        stage.className = 'rounded-3xl bg-card px-4 py-2 text-card-foreground';
        // 宽度与桌面端弹窗卡片一致, 定位到屏外避免闪动
        Object.assign(stage.style, { position: 'fixed', top: '0', left: '-10000px', width: '768px' });
        stage.appendChild(contentRef.current.cloneNode(true));
        document.body.appendChild(stage);

        setIsSharing(true);
        try {
            const blob = await snapdom.toBlob(stage, {
                type: 'png',
                scale: 2,
                embedFonts: true,
                exclude: ['[data-share-exclude]'],
                excludeMode: 'remove',
            });
            setPreview({ url: URL.createObjectURL(blob), blob });
        } catch (error) {
            toast.error(error instanceof Error ? error.message : String(error));
        } finally {
            stage.remove();
            setIsSharing(false);
        }
    };

    const handleCopyImage = async () => {
        if (!preview) return;
        try {
            await navigator.clipboard.write([new ClipboardItem({ 'image/png': preview.blob })]);
            toast.success(tCommon('copy.success'));
        } catch {
            toast.error(tCommon('copy.failed'));
        }
    };

    const handleDownloadImage = () => {
        if (!preview) return;
        const link = document.createElement('a');
        link.href = preview.url;
        link.download = `channel-${channel.id}.png`;
        link.click();
    };

    const iconButtonClass = 'flex size-6 items-center justify-center rounded-md text-muted-foreground/60 transition-colors hover:text-foreground disabled:opacity-50';
    const previewActionClass = 'flex size-10 items-center justify-center rounded-xl border border-border bg-card text-muted-foreground transition-colors hover:text-foreground active:scale-95';

    return (
        <div ref={contentRef} className="@container/stats cursor-default">
            <div className="grid gap-4 pb-2 @2xl/stats:grid-cols-[minmax(0,260px)_minmax(0,1fr)]">
                {/* 左列: 渠道汇总 */}
                <section className="flex flex-col gap-2">
                    <div className="flex h-7 items-center gap-1">
                        <h3 className={`truncate text-xs font-semibold tracking-wider text-muted-foreground ${isNameHidden ? 'select-none blur-[3px]' : ''}`}>
                            {channel.name}
                        </h3>
                        <div data-share-exclude className="flex shrink-0 items-center">
                            <button
                                type="button"
                                onClick={() => setIsNameHidden(!isNameHidden)}
                                aria-pressed={isNameHidden}
                                className={iconButtonClass}
                            >
                                {isNameHidden ? <EyeOff className="size-3.5" /> : <Eye className="size-3.5" />}
                            </button>
                            <button type="button" onClick={handleShare} disabled={isSharing} className={iconButtonClass}>
                                <Share2 className="size-3.5" />
                            </button>
                        </div>
                    </div>
                    <dl className="grid grid-cols-2 gap-2 @md/stats:grid-cols-4 @2xl/stats:grid-cols-1 @2xl/stats:flex-1 @2xl/stats:auto-rows-fr">
                        {summary.map(({ icon, label, value, sub }) => (
                            <div key={label} className="flex flex-col rounded-2xl border bg-card p-3">
                                <dt className="flex items-center gap-1.5 text-xs text-muted-foreground">
                                    {icon}
                                    <span className="truncate">{label}</span>
                                </dt>
                                <dd className="mt-auto pt-2 text-right">
                                    <span className="block text-lg font-bold tabular-nums text-card-foreground">{value}</span>
                                    {sub && (
                                        <span className="mt-0.5 flex flex-wrap items-center justify-end gap-x-2 text-[11px] tabular-nums text-muted-foreground">
                                            {sub}
                                        </span>
                                    )}
                                </dd>
                            </div>
                        ))}
                    </dl>
                </section>

                {/* 右列: 模型统计, 仅列出当前维度下的前六名 */}
                <section className="flex flex-col gap-2">
                    <div className="flex h-7 items-center justify-between gap-2">
                        <h4 className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
                            {t('models')}
                            <span className="tabular-nums">({channel.models.length})</span>
                        </h4>
                        <div className="flex shrink-0 items-center text-xs">
                            {sortOptions.map(({ key, label }, index) => (
                                <Fragment key={key}>
                                    {index > 0 && <span aria-hidden="true" className="mx-1 text-muted-foreground/40">/</span>}
                                    <button
                                        type="button"
                                        onClick={() => setModelSort(key)}
                                        aria-pressed={modelSort === key}
                                        className={`transition-colors ${modelSort === key
                                            ? 'font-medium text-foreground'
                                            : 'text-muted-foreground/50 hover:text-muted-foreground'}`}
                                    >
                                        {label}
                                    </button>
                                </Fragment>
                            ))}
                        </div>
                    </div>

                    {modelStats.length === 0 ? (
                        <div className="rounded-2xl border bg-card p-6 text-center text-xs text-muted-foreground">
                            {t('noModels')}
                        </div>
                    ) : (
                        <ul className="space-y-2">
                            {modelStats.slice(0, 6).map((model) => (
                                <li
                                    key={model.id}
                                    className="grid gap-2 rounded-2xl border bg-card p-3 @md/stats:grid-cols-[minmax(0,1fr)_auto] @md/stats:items-center @md/stats:gap-4 @2xl/stats:h-16"
                                >
                                    <div className="min-w-0 space-y-1.5">
                                        <span className="block truncate text-sm font-medium text-card-foreground">{model.name}</span>
                                        <div className="flex items-center gap-2">
                                            <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-muted">
                                                <div className="h-full rounded-full bg-primary" style={{ width: `${model.share}%` }} />
                                            </div>
                                            <span className="w-8 shrink-0 text-right text-[10px] tabular-nums text-muted-foreground">
                                                {model.share.toFixed(0)}%
                                            </span>
                                        </div>
                                    </div>

                                    <div className="grid grid-cols-2 gap-x-3 gap-y-1 text-xs tabular-nums @md/stats:w-64 @md/stats:grid-cols-4 @md/stats:gap-x-2">
                                        <div className="flex items-center gap-1">
                                            <MessageSquare className="size-3.5 shrink-0 text-chart-1" />
                                            <MetricValue metric={model.count} />
                                        </div>
                                        <div className="flex items-center gap-1">
                                            <CheckCircle2 className="size-3.5 shrink-0 text-accent" />
                                            <span>{model.rate.toFixed(0)}%</span>
                                        </div>
                                        <div className="flex items-center gap-1">
                                            <FileText className="size-3.5 shrink-0 text-chart-3" />
                                            <MetricValue metric={model.tokens} />
                                        </div>
                                        <div className="flex items-center gap-1">
                                            <DollarSign className="size-3.5 shrink-0 text-chart-5" />
                                            <MetricValue metric={model.cost} />
                                        </div>
                                    </div>
                                </li>
                            ))}
                        </ul>
                    )}
                </section>
            </div>

            {/* 分享图预览: 覆盖整张弹窗卡片 */}
            {preview && (
                <div className="absolute inset-0 z-30 flex flex-col items-center justify-center gap-3 rounded-3xl bg-card/95 p-4 backdrop-blur-sm">
                    <img
                        src={preview.url}
                        alt=""
                        className="max-h-[70vh] min-h-0 w-auto max-w-full rounded-2xl border border-border object-contain"
                    />
                    <div className="flex shrink-0 items-center gap-2">
                        <button type="button" onClick={handleCopyImage} className={previewActionClass}>
                            <Copy className="size-4" />
                        </button>
                        <button type="button" onClick={handleDownloadImage} className={previewActionClass}>
                            <Download className="size-4" />
                        </button>
                        <button type="button" onClick={() => setPreview(null)} className={previewActionClass}>
                            <X className="size-4" />
                        </button>
                    </div>
                </div>
            )}
        </div>
    );
}
