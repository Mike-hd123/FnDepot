import { useCallback, useEffect, useRef, useState } from 'react';
import { Check, Copy } from 'lucide-react';
import { useCopyToClipboard } from '@uidotdev/usehooks';
import { useTranslations } from 'use-intl';
import { cn } from '@/lib/utils';
import { toast } from 'sonner';

type CopyIconButtonProps = {
    text: string;
    className?: string;
    copyIconClassName?: string;
    checkIconClassName?: string;
};

export function CopyIconButton({
    text,
    className,
    copyIconClassName,
    checkIconClassName,
}: CopyIconButtonProps) {
    const t = useTranslations('common.copy');
    const [, copyToClipboard] = useCopyToClipboard();
    const [copied, setCopied] = useState(false);
    const timerRef = useRef<number | null>(null);

    useEffect(() => {
        return () => {
            if (timerRef.current) window.clearTimeout(timerRef.current);
        };
    }, []);

    const handleClick = useCallback(async () => {
        if (!text) {
            toast.error(t('failed'), { description: t('noContent') });
            return;
        }

        try {
            await copyToClipboard(text);

            setCopied(true);
            toast.success(t('success'));

            if (timerRef.current) window.clearTimeout(timerRef.current);
            timerRef.current = window.setTimeout(() => setCopied(false), 2000);
        } catch (err) {
            const description = err instanceof Error ? err.message : String(err);
            toast.error(t('failed'), { description });
        }
    }, [
        text,
        copyToClipboard,
        t,
    ]);

    return (
        <button
            type="button"
            onClick={handleClick}
            aria-label="Copy"
            className={cn(className)}
        >
            <span className="grid place-items-center">
                <Copy
                    className={cn(
                        'col-start-1 row-start-1 transition-transform duration-200 motion-reduce:transition-none',
                        copied ? 'scale-0 delay-0 ease-in' : 'scale-100 delay-200 ease-out',
                        copyIconClassName,
                    )}
                />
                <Check
                    className={cn(
                        'col-start-1 row-start-1 transition-transform duration-200 motion-reduce:transition-none',
                        copied ? 'scale-100 delay-200 ease-out' : 'scale-0 delay-0 ease-in',
                        checkIconClassName,
                    )}
                />
            </span>
        </button>
    );
}
