# FnDepot share 合规修复配方（2026-10-15 批量整改）

> 背景：审计发现 5 个应用不符合飞牛开放规范（应用数据未落 data-share、缺卸载数据保留/清除交互）。
> 本文件是唯一权威配方。**基准实现 = `emby-in-one` 1.4.4-9**（已在 NAS 实测通过），模板 = `app-template`。
> 官方依据：`/vol2/1000/workspace/fnos-dev-guide.md` 第 380-410 / 560-620 / 785-800 / 1380-1430 行 +
> `docs/fnos-open-api-integration.md`；卸载双选字段名 `wizard_delete_data` 是系统约定，**不可改名**
> （参考系统自带实现：`/var/apps/EasyTier-Web/wizard/uninstall`、`/var/apps/OpenList/wizard/uninstall`）。

## 铁律（违反任何一条即返工）

1. **只改源码目录，绝不碰运行时**：`/vol1/@appcenter/*`、`/vol1|2/@appdata/*`、`/var/apps/*` 全部只读参考。
2. **不碰 `fnpack.json`、不碰仓库根 `README.md`**（由主会话统一收口登记）。
3. **不执行 `git add/commit/push`**，不打 fpk（构建由主会话统一跑）。
4. **不重启/不停止任何在跑服务**，不改 `config/privilege` 的 run-as（本轮不动权限模型）。
5. 每个改动文件先 `cp -p <f> <f>.bak_20261015_share` 备份（模板文件除外）。
6. 脚本必须 `bash -n` 通过；JSON/wizard 必须能被解析。

## 官方规范要点（必须照做）

### A. `config/resource` 声明 data-share
```json
{
  "data-share": {
    "shares": [
      { "name": "<share名>", "permission": { "rw": ["<应用运行用户>"] } }
    ]
  }
}
```
- 声明后 fnOS **自动建目录 + 自动授 Windows ACL**，不要 `mkdir` + `chmod 777` 去替代它。
- `<share名>` 一律用 **appname 本身**（与 emby/hyatlas 现状一致）。
- 已有 `api-scope` 的文件**保留原字段**，只追加 `data-share`（WOC 就属于这种）。
- `permission.rw` 里填 `config/privilege` 的 `username`；若 privilege 是 `run-as: package/user` 无独立用户，则省略 `permission` 整块（照 emby 写法）。

### B. `resolve_share_root` 两跳解析（每个生命周期脚本各带一份，禁止 source 兄弟文件）
卸载阶段 `cmd/` 可能已被清，所以 install_callback / upgrade_callback / uninstall_callback / main **四个脚本各内联一份**，函数体逐字照抄：

```bash
resolve_share_root() {
  local _app="$1" _name="$2" _paths="$3" _c
  for _c in "/var/apps/${_app}/shares/${_name}" "${_paths%%:*}"; do
    [ -n "${_c}" ] || continue
    case "${_c}" in /vol0/*) continue ;; esac
    if [ -d "${_c}" ] || mkdir -p "${_c}" 2>/dev/null; then
      printf '%s\n' "${_c}"
      return 0
    fi
  done
  return 1
}
SHARE_ROOT="$(resolve_share_root <appname> <share名> "${TRIM_DATA_SHARE_PATHS:-}")"
```

**两个已证伪的错写法，禁止出现**：
- ❌ `dirname "${TRIM_DATA_SHARE_PATHS}"` —— 安装阶段该变量是未落卷占位路径 `/vol0/@appshare/<app>`，拼出的目录不存在也建不动，`|| true` 吞掉错误 → 日志写着"落 share 了"，数据其实在别处（hyatlas 现役就是这个写法，靠自愈 hack 兜着）。
- ❌ `${APP_DIR}/shares`（`TRIM_APPDEST` = @appcenter）—— 升级会整体替换 @appcenter，数据必丢。
- 官方文档写 `/var/apps/<app>/share/`（单数），实测系统用 **`shares/`（复数）**，以实测为准。

### C. 数据 + 配置真身都进 share
- `DATA_DIR="${SHARE_ROOT}/data"`（或应用既有子目录名，保持与现有布局一致，避免搬家搬错）。
- **配置文件真身也必须进 share**：@appcenter 每次升级整体删除（实测 Emby 1.4.4-6 的 config.yaml 被抹掉，应用瘫痪半天）。
- `@appdata`（`TRIM_PKGVAR`）只留 **日志 / PID / 临时运行文件**。
- 迁移必须**幂等 + 不覆盖**：只在 share 目标缺失/为空时 `cp -a` 老数据，源目录保留不删（留待用户或卸载清理）。写一行迁移日志到 `${TRIM_PKGVAR}/migrate.log`。

```bash
# 幂等迁移样例（老位置有东西、且新位置还没有，才拷）
for OLD in "<老数据路径>"; do
  if [ -d "$OLD" ] && [ -n "$(ls -A "$OLD" 2>/dev/null)" ]; then
    if [ -z "$(ls -A "${DATA_DIR}" 2>/dev/null)" ]; then
      cp -a "$OLD/." "${DATA_DIR}/" 2>/dev/null \
        && echo "migrated: $OLD -> ${DATA_DIR}" >> "${TRIM_PKGVAR}/migrate.log"
    fi
  fi
done
```

### D. 失败要让用户看得见
```bash
die() {
  echo "FATAL: $1" >&2
  [ -n "${TRIM_TEMP_LOGFILE:-}" ] && echo "FATAL: $1" > "${TRIM_TEMP_LOGFILE}"
  exit 1
}
```
`TRIM_TEMP_LOGFILE` 在 `main` / `install_*` / `upgrade_*` 里有效（应用中心界面可见）；
**`config_*` 和 `uninstall_*` 里不支持**，那两处只写自己的 log。

### E. 卸载双选：`wizard/uninstall`（缺失的应用新建）
```json
[
  {
    "stepTitle": "卸载应用",
    "items": [
      { "type": "tips", "helpText": "保留或删除应用数据。" },
      {
        "type": "radio",
        "field": "wizard_delete_data",
        "label": "仅卸载，保留现有数据以备将来重新安装。",
        "initValue": "false",
        "options": [
          { "label": "保留现有数据", "value": "false" },
          { "label": "清除所有应用数据（不可恢复）！！！", "value": "true" }
        ]
      }
    ]
  }
]
```

### F. `uninstall_callback` 按选择分支
```bash
DATA="${wizard_delete_data:-false}"
case "$DATA" in
  "true")
    # 只清 share 内容，保留目录本体与属主，便于将来重装复用
    # 用 find 不用 rm -rf "$D"/* —— 通配不匹配 . 开头隐藏文件，会留垃圾
    if [ -n "${SHARE_ROOT}" ] && [ -d "${SHARE_ROOT}" ]; then
      find "${SHARE_ROOT}" -mindepth 1 -maxdepth 1 -exec rm -rf {} +
    fi
    # 日志/PID 属 @appdata，一并清（配置真身已在 share，不受影响）
    [ -n "${TRIM_PKGVAR:-}" ] && [ -d "${TRIM_PKGVAR}" ] && rm -rf "${TRIM_PKGVAR}"
    ;;
  "false"|*)
    echo "保留数据（share 与 @appdata 均不动，可随时重装恢复）"
    ;;
esac
```
清数据前**必须先停进程**（调同目录 `main` stop，路径拿不到就按 PID 文件 kill）。

### G. 端口只认一个事实源
`manifest` 的 `service_port` 是唯一端口事实源。若应用配置文件里的端口优先级高于 env，
`cmd/main` 必须在启动前把 `$TRIM_SERVICE_PORT` **注入/覆盖**进应用自己的配置（幂等：先删旧再写）。

## 版本号口径
飞牛版本 = `上游版本-本地修订号`，**禁止改上游版本号**。本轮全属打包层修复，本地修订号 +1：
`manifest` 的 `version` 与 `changelog` 同步改，`build*.sh` 里若写死了版本/文件名也要同步。

## 每个应用的交付物（写盘范围）
只允许动下列路径（其余一律不碰）：
- `<app>/` 或 `<app>/src/` 或 `<app>/src/fnos*/` 下的：`manifest`、`config/resource`、`cmd/*`、`wizard/*`、`build*.sh`（仅版本/路径同步所需行）、`README.md`、`AGENT.md`（补一句本轮合规修复说明）
- 新增备份文件 `.bak_20261015_share`

## 自检清单（subagent 必须逐条实测并回报）
1. `bash -n` 全部 `cmd/*` 脚本 → 0 错。
2. `config/resource`、`wizard/uninstall`、`wizard/install` 用 `python3 -c 'import json,sys;json.load(open(sys.argv[1]))'` 能解析。
3. `grep -n 'dirname.*TRIM_DATA_SHARE_PATHS\|${APP_DIR}/shares\|/share/'` 在改过的脚本里 **0 命中**。
4. `grep -c resolve_share_root` 在 main / install_callback / upgrade_callback / uninstall_callback **各 ≥1**。
5. `wizard_delete_data` 在 `wizard/uninstall` 与 `uninstall_callback` **都出现**。
6. manifest `version` = 目标新版本号；`config/resource` 含 `data-share`。
7. 数据落点：`grep -n 'DATA_DIR\|SHARE_ROOT' cmd/main` 显示数据在 `${SHARE_ROOT}/...` 之下，**不再有 @appdata 当数据目录**。
8. 回报格式：改了哪些文件 / 每个文件改了哪几处 / 自检 8 条结果 / 该应用现存数据实际路径与迁移目标路径是否一致（重要：避免把已在 share 的数据搬走）。
