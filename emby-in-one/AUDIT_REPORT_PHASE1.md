# Emby-In-One Phase 1 安全审计报告

**审计范围**: /vol2/1000/workspace/FnDepot/emby-in-one
**审计日期**: 2026-09-29
**审计性质**: 只读审计（未修改任何源码或打包文件）
**版本**: 1.4.4-4

---

## 一、硬编码 IP 地址

**结论: 无安全风险**

emby-in-one 源码及二进制中未发现任何硬编码内网 IP 地址。

扫描覆盖:
- Go 源码 (src/internal/backend/)
- Python gateway sidecar (app/gateway/emby-gateway.py)
- cmd/ 脚本 (main, install_callback, upgrade_callback 等)
- config/config.yaml
- 二进制 (strings 扫描)
- build.sh, install.sh, release-install.sh

唯一匹配:
- ssrf_test.go 中的测试向量 (127.0.0.1, 10.0.0.1, 172.16.0.1) — 标准 SSRF 防护测试用例，非生产硬编码

横向对比: wechat-on-cloud 和 vikunja 有 192.168.x 硬编码（woc-lan 网段 / publicurl 历史 bug），但这两个是不同应用，不在本次审计范围。

---

## 二、明文密钥 / API Key / 凭证

**结论: 存在 3 处明文敏感信息，全部集中在 config/config.yaml**

### 2.1 admin 默认密码（明文）

**文件**: app/config/config.yaml
**位置**: admin.password 字段
**值**: [REDACTED]（明文，非哈希）
**打包状态**: 随 app.tgz 打入 FPK，用户安装后落入运行时 config 目录

启动时 `ensureAdminPasswordHashed()` (auth_manager.go:96) 会检测明文密码并自动 scrypt 哈希化，首次启动后 config.yaml 中的 password 字段被替换为 `salt:derived_key` 格式（32:128 hex）。但初始分发物中仍是明文。

### 2.2 上游 Emby 服务器 URL

**文件**: app/config/config.yaml
**位置**: upstream[0].url
**值**: [REDACTED]（https://etf.entertang.work/emby）
**风险**: 暴露上游服务地址

### 2.3 上游 Emby API Key

**文件**: app/config/config.yaml
**位置**: upstream[0].apiKey
**值**: [REDACTED]
**风险**: 明文 API Key，任何拿到 FPK 的人均可提取

### 2.4 cmd/main 硬编码 admin 凭证

**文件**: cmd/main (行 122-123)
**内容**: EIO_ADMIN_USER="admin" / EIO_ADMIN_PASS="admin" 作为环境变量注入 gateway sidecar
**用途**: sidecar 自动重登机制 — 后端重启致 token 失效时，sidecar 用此凭证自动重登
**风险**: 凭证以明文环境变量形式传递，ps/proc 可见

### 2.5 密码哈希机制（缓解措施评估）

emby-in-one 使用自实现的 scrypt 变体（scrypt_local.go），非标准库 golang.org/x/crypto/scrypt:
- 参数: N=16384, r=8, p=1, keyLen=64
- 格式: `salt(16bytes hex):derived(64bytes hex)` = 32:128 hex
- 有 RFC 7914 测试向量验证（scrypt_local_test.go）
- 有恒定时间比较（spendVerifyTime / dummyPasswordHash 防时序攻击）
- VerifyPassword 拒绝明文存储（auth_test.go: TestVerifyPasswordRejectsPlaintext）

评估: 哈希实现质量良好，但自实现 scrypt 存在审计风险（不如使用标准库）。

### 2.6 二进制扫描

Go 二进制 (app/emby-in-one) strings 扫描:
- entertang: 0 hits
- cdd32: 0 hits
- etf.: 0 hits
- 192.168: 0 hits
- apiKey: 0 hits
- admin/admin: 1 hit（HTML 路由 /admin/admin.html，非凭证）

**结论: 敏感信息未编译进二进制，全部集中在 config.yaml 和 cmd/main 脚本中。**

---

## 三、图标 Not Found 根因分析

**结论: 源码层面未发现致因；当前版本图标打包正确**

### 3.1 图标文件清单

| 文件 | 尺寸 | 格式 | 位置 | 状态 |
|------|------|------|------|------|
| ICON.PNG | 64x64 | RGBA PNG | FPK 顶层 | ✅ 存在 |
| ICON_256.PNG | 256x256 | RGBA PNG | FPK 顶层 | ✅ 存在 |
| ui/images/icon-64.png | 64x64 | RGBA PNG | app.tgz 内 | ✅ 存在 |
| ui/images/icon-256.png | 256x256 | RGBA PNG | app.tgz 内 | ✅ 存在 |

图标内容: 绿色 (#52B54B) + 白色简单图案，非空白/透明。

### 3.2 打包链路

build.sh 流程:
1. `mkdir -p app/ui/images`
2. `cp ui/config app/ui/`
3. `cp ui/images/icon-64.png app/ui/images/`
4. `cp ui/images/icon-256.png app/ui/images/`
5. `fnpack build -d "$HERE"` — fnpack 自动将 ICON.PNG/ICON_256.PNG 放 FPK 顶层，app/ 内容打入 app.tgz

build.sh 自检:
- `grep -q '^ui/config' app.tgz` ✅
- `grep -q '"gatewaySocket"' ui/config` ✅

### 3.3 ui/config 图标引用

```json
"icon": "images/icon-{0}.png"
```
`{0}` 是 fnOS 桌面模板占位符，运行时替换为尺寸数字（64/256）。文件 `icon-64.png` 和 `icon-256.png` 均存在于 app.tgz 的 `ui/images/` 目录。

manifest 关键字段:
- `desktop_uidir = ui` — 告知 fnOS UI 资源在 ui/ 目录
- `desktop_applaunchname = emby-in-one.panel` — 桌面面板名

### 3.4 历史 changelog 线索

manifest changelog: "1.4.4-2：...换图标。"
→ 1.4.4-2 版本已更换图标。如果 "Icon Not Found" 是历史问题，在 1.4.4-2 已修复。

### 3.5 运行时状态

应用当前未安装（/vol1/@appdata, @appconf, @apphome, @appmeta 均为空），无法验证运行时图标解析。但从源码和 FPK 结构看，图标打包链路完整无误。

### 3.6 潜在风险点

- manifest 无 icon 字段（与其他 FnDepot 应用一致，fnOS 使用 FPK 顶层 ICON.PNG，非 manifest 字段）
- build.sh 对图标文件只做提示（`echo "⚠️ 提示: 无 $f"`）不做强制失败（`exit 1`）。如果图标文件缺失，build 仍会产出 FPK，但图标会丢失。建议改为强制检查。

---

## 四、Admin 默认凭证

**结论: 存在默认凭证 admin/admin，多处明文硬编码**

### 4.1 config.yaml 默认凭证

```yaml
admin:
  username: 'admin'
  password: 'admin'  # [REDACTED]
```

- 随 FPK 分发，安装后落入运行时
- 首次启动自动 scrypt 哈希化（ensureAdminPasswordHashed）
- 但 config.yaml 初始值仍是明文 admin

### 4.2 cmd/main 环境变量注入

```bash
EIO_ADMIN_USER="admin"
EIO_ADMIN_PASS="admin"
```

- 注入 gateway sidecar 进程环境
- 用于 sidecar 自动重登机制（后端重启 token 失效时自动重登）
- 明文环境变量，ps/proc 可见

### 4.3 二进制中的 /admin/admin.html

二进制 strings 中发现 `/admin/admin.html` — 这是 HTML 路由路径，非凭证。

### 4.4 认证安全机制

- scrypt 哈希（N=16384, r=8, p=1）
- 恒定时间比较（dummyPasswordHash / spendVerifyTime 防时序攻击）
- 登录限流（auth_manager.go: "Login rate limited: ip=%s"）
- token 不设过期，靠显式登出/重置/吊销移除
- token 落盘 tokens.json 持久化

### 4.5 风险评估

| 风险 | 严重度 | 说明 |
|------|--------|------|
| 默认 admin/admin 凭证 | 中 | FPK 内明文，但首次启动自动哈希化 |
| cmd/main 硬编码 admin/admin | 中 | 环境变量明文，ps 可读 |
| 无强制改密提示 | 低 | 用户可能不知道要改密码 |
| config.yaml 含上游 apiKey | 高 | 明文 API Key 随 FPK 分发 |

---

## 五、横向对比

| 应用 | 硬编码 IP | 明文密钥 | 默认凭证 |
|------|-----------|----------|----------|
| emby-in-one | 无 | config.yaml (apiKey/url) + cmd/main (admin) | admin/admin |
| hyatlas | 无 | 无 | (未审计) |
| octopus | 无 | 无 | (未审计) |
| ezbookkeeping | 无 | 无 | (未审计) |
| wechat-on-cloud | 192.168.5.x (设计如此) | 无 | (未审计) |
| vikunja | 192.168.5.2 (历史 bug 已修) | 无 | (未审计) |

---

## 六、修复建议（Phase 2 参考，本次不执行）

1. **config.yaml 去敏**: apiKey 和 url 改为占位符/空值，安装时由用户通过安装向导填写
2. **cmd/main 凭证**: 改为从 config.yaml 读取而非硬编码环境变量，或使用 fd 传递
3. **build.sh 图标检查**: 图标文件缺失时 exit 1 而非仅 echo 提示
4. **默认密码**: 安装向导增加强制改密步骤，或生成随机初始密码
5. **scrypt**: 考虑迁移到 golang.org/x/crypto/scrypt 标准库（当前为自实现）

---

**审计人**: 灵犀 (Hermes Agent)
**任务**: t_2c44a4ae
