import { Fragment, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { useTranslations } from 'use-intl';
import dayjs from 'dayjs';
import { useStatsDaily, type StatsDailyFormatted } from '@/api/stats';

// 热力图上的一天。
interface ActivityDay {
    dateStr: string; // YYYYMMDD, 同时用作格子的 key 和提示的日期来源。
    formatted: StatsDailyFormatted | null; // 当日统计, 无数据为 null。
}

// Activity 以一年的日历热力图展示每日请求量, 悬浮某天时显示当日统计。
export function Activity() {
    const { data: stats, maxRequestCount } = useStatsDaily();
    const t = useTranslations('home.activity');
    const scrollRef = useRef<HTMLDivElement>(null); // 横向滚动容器, 用于贴右和判断两端。
    // 悬浮提示的内容与位置; 关闭时先转为不可见再移除, 以走完淡出过渡。
    const [tooltip, setTooltip] = useState<ActivityDay & { x: number; y: number; visible: boolean } | null>(null);
    const [edges, setEdges] = useState({ atStart: true, atEnd: true }); // 横向滚动是否已抵达两端。

    // 自本周向前推 53 周的周日起逐日铺满 54 列; 今天之后的日期为 null, 只占位不渲染格子。
    // 378 次 dayjs 运算加建表, 只应随统计数据变化重算。
    const days = useMemo<(ActivityDay | null)[]>(() => {
        if (!stats) return [];
        const byDate = new Map(stats.map((stat) => [stat.date, stat]));
        const today = dayjs();
        const start = today.subtract(today.day() + 53 * 7, 'day');

        return Array.from({ length: 54 * 7 }, (_, index) => {
            const date = start.add(index, 'day');
            if (date.isAfter(today, 'day')) return null;
            const dateStr = date.format('YYYYMMDD');
            return { dateStr, formatted: byDate.get(dateStr) ?? null };
        });
    }, [stats]);

    // 记录横向滚动是否已抵达两端, 供遮罩判断淡化哪一侧。
    const checkScroll = () => {
        const node = scrollRef.current;
        if (!node) return;
        setEdges({
            atStart: node.scrollLeft <= 1,
            atEnd: Math.abs(node.scrollWidth - node.clientWidth - node.scrollLeft) <= 1,
        });
    };

    // 默认停在最右侧, 露出最近的日期; 窗口尺寸变化后重新贴右。
    // 赋值 scrollLeft 会触发 scroll 事件, 边缘状态由 onScroll 更新, 此处无需重复计算。
    useLayoutEffect(() => {
        const scrollToRight = () => {
            if (!scrollRef.current) return;
            scrollRef.current.scrollLeft = scrollRef.current.scrollWidth;
        };
        scrollToRight();
        window.addEventListener('resize', scrollToRight);
        return () => window.removeEventListener('resize', scrollToRight);
    }, [days]);

    // 网格与悬浮状态无关: 悬浮会重渲染本组件, 但不应重建这 378 个节点。
    // 格子也不各自挂监听, 由网格统一委托, data-index 即回查 days 的下标。
    const grid = useMemo(() => (
        <div
            className="grid gap-1"
            style={{
                gridTemplateColumns: 'repeat(54, 0.875rem)',
                gridTemplateRows: 'repeat(7, 0.875rem)',
                gridAutoFlow: 'column',
            }}
            onMouseOver={(event) => {
                const target = event.target as HTMLElement;
                const index = target.dataset.index;
                const day = index === undefined ? null : days[Number(index)];
                if (!day) return;
                const rect = target.getBoundingClientRect();
                setTooltip({ ...day, x: rect.left + rect.width / 2, y: rect.top, visible: true });
            }}
        >
            {days.map((day, index) => {
                if (!day) return <div key={`future-${index}`} />;

                const level = maxRequestCount > 0
                    ? Math.min(4, Math.ceil((day.formatted?.request_count.raw ?? 0) * 4 / maxRequestCount))
                    : 0;

                return (
                    <div
                        key={day.dateStr}
                        data-index={index}
                        className="rounded-sm transition-all cursor-pointer hover:scale-150"
                        style={{
                            backgroundColor: level === 0 ? 'var(--muted)' : 'var(--primary)',
                            opacity: level === 0 ? 1 : level / 4,
                        }}
                    />
                );
            })}
        </div>
    ), [days, maxRequestCount]);

    // 遮罩只淡化仍可继续滚动的那一侧, 两端都到头则不淡化。
    const maskImage = edges.atStart && edges.atEnd
        ? 'none'
        : `linear-gradient(to right, ${[
            edges.atStart ? 'black 0' : 'transparent, rgba(0,0,0,0) 10px, black 40px',
            edges.atEnd ? 'black 100%' : 'black calc(100% - 40px), rgba(0,0,0,0) calc(100% - 10px), transparent',
        ].join(', ')})`;

    return (
        <div className="rounded-3xl bg-card border-border border text-card-foreground">
            <div
                ref={scrollRef}
                onScroll={checkScroll}
                onMouseLeave={() => setTooltip((prev) => prev && { ...prev, visible: false })}
                className="overflow-x-auto p-4"
                style={{ maskImage, WebkitMaskImage: maskImage }}
            >
                <div className="ml-auto w-fit">{grid}</div>
            </div>

            {tooltip && createPortal(
                <div
                    className={`fixed z-50 w-fit min-w-max text-sm bg-background text-foreground border rounded-3xl p-3 transition-opacity duration-500 pointer-events-none ${tooltip.visible ? 'opacity-100' : 'opacity-0'}`}
                    style={{
                        left: tooltip.x,
                        top: tooltip.y,
                        // 贴视口边缘时翻转方向, 避免提示溢出屏幕。
                        transform: `translate(${tooltip.x < 200 ? '10%' : tooltip.x > window.innerWidth - 200 ? '-110%' : '-50%'}, ${tooltip.y < window.innerHeight / 2 ? '15%' : '-105%'})`,
                    }}
                >
                    <div className="space-y-2">
                        <p className="font-semibold text-foreground">
                            {`${tooltip.dateStr.slice(0, 4)}-${tooltip.dateStr.slice(4, 6)}-${tooltip.dateStr.slice(6, 8)}`}
                        </p>
                        {tooltip.formatted ? (
                            <div className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 items-center text-muted-foreground">
                                {([
                                    ['requestCount', tooltip.formatted.request_count],
                                    ['waitTime', tooltip.formatted.wait_time],
                                    ['totalToken', tooltip.formatted.total_token],
                                    ['totalCost', tooltip.formatted.total_cost],
                                ] as const).map(([labelKey, metric]) => (
                                    <Fragment key={labelKey}>
                                        <span className="wrap-break-word">{t(labelKey)}</span>
                                        <span className="text-foreground font-medium text-right">
                                            {metric.formatted.value}{metric.formatted.unit}
                                        </span>
                                    </Fragment>
                                ))}
                            </div>
                        ) : (
                            <p className="text-muted-foreground">{t('noData')}</p>
                        )}
                    </div>
                </div>,
                document.body
            )}
        </div>
    );
}
