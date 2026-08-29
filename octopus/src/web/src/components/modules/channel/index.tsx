import { useMemo } from 'react';
import { ArrowUpAZ } from 'lucide-react';
import { useTranslations } from 'use-intl';
import { useChannelList } from '@/api/channel';
import { PageActions, usePageActionsStore } from '@/components/common/PageActions';
import {
    MorphingDialogClose,
    MorphingDialogTitle,
    MorphingDialogDescription,
} from '@/components/ui/morphing-dialog';
import { Card } from './Card';
import { ChannelForm } from './Form';
import { VirtualizedGrid } from '@/components/common/VirtualizedGrid';

// ChannelActions 向稳定顶栏提供渠道页面的搜索、视图选项和创建入口。
export function ChannelActions() {
    const t = useTranslations('toolbar');
    const tCreate = useTranslations('channel.create');
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
            <div className="w-screen max-w-full md:max-w-xl h-full min-h-0 flex flex-col">
                <MorphingDialogTitle className="shrink-0">
                    <header className="mb-6 flex items-center justify-between">
                        <h2 className="text-2xl font-bold text-card-foreground">{tCreate('dialogTitle')}</h2>
                        <MorphingDialogClose
                            className="relative right-0 top-0"
                            variants={{
                                initial: { opacity: 0, scale: 0.8 },
                                animate: { opacity: 1, scale: 1 },
                                exit: { opacity: 0, scale: 0.8 }
                            }}
                        />
                    </header>
                </MorphingDialogTitle>
                <MorphingDialogDescription disableLayoutAnimation className="flex-1 min-h-0 overflow-auto">
                    <ChannelForm />
                </MorphingDialogDescription>
            </div>
        </PageActions>
    );
}

// Channel 渲染渠道列表正文。
export function Channel() {
    const { data: channelsData } = useChannelList();
    const searchTerm = usePageActionsStore((state) => state.searchTerms.channel || '');
    const layout = usePageActionsStore((state) => state.layouts.channel || 'grid');
    const sortOrder = usePageActionsStore((state) => state.sortOrders.channel === 'desc' ? 'desc' : 'asc');
    const filter = usePageActionsStore((state) => state.channelFilter);

    // 先按搜索词和启用状态过滤, 再按名称排序
    const visibleChannels = useMemo(() => {
        const term = searchTerm.toLowerCase().trim();
        const matched = (channelsData ?? []).filter((item) => {
            if (term && !item.raw.name.toLowerCase().includes(term)) return false;
            if (filter === 'enabled') return item.raw.enabled;
            if (filter === 'disabled') return !item.raw.enabled;
            return true;
        });

        return matched.sort((a, b) =>
            sortOrder === 'asc'
                ? a.raw.name.localeCompare(b.raw.name)
                : b.raw.name.localeCompare(a.raw.name)
        );
    }, [channelsData, searchTerm, filter, sortOrder]);

    return (
        <VirtualizedGrid
            items={visibleChannels}
            layout={layout}
            columns={{ default: 1, md: 2, lg: 3 }}
            estimateItemHeight={216}
            getItemKey={(item) => `channel-${item.raw.id}`}
            renderItem={(item) => <Card channel={item.raw} stats={item.formatted} layout={layout} />}
        />
    );
}
