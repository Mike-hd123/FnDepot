// Package configcheck 提供 Clash 配置的字段级校验与 YAML 文档操作。
//
// 只依赖 gopkg.in/yaml.v3，不引用任何其它 internal 子包，因此可作为叶子包被
// core（临时内核产出校验）与 subscription / configgen（生成与打补丁）共同使用，
// 不会形成依赖环。
//
// 与内核的 provider 契约区分：provider 允许 Base64 编码的 URI 列表、裸 URI 列表等
// 形态，而「主配置」必须是含 proxies / proxy-providers / proxy-groups 的 YAML 映射。
// 本包校验的是后者——两者不可混用。
//
// 文件划分：
//   - check.go  ValidateClashConfig：解析为映射 + 顶层字段存在性与类型校验
//   - document.go  Doc：保留键序的顶层字段读写与序列化
package configcheck
