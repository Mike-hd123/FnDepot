# dsh-plugins — DSH 侧插件源码（受版本控制的那份）

这里放**DeepSeek Harness 插件的源**，与仓库里的 fnOS 应用（`hyatlas/`、`octopus/` …）是两条线：
**这些目录不进 `fnpack.json`、不打 fpk、不受应用中心管理**，只是把过去"只在运行时目录里、只有 .bak"
的活代码纳入 git，让每次改动都有 diff、可回滚。

## 现役插件

| 目录 | 插件 | 现场路径（真正的加载点） | 装载方式 |
|---|---|---|---|
| `hy-memory/` | 混元记忆接入（7 个工具 + 每轮自动召回 + 回合落库 + USER.md/MEMORY.md 常驻注入） | `/vol1/@appshare/Harness/hy-memory` | **link 安装**：DSH 启动时把该目录读进内存 |

⚠️ **link 安装 = 改完必须重启 Harness 才生效**（改 `index.js` / `cordis.patch.yml` 都是）。
判断是否已生效：看进程启动时间晚于文件 mtime（`ps -o lstart -p $(pgrep -f 'dsh.*web' | head -1)`）。

## 怎么用（默认现场是活的，仓库是权威源）

```bash
# 现场改完 → 拉回仓库核对/提交（只有 sha 不一致才动，全一致就打印 SAME）
bash dsh-plugins/sync.sh hy-memory pull
git -C /vol2/1000/workspace/FnDepot diff --stat dsh-plugins

# 仓库这份 → 推回现场（回滚、或换机器重装用；覆盖前自动给现场留 .bak_<时间戳>）
bash dsh-plugins/sync.sh hy-memory push     # 推完必须重启 Harness 才加载
```

新插件在这个脚本的 `case` 里登记一行现场路径即可；`.bak_*` 一律双向排除，不进仓库也不回写现场。

两个脚本都遵循本机纪律：**只 `cp -a` 不删除、覆盖前自动 `*.bak_<时间戳>` 留底、逐字节 sha 核对**，
不一致就报错停手，绝不静默覆盖。

## 秘密纪律

- 这些文件里**只允许出现环境变量名**（`HYATLAS_*`、`OCTOPUS_API_KEY`、`TOKENRHYTHM_KEY` 之类），
  值一律在 `~/.dsh/credentials-vault/hermes.env` 或应用自己的 env 文件（0600）里。
- 每次入库前扫一遍字面量：

```bash
grep -rnE "sk-[A-Za-z0-9]{16,}|ghp_[A-Za-z0-9]{20,}|(key|token|secret|password)[\"']?[:=][[:space:]]*[\"']?[A-Za-z0-9+/_-]{20,}" dsh-plugins || echo "无字面密钥 ✓"
```

## 版本号

`package.json` 的 `version` **不自动升**（本机规则：版本号听指令）。功能改动写进 commit message
与 `README.md` 的 changelog 段即可。
