# HyAtlas(混元记忆)

| | |
|---|---|
| 当前发布 | **4.1.1-2**（v4 纯 Go · B2 改造） |
| 市场条目 | `fnpack.json` → `apps.hyatlas.releases` |
| 包说明 | [README.v4.1.1.md](./README.v4.1.1.md) |
| 源码 | `src/`（v4 Go 工程 + `src/fnos-native/` 打包层） |
| 构建复现 | `bash src/fnos-native/build-fpk.sh`（字节级复现已验证） |

- v4 内置 ONNX Runtime int8 向量引擎（bge-large-zh 1024d），去 llama.cpp 18080 依赖；dashboard 汉化 + 移动端响应式；端口 19528。
- 固定名 `hyatlas.fpk` 为 2.0.1（v3.5.0 Python）历史包，保留供已装用户对照，不再更新。
- v3 源码已删除（v4 直接覆盖，用户指令；v3 历史见上游 tag v3.5.0）。
