import {
    MorphingDialog,
    MorphingDialogTrigger,
    MorphingDialogContainer,
    MorphingDialogContent,
    MorphingDialogDescription,
} from '@/components/ui/morphing-dialog';
import { CheckCircle2, Check, DollarSign, Layers, MessageSquare, Pencil, Trash2, X, XCircle } from 'lucide-react';
import { type StatsMetricsFormatted } from '@/api/stats';
import { type Channel, useEnableChannel, useDeleteChannel } from '@/api/channel';
import { ChannelStats } from './Stats';
import { ChannelForm } from './Form';
import { useTranslations } from 'use-intl';
import { Tooltip, TooltipTrigger, TooltipContent } from '@/components/ui/tooltip';
import { Switch } from '@/components/ui/switch';
import { toast } from 'sonner';
import { useState } from 'react';

export function Card({ channel, stats, layout = 'grid' }: { channel: Channel; stats: StatsMetricsFormatted; layout?: 'grid' | 'list' }) {
    const t = useTranslations('channel.card');
    const enableChannel = useEnableChannel();
    const deleteChannel = useDeleteChannel();
    // 点编辑图标时弹窗直接展示表单, 点卡片其余区域则展示统计; 卡片的捕获阶段先复位, 再由编辑按钮置位
    const [openInEditing, setOpenInEditing] = useState(false);
    // 删除需二次确认, 确认态下图标常驻不隐藏
    const [isConfirmingDelete, setIsConfirmingDelete] = useState(false);

    const hoverActionClass = 'flex size-8 items-center justify-center rounded-lg transition-all active:scale-95 sm:opacity-0 sm:group-hover:opacity-100 sm:focus-visible:opacity-100';

    // 列表布局的统计项, 缺少 unit 时只显示数值
    const listMetrics: { icon: React.ReactNode; label: string; value: string | number; unit?: string }[] = [
        { icon: <MessageSquare className="size-3.5 text-primary" />, label: t('requestCount'), value: stats.request_count.formatted.value, unit: stats.request_count.formatted.unit },
        { icon: <Layers className="size-3.5 text-primary" />, label: t('model'), value: channel.models.length },
        { icon: <CheckCircle2 className="size-3.5 text-emerald-500" />, label: t('successRequests'), value: stats.request_success.formatted.value },
        { icon: <XCircle className="size-3.5 text-destructive" />, label: t('failedRequests'), value: stats.request_failed.formatted.value },
        { icon: <DollarSign className="size-3.5 text-primary" />, label: t('totalCost'), value: stats.total_cost.formatted.value, unit: stats.total_cost.formatted.unit },
    ];

    // 网格布局的统计项
    const gridMetrics = [
        { icon: <MessageSquare className="h-5 w-5" />, label: t('requestCount'), metric: stats.request_count },
        { icon: <DollarSign className="h-5 w-5" />, label: t('totalCost'), metric: stats.total_cost },
    ];

    const handleEnableChange = (checked: boolean) => {
        enableChannel.mutate(
            { id: channel.id, enabled: checked },
            {
                onSuccess: () => {
                    toast.success(checked ? t('toast.enabled') : t('toast.disabled'));
                },
                onError: (error) => {
                    toast.error(error.message);
                },
            }
        );
    };

    return (
        <MorphingDialog>
            <MorphingDialogTrigger className="w-full">
                <article
                    onClickCapture={() => setOpenInEditing(false)}
                    className="group flex flex-col gap-4 rounded-3xl border border-border bg-card text-card-foreground p-4"
                >
                    <header className="relative flex items-center justify-between gap-2">
                        <Tooltip>
                            <TooltipTrigger asChild>
                                <h3 className="text-lg font-bold truncate min-w-0">{channel.name}</h3>
                            </TooltipTrigger>
                            <TooltipContent key={channel.name} side="top" sideOffset={10} align="center">
                                {channel.name}
                            </TooltipContent>
                        </Tooltip>
                        <div className="flex shrink-0 items-center gap-1">
                            {isConfirmingDelete ? (
                                <>
                                    <button
                                        type="button"
                                        onClick={(e) => {
                                            e.stopPropagation();
                                            setIsConfirmingDelete(false);
                                        }}
                                        title={t('cancel')}
                                        aria-label={t('cancel')}
                                        className="flex size-8 items-center justify-center rounded-lg text-muted-foreground transition-all hover:text-foreground active:scale-95"
                                    >
                                        <X className="size-4" />
                                    </button>
                                    <button
                                        type="button"
                                        onClick={(e) => {
                                            e.stopPropagation();
                                            deleteChannel.mutate(channel.id);
                                        }}
                                        disabled={deleteChannel.isPending}
                                        title={t('confirmDelete')}
                                        aria-label={t('confirmDelete')}
                                        className="flex size-8 items-center justify-center rounded-lg text-destructive transition-all hover:text-destructive/70 active:scale-95 disabled:opacity-50"
                                    >
                                        <Check className="size-4" />
                                    </button>
                                </>
                            ) : (
                                <>
                                    <button
                                        type="button"
                                        onClick={(e) => {
                                            e.stopPropagation();
                                            setIsConfirmingDelete(true);
                                        }}
                                        title={t('delete')}
                                        aria-label={t('delete')}
                                        className={`${hoverActionClass} text-destructive hover:text-destructive/70`}
                                    >
                                        <Trash2 className="size-4" />
                                    </button>
                                    <button
                                        type="button"
                                        onClick={() => setOpenInEditing(true)}
                                        title={t('edit')}
                                        aria-label={t('edit')}
                                        className={`${hoverActionClass} text-muted-foreground hover:text-foreground`}
                                    >
                                        <Pencil className="size-4" />
                                    </button>
                                </>
                            )}
                            <Switch
                                checked={channel.enabled}
                                onCheckedChange={handleEnableChange}
                                disabled={enableChannel.isPending}
                                onClick={(e) => e.stopPropagation()}
                            />
                        </div>
                    </header>

                    {layout === 'list' ? (
                        <dl className="grid grid-cols-2 gap-2 lg:grid-cols-5">
                            {listMetrics.map(({ icon, label, value, unit }) => (
                                <div key={label} className="rounded-2xl border border-border/70 bg-background/80 p-2">
                                    <dt className="mb-1 flex items-center gap-1 text-xs text-muted-foreground">
                                        {icon}
                                        {label}
                                    </dt>
                                    <dd className="text-sm font-semibold">
                                        {value}
                                        {unit && <span className="ml-1 text-xs text-muted-foreground">{unit}</span>}
                                    </dd>
                                </div>
                            ))}
                        </dl>
                    ) : (
                        <dl className="grid grid-cols-1 gap-3">
                            {gridMetrics.map(({ icon, label, metric }) => (
                                <div key={label} className="flex items-center justify-between rounded-2xl border border-border/70 bg-background/80 p-2">
                                    <div className="flex items-center gap-3">
                                        <span className="flex h-10 w-10 items-center justify-center rounded-lg bg-primary/10 text-primary">
                                            {icon}
                                        </span>
                                        <dt className="text-sm text-muted-foreground">{label}</dt>
                                    </div>
                                    <dd className="text-base">
                                        {metric.formatted.value}
                                        <span className="ml-1 text-xs text-muted-foreground">{metric.formatted.unit}</span>
                                    </dd>
                                </div>
                            ))}
                        </dl>
                    )}

                </article>
            </MorphingDialogTrigger>

            <MorphingDialogContainer>
                <MorphingDialogContent className="relative w-full md:max-w-3xl bg-card text-card-foreground px-4 py-2 rounded-3xl max-h-[90vh] overflow-y-auto">
                    <MorphingDialogDescription>
                        {openInEditing
                            ? <ChannelForm channel={channel} />
                            : <ChannelStats channel={channel} stats={stats} />}
                    </MorphingDialogDescription>
                </MorphingDialogContent>
            </MorphingDialogContainer>
        </MorphingDialog>
    );
}
