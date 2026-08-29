import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { apiRequest } from './client';
import { modelListQueryOptions } from './queries';

/**
 * LLM 价格信息
 */
export interface LLMPrice {
    input: number;
    output: number;
    cache_read: number;
    cache_write: number;
}

/**
 * LLM 模型信息
 */
export interface LLMInfo extends LLMPrice {
    name: string;
}

/**
 * 获取 LLM 模型列表 Hook
 * 
 * @example
 * const { data: models, isLoading, error } = useModelList();
 * 
 * if (isLoading) return <Loading />;
 * if (error) return <Error message={error.message} />;
 * 
 * models?.forEach(model => console.log(model.name, model.input));
 */
export function useModelList() {
    return useQuery({
        ...modelListQueryOptions,
        refetchInterval: 30000,
        refetchOnMount: 'always',
    });
}

/**
 * 更新 LLM 模型 Hook
 * 
 * @example
 * const updateModel = useUpdateModel();
 * 
 * updateModel.mutate({
 *   name: 'gpt-4',
 *   input: 0.03,
 *   output: 0.06,
 *   cache_read: 0.015,
 *   cache_write: 0.03,
 * });
 */
export function useUpdateModel() {
    const queryClient = useQueryClient();

    return useMutation({
        mutationFn: (data: LLMInfo) =>
            apiRequest<LLMInfo>('/api/v1/model/update', { method: 'POST', body: data }),
        onSuccess: () => queryClient.invalidateQueries({ queryKey: modelListQueryOptions.queryKey }),
    });
}

/**
 * 创建 LLM 模型 Hook
 * 
 * @example
 * const createModel = useCreateModel();
 * 
 * createModel.mutate({
 *   name: 'gpt-4',
 *   input: 0.03,
 *   output: 0.06,
 *   cache_read: 0.015,
 *   cache_write: 0.03,
 * });
 */
export function useCreateModel() {
    const queryClient = useQueryClient();

    return useMutation({
        mutationFn: (data: LLMInfo) =>
            apiRequest<LLMInfo>('/api/v1/model/create', { method: 'POST', body: data }),
        onSuccess: () => queryClient.invalidateQueries({ queryKey: modelListQueryOptions.queryKey }),
    });
}

/**
 * 删除 LLM 模型 Hook
 * 
 * @example
 * const deleteModel = useDeleteModel();
 * 
 * deleteModel.mutate('gpt-4'); // 删除名称为 'gpt-4' 的模型
 */
export function useDeleteModel() {
    const queryClient = useQueryClient();

    return useMutation({
        mutationFn: (name: string) =>
            apiRequest<null>('/api/v1/model/delete', { method: 'POST', body: { name } }),
        onSuccess: () => queryClient.invalidateQueries({ queryKey: modelListQueryOptions.queryKey }),
    });
}

/**
 * 更新 LLM 模型价格 Hook
 * 
 * @example
 * const updatePrice = useUpdateModelPrice();
 * 
 * updatePrice.mutate(); // 触发价格更新
 */
export function useUpdateModelPrice() {
    const queryClient = useQueryClient();

    return useMutation({
        mutationFn: () => apiRequest<null>('/api/v1/model/update-price', { method: 'POST', body: {} }),
        onSuccess: () => queryClient.invalidateQueries({ queryKey: ['models', 'last-update-time'] }),
    });
}

/**
 * 重建渠道模型价格 Hook
 */
export function useRebuildModelPrice() {
    const queryClient = useQueryClient();

    return useMutation({
        mutationFn: () => apiRequest<{ count: number }>('/api/v1/model/rebuild-price', { method: 'POST', body: {} }),
        onSuccess: () => queryClient.invalidateQueries({ queryKey: modelListQueryOptions.queryKey }),
    });
}

/**
 * 获取 LLM 模型价格最后更新时间 Hook
 * 
 * @example
 * const { data: lastUpdateTime } = useLastUpdateTime();
 * 
 * if (lastUpdateTime) {
 *   console.log('最后更新:', new Date(lastUpdateTime).toLocaleString());
 * }
 */
export function useLastUpdateTime() {
    return useQuery({
        queryKey: ['models', 'last-update-time'],
        queryFn: () => apiRequest<string>('/api/v1/model/last-update-time'),
        refetchInterval: 30000,
    });
}
