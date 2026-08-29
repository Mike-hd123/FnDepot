import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';

// 首页各处共用的统计维度: 金额, 次数, 词元。
export type MetricKey = 'cost' | 'count' | 'tokens';

// 趋势图可选的时间周期, 单位为天。
export type ChartPeriod = '1' | '7' | '30';

// 首页各区块的视图选项, 除渠道名模糊开关外均持久化到 localStorage。
interface HomeViewState {
    channelRankSortMode: MetricKey; // 渠道排行榜的排序维度。
    modelRankSortMode: MetricKey; // 模型排行榜的排序维度。
    chartMetricType: MetricKey; // 趋势图展示的指标。
    chartPeriod: ChartPeriod; // 趋势图的时间周期。
    isChannelNameHidden: boolean; // 是否模糊渠道名称, 分享图跟随此状态, 不持久化。
    setChannelRankSortMode: (value: MetricKey) => void;
    setModelRankSortMode: (value: MetricKey) => void;
    setChartMetricType: (value: MetricKey) => void;
    setChartPeriod: (value: ChartPeriod) => void;
    setChannelNameHidden: (value: boolean) => void;
}

export const useHomeViewStore = create<HomeViewState>()(
    persist(
        (set) => ({
            channelRankSortMode: 'cost',
            modelRankSortMode: 'cost',
            chartMetricType: 'cost',
            chartPeriod: '1',
            isChannelNameHidden: false,
            setChannelRankSortMode: (value) => set({ channelRankSortMode: value }),
            setModelRankSortMode: (value) => set({ modelRankSortMode: value }),
            setChartMetricType: (value) => set({ chartMetricType: value }),
            setChartPeriod: (value) => set({ chartPeriod: value }),
            setChannelNameHidden: (value) => set({ isChannelNameHidden: value }),
        }),
        {
            name: 'home-view-options-storage',
            storage: createJSONStorage(() => localStorage),
            partialize: (state) => ({
                channelRankSortMode: state.channelRankSortMode,
                modelRankSortMode: state.modelRankSortMode,
                chartMetricType: state.chartMetricType,
                chartPeriod: state.chartPeriod,
            }),
        }
    )
);
