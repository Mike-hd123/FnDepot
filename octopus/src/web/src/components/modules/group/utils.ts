export function normalizeKey(value: string) {
    return value.trim().toLowerCase();
}

export function memberKey(member: { channel_grant_id: number }) {
    return String(member.channel_grant_id);
}

export function matchesGroupName(modelName: string, groupKey: string) {
    if (!groupKey) return false;
    return modelName.toLowerCase().includes(groupKey);
}
