import { useMemo } from 'react';
import { ArrowUpAZ } from 'lucide-react';
import { useTranslations } from 'use-intl';
import { useChannelStats } from '@/api/channel';
import { PageActions, usePageActionsStore } from '@/components/common/PageActions';
import { MorphingDialogDescription } from '@/components/ui/morphing-dialog';
import { Card } from './Card';
import { ChannelForm } from './Form';
import { VirtualizedGrid } from '@/components/common/VirtualizedGrid';

// ChannelActions 向稳定顶栏提供渠道页面的搜索、视图选项和创建入口。
export function ChannelActions() {
    const t = useTranslations('toolbar');
    const searchTerm = usePageActionsStore((state) => state.searchTerms.channel || '');
    const layout = usePageActionsStore((state) => state.layouts.channel || 'grid');
    const sortOrder = usePageActionsStore((state) => state.sortOrders.channel === 'desc' ? 'desc' : 'asc');
    const filter = usePageActionsStore((state) => state.channelFilter);
    const setSearchTerm = usePageActionsStore((state) => state.setSearchTerm);
    const setLayout = usePageActionsStore((state) => state.setLayout);
    const setSort = usePageActionsStore((state) => state.setSort);
    const setFilter = usePageActionsStore((state) => state.setChannelFilter);

    return (
        <PageActions
            searchTerm={searchTerm}
            onSearchTermChange={(value) => setSearchTerm('channel', value)}
            layout={layout}
            onLayoutChange={(value) => setLayout('channel', value)}
            sortOptions={[
                { value: 'asc', label: t('popover.nameAsc'), icon: ArrowUpAZ },
                { value: 'desc', label: t('popover.nameDesc'), icon: ArrowUpAZ },
            ]}
            sortValue={sortOrder}
            onSortChange={(value) => {
                if (value === 'asc' || value === 'desc') setSort('channel', value);
            }}
            filterOptions={[
                { value: 'all', label: t('popover.filter.channel.all') },
                { value: 'enabled', label: t('popover.filter.channel.enabled') },
                { value: 'disabled', label: t('popover.filter.channel.disabled') },
            ]}
            filterValue={filter}
            onFilterChange={(value) => {
                if (value === 'all' || value === 'enabled' || value === 'disabled') setFilter(value);
            }}
        >
            <div className="w-screen max-w-full md:max-w-3xl flex flex-col">
                <MorphingDialogDescription disableLayoutAnimation>
                    <ChannelForm />
                </MorphingDialogDescription>
            </div>
        </PageActions>
    );
}

// Channel 渲染渠道列表正文。
export function Channel() {
    const { data: statsData } = useChannelStats();
    const searchTerm = usePageActionsStore((state) => state.searchTerms.channel || '');
    const layout = usePageActionsStore((state) => state.layouts.channel || 'grid');
    const sortOrder = usePageActionsStore((state) => state.sortOrders.channel === 'desc' ? 'desc' : 'asc');
    const filter = usePageActionsStore((state) => state.channelFilter);

    // 先按搜索词和启用状态过滤, 再按名称排序
    const visibleChannels = useMemo(() => {
        const term = searchTerm.toLowerCase().trim();
        const matched = (statsData ?? []).filter((channel) => {
            if (term && !channel.channel_name.toLowerCase().includes(term)) return false;
            if (filter === 'enabled') return channel.enabled;
            if (filter === 'disabled') return !channel.enabled;
            return true;
        });

        return matched.sort((a, b) =>
            sortOrder === 'asc'
                ? a.channel_name.localeCompare(b.channel_name)
                : b.channel_name.localeCompare(a.channel_name)
        );
    }, [statsData, searchTerm, filter, sortOrder]);

    return (
        <VirtualizedGrid
            items={visibleChannels}
            layout={layout}
            columns={{ default: 1, sm: 2, md: 3, lg: 4, xl: 5, '2xl': 6 }}
            estimateItemHeight={232}
            getItemKey={(channel) => `channel-${channel.channel_id}`}
            renderItem={(channel) => (
                <Card channel={channel} />
            )}
        />
    );
}
