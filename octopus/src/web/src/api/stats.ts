import { queryOptions, useQuery } from '@tanstack/react-query';
import { apiRequest } from './client';
import { statsDailyQueryOptions, statsHourlyQueryOptions, statsTotalQueryOptions } from './queries';
import { formatCount, formatMoney, formatTime } from '@/lib/utils';

/**
 * 统计数据
 */
export interface StatsMetrics {
    input_token: number;
    output_token: number;
    input_cost: number;
    output_cost: number;
    wait_time: number;
    request_success: number;
    request_failed: number;
}

export interface StatsMetricsFormatted {
    input_token: ReturnType<typeof formatCount>;
    output_token: ReturnType<typeof formatCount>;
    input_cost: ReturnType<typeof formatMoney>;
    output_cost: ReturnType<typeof formatMoney>;
    wait_time: ReturnType<typeof formatTime>;
    request_success: ReturnType<typeof formatCount>;
    request_failed: ReturnType<typeof formatCount>;

    request_count: ReturnType<typeof formatCount>;
    total_token: ReturnType<typeof formatCount>;
    total_cost: ReturnType<typeof formatMoney>;
}

// formatStatsMetrics 把一组累计统计格式化为界面展示字段，并补齐合计项。
// 渠道、渠道模型和渠道凭据各自维护独立计数，均按同一口径展示。
export function formatStatsMetrics(metrics: StatsMetrics): StatsMetricsFormatted {
    return {
        input_token: formatCount(metrics.input_token),
        output_token: formatCount(metrics.output_token),
        total_token: formatCount(metrics.input_token + metrics.output_token),
        input_cost: formatMoney(metrics.input_cost),
        output_cost: formatMoney(metrics.output_cost),
        total_cost: formatMoney(metrics.input_cost + metrics.output_cost),
        wait_time: formatTime(metrics.wait_time),
        request_success: formatCount(metrics.request_success),
        request_failed: formatCount(metrics.request_failed),
        request_count: formatCount(metrics.request_success + metrics.request_failed),
    };
}

export interface StatsDaily extends StatsMetrics {
    date: string;
}
export interface StatsDailyResponse {
    max_request_count: number;
    items: StatsDaily[];
}
export interface StatsDailyFormatted extends StatsMetricsFormatted {
    date: string;
}
interface StatsDailyFormattedResponse {
    max_request_count: number;
    items: StatsDailyFormatted[];
}

export interface StatsTotal extends StatsMetrics {
    id: number;
}
type StatsTotalFormatted = StatsMetricsFormatted;

export interface StatsHourly extends StatsMetrics {
    hour: number;
    date: string;
}
interface StatsHourlyFormatted extends StatsMetricsFormatted {
    hour: number;
    date: string;
}
/**
 * API Key 统计数据
 */
export interface StatsAPIKey extends StatsMetrics {
    api_key_id: number;
}

export interface StatsAPIKeyFormatted extends StatsMetricsFormatted {
    api_key_id: number;
}

// statsDailyFormattedQueryOptions 统一首页每日统计查询、格式化和刷新策略。
const statsDailyFormattedQueryOptions = queryOptions({
    ...statsDailyQueryOptions,
    select: (data): StatsDailyFormattedResponse => ({
        max_request_count: data.max_request_count,
        items: data.items.map((item): StatsDailyFormatted => ({
            ...formatStatsMetrics(item),
            date: item.date,
        })),
    }),
    refetchInterval: 3600000, // 1 小时
    refetchOnMount: 'always',
});

/**
 * 获取每日统计数据 Hook
 */
export function useStatsDaily() {
    const query = useQuery(statsDailyFormattedQueryOptions);
    return {
        ...query,
        data: query.data?.items,
        maxRequestCount: query.data?.max_request_count ?? 0,
    };
}

// statsHourlyFormattedQueryOptions 统一首页每小时统计查询、格式化和刷新策略。
const statsHourlyFormattedQueryOptions = queryOptions({
    ...statsHourlyQueryOptions,
    select: (data) => data.map((item): StatsHourlyFormatted => ({
        ...formatStatsMetrics(item),
        hour: item.hour,
        date: item.date,
    })),
    refetchInterval: 10000,// 10 秒
    refetchOnMount: 'always',
});

/**
 * 获取每小时统计数据 Hook
 */
export function useStatsHourly() {
    return useQuery(statsHourlyFormattedQueryOptions);
}

// statsTotalFormattedQueryOptions 统一首页总统计查询、格式化和刷新策略。
const statsTotalFormattedQueryOptions = queryOptions({
    ...statsTotalQueryOptions,
    select: (data): StatsTotalFormatted => formatStatsMetrics(data),
    refetchInterval: 10000,// 10 秒
    refetchOnMount: 'always',
});

/**
 * 获取总统计数据 Hook
 */
export function useStatsTotal() {
    return useQuery(statsTotalFormattedQueryOptions);
}



/**
 * 获取 API Key 统计数据列表 Hook
 */
export function useStatsAPIKey() {
    return useQuery({
        queryKey: ['stats', 'apikey'],
        queryFn: () => apiRequest<StatsAPIKey[]>('/api/v1/stats/apikey'),
        select: (data) => data.map((item): StatsAPIKeyFormatted => ({
            ...formatStatsMetrics(item),
            api_key_id: item.api_key_id,
        })),
        refetchInterval: 30000,
        refetchOnMount: 'always',
    });
}
