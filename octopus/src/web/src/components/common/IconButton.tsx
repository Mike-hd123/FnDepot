import type { ComponentProps } from 'react';
import { Slot } from 'radix-ui';
import { cn } from '@/lib/utils';
import { Tooltip, TooltipTrigger, TooltipContent } from '@/components/ui/tooltip';

// IconButton 图标按钮: 无背景, 悬浮变亮; 给出 tip 时附带 Tooltip。
// 不走 Button: 那里的 ghost 变体自带背景与内边距, 用它反而要多写三个类去抵消,
// 且其基础样式含 transition-all, 会与 asChild 承载的 motion 元素争夺 transform。
// 默认悬浮变亮, 删除类按钮传 hover:text-destructive 覆盖; 尺寸由调用方给出。
export function IconButton({ tip, asChild, className, ...props }: ComponentProps<'button'> & {
    tip?: string; // 为空则不套 Tooltip。
    asChild?: boolean; // 由子元素出标签, 用于承载 motion 元素或自带按钮的组件。
}) {
    const Comp = asChild ? Slot.Root : 'button';
    const button = (
        <Comp
            type={asChild ? undefined : 'button'}
            className={cn(
                'inline-flex items-center justify-center text-muted-foreground transition-colors hover:text-foreground disabled:pointer-events-none disabled:opacity-50',
                className,
            )}
            {...props}
        />
    );

    if (!tip) return button;
    return (
        <Tooltip>
            <TooltipTrigger asChild>{button}</TooltipTrigger>
            <TooltipContent>{tip}</TooltipContent>
        </Tooltip>
    );
}
