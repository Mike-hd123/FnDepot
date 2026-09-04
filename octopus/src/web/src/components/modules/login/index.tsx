import { useState } from "react"
import { useTranslations } from 'use-intl'
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { useLogin } from "@/api/user"
import { useAPIKeyLogin } from "@/api/apikey"
import Logo from "@/components/modules/logo"
import { KeyRound, User } from "lucide-react"
import {
  Tabs,
  TabsList,
  TabsTrigger,
  TabsContent,
} from "@/components/ui/tabs"

type LoginMode = 'user' | 'apikey';

// LoginForm 渲染用户密码和 API Key 两种登录表单。
export function LoginForm() {
  const t = useTranslations('login')
  const [mode, setMode] = useState<LoginMode>('user')
  const [username, setUsername] = useState("")
  const [password, setPassword] = useState("")
  const [trustDevice, setTrustDevice] = useState(false) // trustDevice 表示是否请求后端签发 30 天登录凭证。
  const [apiKey, setApiKey] = useState("")
  const [error, setError] = useState<string | null>(null)

  const loginMutation = useLogin()
  const apiKeyLoginMutation = useAPIKeyLogin()

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setError(null)

    try {
      if (mode === 'user') {
        await loginMutation.mutateAsync({
          username,
          password,
          expire: trustDevice ? -1 : 86400,
        })
      } else {
        await apiKeyLoginMutation.mutateAsync(apiKey)
      }
    } catch (err: unknown) {
      const errorMessage = err instanceof Error ? err.message : t('error.generic')
      setError(errorMessage)
    }
  }

  const isPending = loginMutation.isPending || apiKeyLoginMutation.isPending

  const handleModeChange = (value: string) => {
    setMode(value as LoginMode)
    setError(null)
  }

  return (
    <div className="min-h-screen flex animate-in items-center justify-center px-6 text-foreground fade-in duration-300">
      <div className="w-full max-w-sm space-y-8">
        <header className="flex flex-col items-center gap-3">
          <Logo size={48} />
          <h1 className="text-2xl font-bold">Octopus</h1>
        </header>

        <Tabs value={mode} onValueChange={handleModeChange}>
          <TabsList className="flex w-full rounded-2xl bg-muted p-1">
            <TabsTrigger
              value="user"
              className="w-full flex items-center justify-center gap-2 py-2.5 px-4 rounded-xl text-sm font-medium transition-colors data-[state=active]:text-foreground data-[state=inactive]:text-muted-foreground data-[state=inactive]:hover:text-foreground"
            >
              <User className="w-4 h-4" />
              {t('mode.user')}
            </TabsTrigger>
            <TabsTrigger
              value="apikey"
              className="w-full flex items-center justify-center gap-2 py-2.5 px-4 rounded-xl text-sm font-medium transition-colors data-[state=active]:text-foreground data-[state=inactive]:text-muted-foreground data-[state=inactive]:hover:text-foreground"
            >
              <KeyRound className="w-4 h-4" />
              {t('mode.apikey')}
            </TabsTrigger>
          </TabsList>

          <form onSubmit={handleSubmit} className="space-y-6 pt-2">
            <div className="-mx-3 p-3 py-6">
              <TabsContent value="user" className="space-y-6" style={{ overflow: 'visible' }}>
                <Field>
                  <FieldLabel htmlFor="username">{t('username')}</FieldLabel>
                  <Input
                    id="username"
                    type="text"
                    placeholder={t('usernamePlaceholder')}
                    value={username}
                    onChange={(e) => setUsername(e.target.value)}
                    required={mode === 'user'}
                    disabled={isPending}
                  />
                </Field>
                <Field>
                  <FieldLabel htmlFor="password">{t('password')}</FieldLabel>
                  <Input
                    id="password"
                    type="password"
                    placeholder={t('passwordPlaceholder')}
                    value={password}
                    onChange={(e) => setPassword(e.target.value)}
                    required={mode === 'user'}
                    disabled={isPending}
                  />
                </Field>
                <Field orientation="horizontal" data-disabled={isPending}>
                  <Checkbox
                    id="trust-device"
                    checked={trustDevice}
                    onCheckedChange={(checked) => setTrustDevice(checked === true)}
                    disabled={isPending}
                  />
                  <FieldLabel htmlFor="trust-device" className="text-muted-foreground">
                    {t('trustDevice')}
                  </FieldLabel>
                </Field>
              </TabsContent>
              <TabsContent value="apikey" style={{ overflow: 'visible' }}>
                <Field>
                  <FieldLabel htmlFor="apikey">{t('apikey')}</FieldLabel>
                  <Input
                    id="apikey"
                    type="password"
                    placeholder={t('apikeyPlaceholder')}
                    value={apiKey}
                    onChange={(e) => setApiKey(e.target.value)}
                    required={mode === 'apikey'}
                    disabled={isPending}
                  />
                </Field>
              </TabsContent>
            </div>

            {error && <FieldDescription className="text-destructive">{error}</FieldDescription>}

            <Button type="submit" disabled={isPending} className="w-full">
              {isPending ? t('button.loading') : t('button.submit')}
            </Button>
          </form>
        </Tabs>
      </div>
    </div>
  )
}
