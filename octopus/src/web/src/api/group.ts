import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect, useMemo, useState } from 'react';
import { apiRequest } from './client';
import { groupListQueryOptions } from './queries';
import type { ChannelModel } from './channel';

// GroupMode 表示分组的手动或故障转移路由模式。
export type GroupMode = 'manual' | 'failover';

// GroupRelayConfig 保存分组 Relay 配置。
export interface GroupRelayConfig {
    member_max_attempts: number;
    member_retry_interval_seconds: number;
    member_non_stream_response_timeout_seconds: number;
    member_stream_first_event_timeout_seconds: number;
    member_cooldown_seconds: number;
    member_affinity_seconds: number;
}

// GroupItem 是分组内可手动选择或故障转移的渠道模型。
export interface GroupItem {
    id?: number;
    group_id?: number;
    channel_model_id: number;
    channel_model?: ChannelModel;
    priority: number;
}

// GroupRuntimeStatus 表示 Relay 当前进程内的实时路由状态。
export interface GroupRuntimeStatus {
    group_id: number;
    current_item_id: number;
    probe_item_id: number;
    affinity_until: number;
    cooldowns: Record<number, number>;
}

// Group 是客户端模型名称对应的渠道分组。
export interface Group {
    id?: number;
    name: string;
    mode: GroupMode;
    active_item_id: number;
    relay_config: GroupRelayConfig;
    items?: GroupItem[];
    runtime?: GroupRuntimeStatus; // runtime 仅由 SSE 合并到前端缓存，不属于 GET 返回的持久化字段。
}

// GroupItemAddRequest 是待新增的分组项。
interface GroupItemAddRequest {
    channel_model_id: number;
    priority: number;
}

// GroupItemUpdateRequest 是待更新展示和故障转移顺序的分组项。
interface GroupItemUpdateRequest {
    id: number;
    priority: number;
}

// GroupUpdateRequest 是分组普通配置和成员变更。
export interface GroupUpdateRequest {
    id: number;
    name?: string;
    mode?: GroupMode;
    relay_config?: GroupRelayConfig;
    items_to_add?: GroupItemAddRequest[];
    items_to_update?: GroupItemUpdateRequest[];
    items_to_delete?: number[];
}

// useGroupList 获取全部分组，并由明确需要运行状态的页面控制是否建立实时连接。
export function useGroupList(enabled = true, runtimeEnabled = false) {
    // runtimeByGroup 保存 SSE 推送的进程内状态，不写回后端分组配置。
    const [runtimeByGroup, setRuntimeByGroup] = useState(() => new Map<number, GroupRuntimeStatus>());

    const query = useQuery({
        ...groupListQueryOptions,
        enabled,
        refetchInterval: 30000,
        refetchOnMount: 'always',
    });
    const hasInitialData = query.data !== undefined;

    useEffect(() => {
        if (!enabled || !runtimeEnabled || !hasInitialData) return;

        const source = new EventSource('/api/v1/group/runtime/stream', { withCredentials: true });
        source.addEventListener('runtime', (event) => {
            try {
                const update = JSON.parse((event as MessageEvent<string>).data) as GroupRuntimeStatus;
                setRuntimeByGroup((current) => {
                    const next = new Map(current);
                    next.set(update.group_id, update);
                    return next;
                });
            } catch {
                return;
            }
        });
        source.onerror = () => setRuntimeByGroup((current) => current.size === 0 ? current : new Map());

        return () => {
            source.close();
            setRuntimeByGroup((current) => current.size === 0 ? current : new Map());
        };
    }, [enabled, hasInitialData, runtimeEnabled]);
    // 未开启实时连接时不合并 runtime，避免连接关闭前推送的状态残留在列表上。
    const runtimeActive = enabled && runtimeEnabled;
    const groups = useMemo(() => query.data?.map((group) => {
        const runtime = !runtimeActive || group.id === undefined ? undefined : runtimeByGroup.get(group.id);
        return runtime ? { ...group, runtime } : group;
    }), [query.data, runtimeActive, runtimeByGroup]);
    return { ...query, data: groups };
}

// useCreateGroup 创建分组。
export function useCreateGroup() {
    const queryClient = useQueryClient();
    return useMutation({
        mutationFn: (data: Group) =>
            apiRequest<Group>('/api/v1/group/create', { method: 'POST', body: data }),
        onSuccess: () => queryClient.invalidateQueries({ queryKey: groupListQueryOptions.queryKey }),
    });
}

// useUpdateGroup 更新分组配置和成员。
export function useUpdateGroup() {
    const queryClient = useQueryClient();
    return useMutation({
        mutationFn: (data: GroupUpdateRequest) =>
            apiRequest<Group>('/api/v1/group/update', { method: 'POST', body: data }),
        onSuccess: () => queryClient.invalidateQueries({ queryKey: groupListQueryOptions.queryKey }),
    });
}

// useUpdateGroupActiveItem 手动切换或清空分组当前渠道。
export function useUpdateGroupActiveItem() {
    const queryClient = useQueryClient();
    return useMutation({
        mutationFn: ({ groupId, itemId }: { groupId: number; itemId: number }) =>
            apiRequest<Group>(`/api/v1/group/active/${groupId}`, {
                method: 'POST',
                body: { item_id: itemId },
            }),
        onSuccess: () => queryClient.invalidateQueries({ queryKey: groupListQueryOptions.queryKey }),
    });
}

// useDeleteGroup 删除分组。
export function useDeleteGroup() {
    const queryClient = useQueryClient();
    return useMutation({
        mutationFn: (id: number) =>
            apiRequest<null>(`/api/v1/group/delete/${id}`, { method: 'DELETE' }),
        onSuccess: () => queryClient.invalidateQueries({ queryKey: groupListQueryOptions.queryKey }),
    });
}
