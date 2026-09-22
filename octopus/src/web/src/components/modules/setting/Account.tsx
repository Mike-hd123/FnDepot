import { useState } from 'react';
import { useTranslations } from 'use-intl';
import { User, KeyRound, Lock, Eye, EyeOff, Pencil, Check, X, Loader } from 'lucide-react';
import { motion, AnimatePresence } from 'motion/react';
import { Input } from '@/components/ui/input';
import { useChangeUsername, useChangePassword, useAuth } from '@/api/user';
import { toast } from 'sonner';

/** 密码输入框，附带显示/隐藏切换 */
function PasswordInput({
    value,
    onChange,
    placeholder,
    disabled,
}: {
    value: string;
    onChange: (value: string) => void;
    placeholder: string;
    disabled: boolean;
}) {
    const [visible, setVisible] = useState(false);

    return (
        <div className="relative">
            <Input
                type={visible ? 'text' : 'password'}
                value={value}
                onChange={(e) => onChange(e.target.value)}
                placeholder={placeholder}
                className="h-9 text-sm rounded-xl pr-10"
                disabled={disabled}
            />
            <button
                type="button"
                onClick={() => setVisible(!visible)}
                aria-label={placeholder}
                className="absolute right-3 top-1/2 -translate-y-1/2 text-muted-foreground transition-colors hover:text-foreground"
            >
                {visible ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
            </button>
        </div>
    );
}

/** 修改用户名的弹出表单，成功后延迟登出 */
function UsernameForm({ onClose }: { onClose: () => void }) {
    const t = useTranslations('setting');
    const { logout } = useAuth();
    const changeUsername = useChangeUsername();
    const [newUsername, setNewUsername] = useState('');

    const handleSubmit = (e: React.FormEvent) => {
        e.preventDefault();
        if (!newUsername.trim()) {
            toast.error(t('account.username.empty'));
            return;
        }

        changeUsername.mutate(
            { newUsername: newUsername.trim() },
            {
                onSuccess: () => {
                    toast.success(t('account.username.success'));
                    onClose();
                    setTimeout(() => logout(), 1000);
                },
                onError: () => toast.error(t('account.username.failed')),
            }
        );
    };

    return (
        <motion.form
            layoutId="account-username"
            onSubmit={handleSubmit}
            transition={{ type: 'spring', stiffness: 400, damping: 30 }}
            className="absolute left-1/2 top-1/2 z-20 grid w-[min(420px,calc(100vw-2rem))] -translate-x-1/2 -translate-y-1/2 gap-2 rounded-3xl border border-border bg-card p-5"
        >
            <Input
                type="text"
                value={newUsername}
                onChange={(e) => setNewUsername(e.target.value)}
                placeholder={t('account.username.placeholder')}
                className="h-9 text-sm rounded-xl"
                disabled={changeUsername.isPending}
            />

            <div className="flex gap-2 pt-1">
                <button
                    type="button"
                    onClick={onClose}
                    disabled={changeUsername.isPending}
                    className="flex-1 h-9 flex items-center justify-center gap-1.5 rounded-xl bg-muted text-muted-foreground text-sm font-medium transition-all hover:bg-muted/80 active:scale-[0.98] disabled:opacity-50"
                >
                    <X className="size-4" />
                    {t('account.cancel')}
                </button>
                <button
                    type="submit"
                    disabled={changeUsername.isPending || !newUsername.trim()}
                    className="flex-1 h-9 flex items-center justify-center gap-1.5 rounded-xl bg-primary text-primary-foreground text-sm font-medium transition-all hover:bg-primary/90 active:scale-[0.98] disabled:opacity-50"
                >
                    {changeUsername.isPending ? <Loader className="size-4 animate-spin" /> : <Check className="size-4" />}
                    {changeUsername.isPending ? t('account.saving') : t('account.save')}
                </button>
            </div>
        </motion.form>
    );
}

/** 修改密码的弹出表单，成功后延迟登出 */
function PasswordForm({ onClose }: { onClose: () => void }) {
    const t = useTranslations('setting');
    const { logout } = useAuth();
    const changePassword = useChangePassword();
    const [oldPassword, setOldPassword] = useState('');
    const [newPassword, setNewPassword] = useState('');
    const [confirmPassword, setConfirmPassword] = useState('');

    const handleSubmit = (e: React.FormEvent) => {
        e.preventDefault();
        if (!oldPassword) {
            toast.error(t('account.password.oldEmpty'));
            return;
        }
        if (!newPassword) {
            toast.error(t('account.password.newEmpty'));
            return;
        }
        if (newPassword !== confirmPassword) {
            toast.error(t('account.password.mismatch'));
            return;
        }
        if (newPassword.length < 6) {
            toast.error(t('account.password.tooShort'));
            return;
        }

        changePassword.mutate(
            { oldPassword, newPassword },
            {
                onSuccess: () => {
                    toast.success(t('account.password.success'));
                    onClose();
                    setTimeout(() => logout(), 1000);
                },
                onError: () => toast.error(t('account.password.failed')),
            }
        );
    };

    return (
        <motion.form
            layoutId="account-password"
            onSubmit={handleSubmit}
            transition={{ type: 'spring', stiffness: 400, damping: 30 }}
            className="absolute left-1/2 top-1/2 z-20 grid w-[min(420px,calc(100vw-2rem))] -translate-x-1/2 -translate-y-1/2 gap-2 rounded-3xl border border-border bg-card p-5"
        >
            <PasswordInput
                value={oldPassword}
                onChange={setOldPassword}
                placeholder={t('account.password.oldPlaceholder')}
                disabled={changePassword.isPending}
            />
            <PasswordInput
                value={newPassword}
                onChange={setNewPassword}
                placeholder={t('account.password.newPlaceholder')}
                disabled={changePassword.isPending}
            />
            <PasswordInput
                value={confirmPassword}
                onChange={setConfirmPassword}
                placeholder={t('account.password.confirmPlaceholder')}
                disabled={changePassword.isPending}
            />

            <div className="flex gap-2 pt-1">
                <button
                    type="button"
                    onClick={onClose}
                    disabled={changePassword.isPending}
                    className="flex-1 h-9 flex items-center justify-center gap-1.5 rounded-xl bg-muted text-muted-foreground text-sm font-medium transition-all hover:bg-muted/80 active:scale-[0.98] disabled:opacity-50"
                >
                    <X className="size-4" />
                    {t('account.cancel')}
                </button>
                <button
                    type="submit"
                    disabled={changePassword.isPending || !oldPassword || !newPassword || !confirmPassword}
                    className="flex-1 h-9 flex items-center justify-center gap-1.5 rounded-xl bg-primary text-primary-foreground text-sm font-medium transition-all hover:bg-primary/90 active:scale-[0.98] disabled:opacity-50"
                >
                    {changePassword.isPending ? <Loader className="size-4 animate-spin" /> : <Check className="size-4" />}
                    {changePassword.isPending ? t('account.saving') : t('account.password.change')}
                </button>
            </div>
        </motion.form>
    );
}

export function SettingAccount() {
    const t = useTranslations('setting');
    // 当前展开的表单, 为空时仅显示两行入口
    const [editing, setEditing] = useState<'username' | 'password' | null>(null);

    return (
        <div className="rounded-3xl border border-border bg-card p-6 space-y-5 relative">
            <h2 className="text-lg font-bold text-card-foreground flex items-center gap-2">
                <User className="h-5 w-5" />
                {t('account.title')}
            </h2>

            <AnimatePresence>
                {editing === 'username' && <UsernameForm onClose={() => setEditing(null)} />}
            </AnimatePresence>

            <AnimatePresence>
                {editing === 'password' && <PasswordForm onClose={() => setEditing(null)} />}
            </AnimatePresence>

            {/* 修改用户名 */}
            <div className="flex items-center justify-between gap-4">
                <div className="flex items-center gap-3">
                    <KeyRound className="h-5 w-5 text-muted-foreground" />
                    <span className="text-sm font-medium">{t('account.username.label')}</span>
                </div>
                <motion.button
                    type="button"
                    layoutId="account-username"
                    onClick={() => setEditing('username')}
                    disabled={editing !== null}
                    aria-label={t('account.username.label')}
                    className="flex size-8 items-center justify-center rounded-lg bg-muted/60 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground active:scale-95 disabled:opacity-50"
                >
                    <Pencil className="size-4" />
                </motion.button>
            </div>

            {/* 修改密码 */}
            <div className="flex items-center justify-between gap-4">
                <div className="flex items-center gap-3">
                    <Lock className="h-5 w-5 text-muted-foreground" />
                    <span className="text-sm font-medium">{t('account.password.label')}</span>
                </div>
                <motion.button
                    type="button"
                    layoutId="account-password"
                    onClick={() => setEditing('password')}
                    disabled={editing !== null}
                    aria-label={t('account.password.label')}
                    className="flex size-8 items-center justify-center rounded-lg bg-muted/60 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground active:scale-95 disabled:opacity-50"
                >
                    <Pencil className="size-4" />
                </motion.button>
            </div>
        </div>
    );
}
