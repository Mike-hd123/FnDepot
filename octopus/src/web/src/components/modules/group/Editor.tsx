import { useCallback, useMemo, useState, type FormEvent } from 'react';
import { Check, ChevronDownIcon, HelpCircle, Plus, Search, Sparkles, Trash2 } from 'lucide-react';
import { useTranslations } from 'use-intl';
import * as AccordionPrimitive from '@radix-ui/react-accordion';
import { useChannelList } from '@/api/channel';
import { Button } from '@/components/ui/button';
import { Field, FieldGroup, FieldLabel } from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import { Accordion, AccordionContent, AccordionItem } from '@/components/ui/accordion';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import { getModelIcon } from '@/lib/model-icons';
import type { GroupMode, GroupRelayConfig } from '@/api/group';
import type { SelectedMember } from './ItemList';
import { MemberList } from './ItemList';
import { matchesGroupName, memberKey, normalizeKey } from './utils';

export type GroupEditorValues = {
    name: string;
    mode: GroupMode;
    relay_config: GroupRelayConfig;
    members: SelectedMember[];
};

// defaultRelayConfig 提供创建分组时的前端初始配置。
const defaultRelayConfig: GroupRelayConfig = {
    member_max_attempts: 2,
    member_retry_interval_seconds: 1,
    member_non_stream_response_timeout_seconds: 120,
    member_stream_first_event_timeout_seconds: 30,
    member_cooldown_seconds: 60,
    member_affinity_seconds: 0,
};

// FieldHelp 渲染配置字段的简短帮助提示。
function FieldHelp({ text }: { text: string }) {
    return (
        <Tooltip>
            <TooltipTrigger asChild>
                <HelpCircle className="size-4 cursor-help text-muted-foreground" />
            </TooltipTrigger>
            <TooltipContent side="top" sideOffset={10} align="center">
                {text}
            </TooltipContent>
        </Tooltip>
    );
}

function ModelPickerSection({
    modelChannels,
    selectedMembers,
    onAdd,
    onAutoAdd,
    autoAddDisabled,
}: {
    modelChannels: SelectedMember[];
    selectedMembers: SelectedMember[];
    onAdd: (channel: SelectedMember) => void;
    onAutoAdd: () => void;
    autoAddDisabled: boolean;
}) {
    const t = useTranslations('group');
    const [searchKeyword, setSearchKeyword] = useState('');

    const selectedKeys = useMemo(() => new Set(selectedMembers.map(memberKey)), [selectedMembers]);
    const normalizedSearch = searchKeyword.trim().toLowerCase();

    const channels = useMemo(() => {
        const byId = new Map<number, { id: number; name: string; models: SelectedMember[] }>();
        modelChannels.forEach((mc) => {
            const existing = byId.get(mc.channel_id);
            if (existing) existing.models.push(mc);
            else byId.set(mc.channel_id, { id: mc.channel_id, name: mc.channel_name, models: [mc] });
        });

        return Array.from(byId.values())
            .map((c) => ({ ...c, models: [...c.models].sort((a, b) => a.name.localeCompare(b.name)) }))
            .sort((a, b) => a.id - b.id);
    }, [modelChannels]);

    const filteredChannels = useMemo(() => {
        if (!normalizedSearch) return channels;
        return channels.reduce<typeof channels>((acc, channel) => {
            if (channel.name.toLowerCase().includes(normalizedSearch)) {
                acc.push(channel);
                return acc;
            }

            const models = channel.models.filter((model) => model.name.toLowerCase().includes(normalizedSearch));
            if (models.length > 0) acc.push({ ...channel, models });
            return acc;
        }, []);
    }, [channels, normalizedSearch]);

    return (
        <div className="rounded-xl border border-border/50 bg-muted/30 flex flex-col min-h-0">
            <div className="grid grid-cols-[1fr_auto_1fr] items-center gap-2 px-3 py-2 border-b border-border/30 bg-muted/50">
                <span className="min-w-0 justify-self-start text-sm font-medium text-foreground">
                    {t('form.addItem')}
                </span>

                <div className="relative justify-self-center w-30">
                    <Search className="pointer-events-none absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
                    <Input
                        value={searchKeyword}
                        onChange={(event) => setSearchKeyword(event.target.value)}
                        className="h-6 rounded-lg border-border/60 bg-background/70 pl-7 pr-2 text-xs shadow-none focus-visible:border-border/60 focus-visible:ring-0"
                        aria-label="search"
                    />
                </div>

                <button
                    type="button"
                    onClick={onAutoAdd}
                    className={cn(
                        'justify-self-end shrink-0 flex items-center gap-1 px-2 py-1 rounded-lg text-xs font-medium transition-colors',
                        autoAddDisabled
                            ? 'text-muted-foreground/50 cursor-not-allowed'
                            : 'hover:bg-muted text-muted-foreground hover:text-foreground'
                    )}
                    disabled={autoAddDisabled}
                    title={t('form.autoAdd')}
                >
                    <Sparkles className="size-3.5" />
                    <span>{t('form.autoAdd')}</span>
                </button>
            </div>

            <div className="flex-1 min-h-0 overflow-y-auto p-2">
                <Accordion type="multiple" className="w-full space-y-2">
                    {filteredChannels.map((channel) => {
                        const total = channel.models.length;
                        const selectedCount = channel.models.reduce(
                            (acc, m) => acc + (selectedKeys.has(memberKey(m)) ? 1 : 0),
                            0
                        );
                        const available = total - selectedCount;

                        return (
                            <AccordionItem key={channel.id} value={`channel-${channel.id}`}>
                                <AccordionPrimitive.Header className="rounded-lg bg-muted sticky top-0 z-10 flex px-2 overflow-hidden">
                                    <AccordionPrimitive.Trigger className="flex flex-1 min-w-0 items-center gap-4 py-4 text-left text-sm transition-all outline-none focus-visible:ring-[3px] disabled:pointer-events-none disabled:opacity-50 [&[data-state=open]>svg]:rotate-180">
                                        <span className="truncate">{channel.name}</span>
                                        <span className="text-xs text-muted-foreground shrink-0">
                                            {available}/{total}
                                        </span>
                                        <ChevronDownIcon className="text-muted-foreground pointer-events-none size-4 shrink-0 transition-transform duration-200" />
                                    </AccordionPrimitive.Trigger>
                                </AccordionPrimitive.Header>
                                <AccordionContent className="px-2 pt-2">
                                    <div className="flex flex-col gap-1.5">
                                        {channel.models.map((m) => {
                                            const isSelected = selectedKeys.has(memberKey(m));
                                            const { Icon, className: iconClassName } = getModelIcon(m.name);
                                            return (
                                                <button
                                                    key={memberKey(m)}
                                                    type="button"
                                                    onClick={() => !isSelected && onAdd(m)}
                                                    disabled={isSelected}
                                                    className={cn(
                                                        'w-full flex items-center justify-between gap-2 rounded-lg border border-border/50 bg-background px-2.5 py-2 text-left transition-colors',
                                                        isSelected ? 'opacity-60 cursor-not-allowed' : 'hover:bg-muted'
                                                    )}
                                                >
                                                    <span className="flex items-center gap-2 min-w-0">
                                                        <Icon aria-hidden="true" className={iconClassName} width={16} height={16} />
                                                        <span className="text-sm font-medium truncate">{m.name}</span>
                                                    </span>

                                                    <span className="shrink-0 text-muted-foreground">
                                                        {isSelected ? (
                                                            <Check className="size-4 text-primary" />
                                                        ) : (
                                                            <Plus className="size-4" />
                                                        )}
                                                    </span>
                                                </button>
                                            );
                                        })}
                                    </div>
                                </AccordionContent>
                            </AccordionItem>
                        );
                    })}
                </Accordion>
            </div>
        </div>
    );
}

function SortSection({
    members,
    onReorder,
    onRemove,
    removingIds,
    onClear,
}: {
    members: SelectedMember[];
    onReorder: (members: SelectedMember[]) => void;
    onRemove: (id: string) => void;
    removingIds: Set<string>;
    onClear: () => void;
}) {
    const t = useTranslations('group');

    return (
        <div className="rounded-xl border border-border/50 bg-muted/30 flex flex-col min-h-0">
            <div className="flex items-center justify-between px-3 py-2 border-b border-border/30 bg-muted/50">
                <span className="text-sm font-medium text-foreground">
                    {t('form.items')}
                    {members.length > 0 && (
                        <span className="ml-1.5 text-xs text-muted-foreground font-normal">
                            ({members.length})
                        </span>
                    )}
                </span>
                <button
                    type="button"
                    onClick={onClear}
                    disabled={members.length === 0}
                    className={cn(
                        'flex items-center gap-1 px-2 py-1 rounded-lg text-xs font-medium transition-colors',
                        members.length === 0
                            ? 'text-muted-foreground/50 cursor-not-allowed'
                            : 'hover:bg-muted text-muted-foreground hover:text-foreground'
                    )}
                    title={t('form.clear')}
                >
                    <Trash2 className="size-3.5" />
                    <span>{t('form.clear')}</span>
                </button>
            </div>

            <div className="flex-1 min-h-0">
                <MemberList
                    members={members}
                    onReorder={onReorder}
                    onRemove={onRemove}
                    removingIds={removingIds}
                    showConfirmDelete={false}
                />
            </div>
        </div>
    );
}

export function GroupEditor({
    initial,
    submitText,
    submittingText,
    isSubmitting,
    onSubmit,
    onCancel,
}: {
    initial?: {
        name?: string;
        mode?: GroupMode;
        relay_config?: Partial<GroupRelayConfig>;
        members?: SelectedMember[];
    };
    submitText: string;
    submittingText: string;
    isSubmitting: boolean;
    onSubmit: (values: GroupEditorValues) => void;
    onCancel?: () => void;
}) {
    const t = useTranslations('group');
    const { data: channelsData = [] } = useChannelList();
    const modelChannels = useMemo<SelectedMember[]>(() => channelsData.flatMap(({ raw: channel }) =>
        channel.models.map((channelModel) => ({
            id: String(channelModel.id),
            channel_model_id: channelModel.id,
            name: channelModel.name,
            enabled: channel.enabled,
            channel_id: channelModel.channel_id,
            channel_name: channel.name,
        }))
    ), [channelsData]);

    const [groupName, setGroupName] = useState(initial?.name ?? '');
    const [mode, setMode] = useState<GroupMode>(initial?.mode ?? 'manual');
    const [relayConfig, setRelayConfig] = useState<GroupRelayConfig>(() => ({
        ...defaultRelayConfig,
        ...initial?.relay_config,
    }));
    const [selectedMembers, setSelectedMembers] = useState<SelectedMember[]>(initial?.members ?? []);
    const [removingIds, setRemovingIds] = useState<Set<string>>(new Set());

    const groupKey = normalizeKey(groupName);

    const matchedModelChannels = useMemo(() => {
        if (!groupKey) return [];
        return modelChannels.filter((mc) => matchesGroupName(mc.name, groupKey));
    }, [groupKey, modelChannels]);

    const handleAddMember = useCallback((channel: SelectedMember) => {
        const key = memberKey(channel);
        setSelectedMembers((prev) => {
            if (prev.some((m) => m.id === key)) return prev;
            return [...prev, { ...channel, id: key }];
        });
    }, []);

    const autoAddDisabled = useMemo(() => {
        if (!groupKey || matchedModelChannels.length === 0) return true;
        const existing = new Set(selectedMembers.map((m) => m.id));
        return matchedModelChannels.every((mc) => existing.has(memberKey(mc)));
    }, [groupKey, matchedModelChannels, selectedMembers]);

    const handleAutoAdd = useCallback(() => {
        if (matchedModelChannels.length === 0) return;
        setSelectedMembers((prev) => {
            const existing = new Set(prev.map((m) => m.id));
            const toAdd = matchedModelChannels
                .filter((mc) => !existing.has(memberKey(mc)))
                .map((mc) => ({ ...mc, id: memberKey(mc) }));
            return toAdd.length ? [...prev, ...toAdd] : prev;
        });
    }, [matchedModelChannels]);

    const handleRemoveMember = useCallback((id: string) => {
        setRemovingIds((prev) => new Set(prev).add(id));
        setTimeout(() => {
            setSelectedMembers((prev) => prev.filter((m) => m.id !== id));
            setRemovingIds((prev) => { const n = new Set(prev); n.delete(id); return n; });
        }, 200);
    }, []);

    const handleClearMembers = useCallback(() => {
        setSelectedMembers([]);
        setRemovingIds(new Set());
    }, []);

    const isValid = groupKey.length > 0 && selectedMembers.length > 0;

    const handleSubmit = (event: FormEvent<HTMLFormElement>) => {
        event.preventDefault();
        if (!isValid) return;
        onSubmit({
            name: groupName,
            mode,
            relay_config: relayConfig,
            members: selectedMembers,
        });
    };


    return (
        <form onSubmit={handleSubmit} className="flex flex-col h-full min-h-0 ">
            <div className="flex-1 min-h-0 overflow-hidden px-1">
                <FieldGroup className="gap-4 flex flex-col min-h-0 h-full">
                    <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                        <Field>
                            <FieldLabel htmlFor="group-name">{t('form.name')}</FieldLabel>
                            <Input
                                id="group-name"
                                value={groupName}
                                onChange={(e) => setGroupName(e.target.value)}
                                className="rounded-xl"
                            />
                        </Field>
                        <Field>
                            <FieldLabel htmlFor="group-mode">
                                {t('form.mode')}
                                <FieldHelp text={t('form.modeHint')} />
                            </FieldLabel>
                            <Select
                                value={mode}
                                onValueChange={(value) => setMode(value as GroupMode)}
                            >
                                <SelectTrigger id="group-mode" className="w-full rounded-xl">
                                    <SelectValue />
                                </SelectTrigger>
                                <SelectContent>
                                    <SelectItem value="manual">{t('form.manual')}</SelectItem>
                                    <SelectItem value="failover">{t('form.failover')}</SelectItem>
                                </SelectContent>
                            </Select>
                        </Field>
                    </div>

                    <Tabs defaultValue="members" className="flex flex-1 min-h-0">
                        <TabsList className="grid w-full shrink-0 grid-cols-2">
                            <TabsTrigger value="members">{t('form.members')}</TabsTrigger>
                            <TabsTrigger value="relay">{t('form.relay')}</TabsTrigger>
                        </TabsList>

                        <TabsContent value="members" className="min-h-0 overflow-hidden">
                            <div className="grid h-full min-h-0 grid-cols-1 gap-4 md:grid-cols-2">
                                <ModelPickerSection
                                    modelChannels={modelChannels}
                                    selectedMembers={selectedMembers}
                                    onAdd={handleAddMember}
                                    onAutoAdd={handleAutoAdd}
                                    autoAddDisabled={autoAddDisabled}
                                />
                                <SortSection
                                    members={selectedMembers}
                                    onReorder={setSelectedMembers}
                                    onRemove={handleRemoveMember}
                                    removingIds={removingIds}
                                    onClear={handleClearMembers}
                                />
                            </div>
                        </TabsContent>

                        <TabsContent value="relay" className="min-h-0 overflow-y-auto px-1">
                            <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
                                <Field>
                                    <FieldLabel htmlFor="group-retry-count">
                                        {t('form.retryCount')}
                                        <FieldHelp text={t('form.retryCountHint')} />
                                    </FieldLabel>
                                    <Input
                                        id="group-retry-count"
                                        type="number"
                                        inputMode="numeric"
                                        min={0}
                                        step={1}
                                        value={String(relayConfig.member_max_attempts)}
                                        onChange={(event) => {
                                            const value = Number.parseInt(event.target.value, 10);
                                            setRelayConfig((prev) => ({ ...prev, member_max_attempts: Number.isFinite(value) && value >= 1 ? value : 1 }));
                                        }}
                                        className="rounded-xl"
                                    />
                                </Field>
                                <Field>
                                    <FieldLabel htmlFor="group-retry-interval">
                                        {t('form.retryInterval')}
                                        <FieldHelp text={t('form.retryIntervalHint')} />
                                    </FieldLabel>
                                    <Input
                                        id="group-retry-interval"
                                        type="number"
                                        inputMode="numeric"
                                        min={1}
                                        step={1}
                                        value={String(relayConfig.member_retry_interval_seconds)}
                                        onChange={(event) => {
                                            const value = Number.parseInt(event.target.value, 10);
                                            setRelayConfig((prev) => ({ ...prev, member_retry_interval_seconds: Number.isFinite(value) && value >= 1 ? value : 1 }));
                                        }}
                                        className="rounded-xl"
                                    />
                                </Field>
                                <Field>
                                    <FieldLabel htmlFor="group-non-stream-timeout">
                                        {t('form.nonStreamTimeout')}
                                        <FieldHelp text={t('form.nonStreamTimeoutHint')} />
                                    </FieldLabel>
                                    <Input
                                        id="group-non-stream-timeout"
                                        type="number"
                                        inputMode="numeric"
                                        min={1}
                                        step={1}
                                        value={String(relayConfig.member_non_stream_response_timeout_seconds)}
                                        onChange={(event) => {
                                            const value = Number.parseInt(event.target.value, 10);
                                            setRelayConfig((prev) => ({ ...prev, member_non_stream_response_timeout_seconds: Number.isFinite(value) && value >= 1 ? value : 1 }));
                                        }}
                                        className="rounded-xl"
                                    />
                                </Field>
                                <Field>
                                    <FieldLabel htmlFor="group-stream-timeout">
                                        {t('form.streamTimeout')}
                                        <FieldHelp text={t('form.streamTimeoutHint')} />
                                    </FieldLabel>
                                    <Input
                                        id="group-stream-timeout"
                                        type="number"
                                        inputMode="numeric"
                                        min={1}
                                        step={1}
                                        value={String(relayConfig.member_stream_first_event_timeout_seconds)}
                                        onChange={(event) => {
                                            const value = Number.parseInt(event.target.value, 10);
                                            setRelayConfig((prev) => ({ ...prev, member_stream_first_event_timeout_seconds: Number.isFinite(value) && value >= 1 ? value : 1 }));
                                        }}
                                        className="rounded-xl"
                                    />
                                </Field>
                                <Field>
                                    <FieldLabel htmlFor="group-cooldown">
                                        {t('form.cooldown')}
                                        <FieldHelp text={t('form.cooldownHint')} />
                                    </FieldLabel>
                                    <Input
                                        id="group-cooldown"
                                        type="number"
                                        inputMode="numeric"
                                        min={1}
                                        step={1}
                                        value={String(relayConfig.member_cooldown_seconds)}
                                        onChange={(event) => {
                                            const value = Number.parseInt(event.target.value, 10);
                                            setRelayConfig((prev) => ({ ...prev, member_cooldown_seconds: Number.isFinite(value) && value >= 1 ? value : 1 }));
                                        }}
                                        className="rounded-xl"
                                    />
                                </Field>
                                <Field>
                                    <FieldLabel htmlFor="group-affinity">
                                        {t('form.affinity')}
                                        <FieldHelp text={t('form.affinityHint')} />
                                    </FieldLabel>
                                    <Input
                                        id="group-affinity"
                                        type="number"
                                        inputMode="numeric"
                                        min={0}
                                        step={1}
                                        value={String(relayConfig.member_affinity_seconds)}
                                        onChange={(event) => {
                                            const value = Number.parseInt(event.target.value, 10);
                                            setRelayConfig((prev) => ({ ...prev, member_affinity_seconds: Number.isFinite(value) && value >= 0 ? value : 0 }));
                                        }}
                                        className="rounded-xl"
                                    />
                                </Field>
                            </div>
                        </TabsContent>
                    </Tabs>
                </FieldGroup>
            </div>

            <div className="pt-4 mt-auto shrink-0">
                <div className="flex gap-2">
                    {onCancel && (
                        <Button type="button" variant="secondary" className="flex-1 rounded-xl h-11" onClick={onCancel}>
                            {t('detail.actions.cancel')}
                        </Button>
                    )}
                    <Button
                        type="submit"
                        disabled={!isValid || isSubmitting}
                        className="flex-1 rounded-xl h-11"
                    >
                        {isSubmitting ? submittingText : submitText}
                    </Button>
                </div>
            </div>
        </form>
    );
}
