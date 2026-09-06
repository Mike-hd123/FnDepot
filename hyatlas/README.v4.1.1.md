# hyatlas.fpk — HyAtlas(混元记忆) 安装包

## 当前发布：4.1.1-2（v4 纯 Go · B2 改造）

- 版本：**hyatlas-4.1.1-2-x86.fpk**（manifest version=4.1.1-2，源码 main@a80d3ab）
- 产物：`/vol2/1000/download/hyatlas-4.1.1-2-x86.fpk`（size 241787281，约 230MB）
- sha256 = `4a5567cf837879bce94111512db567bf9995a6deef4933840b6900a9b0c8c1e0`
- 分发：**GitHub Release 资产** `hyatlas-4.1.1-2`（230MB 超 GitHub 100MB 单文件硬限，不能进 git 跟踪；fnpack.json download_url 指向 Release）
- 上游项目：[tuancookiez-hub/HyAtlas-Memory](https://github.com/tuancookiez-hub/HyAtlas-Memory)（v4.1.1 + B2 commit a80d3ab，Apache-2.0）
- 源码：本目录 `src/`（v4 Go 工程，FnDepot 单仓；B2 独立仓历史见 fork Mike-hd123/HyAtlas-Memory commit a80d3ab）

**B2 改造要点（4.1.1-2）**：

1. **内置 ONNX Runtime int8 向量引擎**（bge-large-zh 1024d）：模型 + ORT 随包分发，**移除 llama.cpp 18080 依赖**；
2. **dashboard 全量汉化** + 移动端响应式（汉堡菜单 + 抽屉侧边栏）；
3. 性能：add p50 22ms、search p50 ~20ms；cos vs llama.cpp q8_0 min=0.979 / mean=0.983；常驻内存 ~450MB。

**v4 vs v3（纯 Go 重写带来的变化）**：

| 维度 | v3.5.0 (Python) | v4.1.1-2 (Go) |
|------|-----------------|---------------|
| 运行时 | venv site-packages (~100MB) | 单 Go 二进制 (~13MB) + 内置模型 (~330MB) |
| embedding | 外部 llama.cpp 18080 (bge-large-zh q8_0) | **内置 ORT int8，随包分发，无外部依赖** |
| 端口 | 19527(API) + 8765(dash) | 19528 单端口 (loopback) |
| 存储 | zvec/sqlite | chromem-go 目录型 (go-data/) |
| dashboard | 外部文件 + site-patches 汉化 | go:embed 内嵌，**出厂即全量汉化 + 移动端响应式** |
| 入口 | 面板直连端口 | fnOS 网关 socket /app/hyatlas |

**配置预设**（cmd/main 写死）：

- `HYATLAS_GO_HOST=127.0.0.1` `HYATLAS_GO_PORT=19528` `HYATLAS_GO_DATA=/vol1/@appshare/hyatlas/go-data`
- LLM 走 Octopus（`http://127.0.0.1:8081/v1`），key 启动时从现役 v3 `config/hy_memory.json` 自动迁移
- embedding 走本机 ORT CPU EP（模型位于 app/models/，`HYATLAS_EMBED_MODEL=bge-large-zh`）

**sidecar 关键点**：手机端 /app/hyatlas socket 型入口（对齐 octopus v8），剥前缀 HTTP 反代 + 响应体 JS 改写 `'/api/*' → '/app/hyatlas/api/*'`，同源同 socket，桌面 iframe 经 /app/hyatlas 打开。

**数据**：复用 `/var/apps/hyatlas/shares`(→`/vol1/@appshare/hyatlas`)，v4 数据在 `go-data/` 子目录，与 v3 数据互不覆盖。

## 源码布局（FnDepot/hyatlas/src/）

- 仓库根 = **v4 Go 工程**：`server.go`（HTTP + 路由）、`store.go`（chromem-go 存储）、`embed.go`（Embedder 接口 + OpenAI 兼容 embedder）、`bge/`（内置 ORT bge-large-zh int8 引擎 + tokenizer）、`dashboard/`（dist 产物，go:embed 内嵌）、`graph/`（LLM 知识图谱）、`llm.go`、`memory/`、`plugins/hy_memory/`（Hermes 插件）
- `fnos-native/` = **fpk 打包层**：`build-fpk.sh`（可复现构建）、`cmd/`（安装/启停脚本，main 内置配置预设）、`config/`、`wizard/`、`manifest`、`gateway/gateway_proxy.py`（socket sidecar）、`ui/`（socket 型入口 config + 图标）、`bin/hyatlas-go-linux-amd64`（权威二进制归档，sha256 `f6100c564dbdc37e040c487b5beb3efa1ad20594754ba5b89c1384f0b41d2c2d`）
- **模型三件套不入库**（~355MB 超 GitHub 100MB 单文件限制）：`model_int8.onnx`（sha256 `8a3f371a7e535e25d3d5a0ff0c0501a605ef0b62577800d2bf4b1fc76d6cbcf1`）、`libonnxruntime.so`（`99458e9d185dfa1a9b5f6510790ede3bedc25dea378adb904ce292b517eeaecf`）、`tokenizer.json`（`7dfbf1966ebf99d471c3796e9b457329d2b2182b817e144f1e904b957745c839`）。构建时按 `MODEL_SRC` → `/tmp/b2-models` → 从现有 fpk 提取 的顺序解析，sha256 断言防漂移。
- v3 Python 源码**已删除**（v4 直接覆盖，不做 legacy 保留；v3 历史见上游 tag v3.5.0 与 2.0.1 包）

## 构建与复现

```bash
bash src/fnos-native/build-fpk.sh              # 默认输出 /vol2/1000/download/hyatlas-4.1.1-2-x86.fpk
bash src/fnos-native/build-fpk.sh /tmp/out.fpk # 指定输出
```

- **字节级复现已验证**：构建产物 sha256 与已发布产物完全一致（`4a5567cf...`，双输入路径均实测）。
- 从源码重编译二进制：`cd src && go build -o hyatlas-go .`（需 go ≥1.26，工具链 `/tmp/go-b2`，go1.26.8）。Go 链接产物不可字节级复现，权威二进制以 `fnos-native/bin/` 归档为准。
- `fnpack build -d .` 仅作结构校验用（fnpack 1.2.4 会重压 app.tgz 改写字节，不用于发布产物）。
- 已知怪癖：manifest 内 `checksum` 字段为 PLACEHOLDER_CHECKSUM（原打包脚本替换发生在 tar 组装之后未生效；fnOS 安装器不校验该字段，实测安装/升级正常）。字节级复现按占位符原件直拼。
- `go vet ./...` 报 server_test.go 一处签名滞后（上游自带），`go build` 不受影响。

## 历史版本

### 2.0.1（v3.5.0 Python 版，已装用户走此包）

- 产物：本目录 `hyatlas.fpk`（sha256 `db168d1d6b08c982c93ae0c6b9f756d1bd6c56ef4dcaac3973d6d47e20a9fdb0`，size 104695185）
  注：fnpack.json 2.0.1 条目 sha256 `7b3be82b...` / size 104695175 与现文件不一致（历史包重打包未回填），保留原条目未动，待后续一并修正。
- v3 源码已从本仓库移除（v4 直接覆盖）。
