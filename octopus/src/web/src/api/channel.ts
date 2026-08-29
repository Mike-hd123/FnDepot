import { queryOptions, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { apiRequest } from './client';
import { channelListQueryOptions, groupListQueryOptions, modelListQueryOptions } from './queries';
import { formatCount, formatMoney, formatTime } from '@/lib/utils';
import type { StatsMetrics, StatsMetricsFormatted } from './stats';
/**
 * 渠道类型枚举
 */
export enum ChannelType {
    OpenAIChat = 'openai',
    OpenAIResponse = 'openai_responses',
    Anthropic = 'anthropic',
    Gemini = 'gemini',
    Volcengine = 'volcengine',
}

type CustomHeader = {
    header_key: string;
    header_value: string;
};

export type ChannelModelSource = 'auto' | 'manual';

export type ChannelModel = StatsMetrics & {
    id: number;
    channel_id: number;
    name: string;
    source: ChannelModelSource;
};

export type ChannelModelInput = {
    name: string;
    source?: ChannelModelSource;
};

/**
 * 渠道完整数据（与后端 model.Channel 对齐）
 */
export type Channel = StatsMetrics & {
    id: number;
    name: string;
    type: ChannelType;
    enabled: boolean;
    base_url: string;
    key: string;
    models: ChannelModel[];
    proxy: boolean;
    auto_sync: boolean;
    custom_header: CustomHeader[];
    param_override?: string | null;
    channel_proxy?: string | null;
    match_regex?: string | null;
};

// ChannelServer 表示后端可能返回空 custom_header 的原始渠道数据。
export type ChannelServer = Omit<Channel, 'custom_header'> & {
    custom_header: CustomHeader[] | null; // 渠道自定义请求头，空值会在页面查询时归一化为空数组。
};

// ChannelListItem 表示页面消费的渠道原始数据及格式化统计。
type ChannelListItem = {
    raw: Channel; // raw 保存可编辑和标识渠道的原始字段。
    formatted: StatsMetricsFormatted; // formatted 保存直接用于界面展示的统计字段。
};

/**
 * 创建渠道请求：必填字段 + 可选字段
 */
type CreateChannelRequest = {
    name: string;
    type: ChannelType;
    enabled?: boolean;
    base_url: string;
    key: string;
    models: ChannelModelInput[];
    proxy?: boolean;
    auto_sync?: boolean;
    custom_header?: CustomHeader[];
    channel_proxy?: string | null;
    param_override?: string | null;
    match_regex?: string | null;
};

/**
 * 更新渠道请求：id + 可选字段
 */
export type UpdateChannelRequest = {
    id: number;
    name?: string;
    type?: ChannelType;
    enabled?: boolean;
    base_url?: string;
    key?: string;
    models?: ChannelModelInput[];
    proxy?: boolean;
    auto_sync?: boolean;
    custom_header?: CustomHeader[];
    channel_proxy?: string | null;
    param_override?: string | null;
    match_regex?: string | null;
};

type FetchModelRequest = {
    type: ChannelType;
    base_url: string;
    key: string;
    proxy?: boolean;
    channel_proxy?: string | null;
    match_regex?: string | null;
    custom_header?: CustomHeader[];
};

// channelListFormattedQueryOptions 统一渠道列表查询、字段归一化和刷新策略。
const channelListFormattedQueryOptions = queryOptions({
    ...channelListQueryOptions,
    select: (data) => data.map((item): ChannelListItem => {
        const models = item.models ?? [];
        return {
            raw: ({
                ...item,
                models,
                custom_header: item.custom_header ?? [],
            }) satisfies Channel,
            formatted: {
                input_token: formatCount(item.input_token),
                output_token: formatCount(item.output_token),
                total_token: formatCount(item.input_token + item.output_token),
                input_cost: formatMoney(item.input_cost),
                output_cost: formatMoney(item.output_cost),
                total_cost: formatMoney(item.input_cost + item.output_cost),
                request_success: formatCount(item.request_success),
                request_failed: formatCount(item.request_failed),
                request_count: formatCount(item.request_success + item.request_failed),
                wait_time: formatTime(item.wait_time),
            }
        };
    }),
    refetchInterval: 30000,
    refetchOnMount: 'always',
});

/**
 * 获取渠道列表 Hook
 * 
 * @example
 * const { data: channels, isLoading, error } = useChannelList();
 * 
 * if (isLoading) return <Loading />;
 * if (error) return <Error message={error.message} />;
 * 
 * channels?.forEach(channel => console.log(channel.raw.name));
 */
export function useChannelList(enabled = true) {
    return useQuery({ ...channelListFormattedQueryOptions, enabled });
}

/**
 * 创建渠道 Hook
 * 
 * @example
 * const createChannel = useCreateChannel();
 * 
 * createChannel.mutate({
 *   name: 'OpenAI',
 *   type: ChannelType.OpenAIChat,
 *   base_url: 'https://api.openai.com',
 *   key: 'sk-xxx',
 *   models: [{ name: 'gpt-4', source: 'manual' }],
 * });
 */
export function useCreateChannel() {
    const queryClient = useQueryClient();

    return useMutation({
        mutationFn: (data: CreateChannelRequest) =>
            apiRequest<ChannelServer>('/api/v1/channel/create', { method: 'POST', body: data }),
        onSuccess: () => {
            queryClient.invalidateQueries({ queryKey: channelListQueryOptions.queryKey });
            queryClient.invalidateQueries({ queryKey: modelListQueryOptions.queryKey });
        },
    });
}

/**
 * 更新渠道 Hook
 * 
 * @example
 * const updateChannel = useUpdateChannel();
 * 
 * updateChannel.mutate({
 *   id: 1,
 *   name: 'OpenAI Updated',
 *   type: ChannelType.OpenAIChat,
 *   enabled: true,
 *   base_url: 'https://api.openai.com',
 *   key: 'sk-xxx',
 *   models: [{ name: 'gpt-4-turbo', source: 'manual' }],
 *   proxy: false,
 * });
 */
export function useUpdateChannel() {
    const queryClient = useQueryClient();

    return useMutation({
        mutationFn: (data: UpdateChannelRequest) =>
            apiRequest<ChannelServer>('/api/v1/channel/update', { method: 'POST', body: data }),
        onSuccess: () => {
            queryClient.invalidateQueries({ queryKey: channelListQueryOptions.queryKey });
            queryClient.invalidateQueries({ queryKey: modelListQueryOptions.queryKey });
            queryClient.invalidateQueries({ queryKey: groupListQueryOptions.queryKey });
        },
    });
}

/**
 * 删除渠道 Hook
 * 
 * @example
 * const deleteChannel = useDeleteChannel();
 * 
 * deleteChannel.mutate(1); // 删除 ID 为 1 的渠道
 */
export function useDeleteChannel() {
    const queryClient = useQueryClient();

    return useMutation({
        mutationFn: (id: number) =>
            apiRequest<null>(`/api/v1/channel/delete/${id}`, { method: 'DELETE' }),
        onSuccess: () => {
            queryClient.invalidateQueries({ queryKey: channelListQueryOptions.queryKey });
            queryClient.invalidateQueries({ queryKey: modelListQueryOptions.queryKey });
            queryClient.invalidateQueries({ queryKey: groupListQueryOptions.queryKey });
        },
    });
}

/**
 * 启用/禁用渠道 Hook
 * 
 * @example
 * const enableChannel = useEnableChannel();
 * 
 * enableChannel.mutate({ id: 1, enabled: true }); // 启用 ID 为 1 的渠道
 * enableChannel.mutate({ id: 1, enabled: false }); // 禁用 ID 为 1 的渠道
 */
export function useEnableChannel() {
    const queryClient = useQueryClient();

    return useMutation({
        mutationFn: (data: { id: number; enabled: boolean }) =>
            apiRequest<null>('/api/v1/channel/enable', { method: 'POST', body: data }),
        onSuccess: () => {
            queryClient.invalidateQueries({ queryKey: channelListQueryOptions.queryKey });
        },
    });
}

/**
 * 获取渠道模型列表 Hook
 * 
 * @example
 * const fetchModel = useFetchModel();
 * 
 * fetchModel.mutate({
 *   type: ChannelType.OpenAIChat,
 *   base_url: 'https://api.openai.com',
 *   key: 'sk-xxx',
 *   proxy: false,
 * });
 * 
 * // 在 onSuccess 中获取模型列表
 * fetchModel.data // ['gpt-4', 'gpt-3.5-turbo', ...]
 */
export function useFetchModel() {
    return useMutation({
        mutationFn: (data: FetchModelRequest) =>
            apiRequest<string[]>('/api/v1/channel/fetch-model', { method: 'POST', body: data }),
    });
}

/**
 * 获取渠道最后同步时间 Hook
 * 
 * @example
 * const lastSyncTime = useLastSyncTime();
 * 
 * if (lastSyncTime) {
 *   console.log('最后同步时间:', new Date(lastSyncTime).toLocaleString());
 * }
 */
export function useLastSyncTime() {
    return useQuery({
        queryKey: ['channels', 'last-sync-time'],
        queryFn: () => apiRequest<string>('/api/v1/channel/last-sync-time'),
        refetchInterval: 30000,
    });
}
/**
 * 同步渠道 Hook
 * 
 * @example
 * const syncChannel = useSyncChannel();
 * 
 * syncChannel.mutate();
 */
export function useSyncChannel() {
    const queryClient = useQueryClient();
    return useMutation({
        mutationFn: () => apiRequest<null>('/api/v1/channel/sync', { method: 'POST' }),
        onSuccess: () => {
            queryClient.invalidateQueries({ queryKey: ['channels', 'last-sync-time'] });
            queryClient.invalidateQueries({ queryKey: channelListQueryOptions.queryKey });
            queryClient.invalidateQueries({ queryKey: modelListQueryOptions.queryKey });
            queryClient.invalidateQueries({ queryKey: groupListQueryOptions.queryKey });
        },
    });
}
