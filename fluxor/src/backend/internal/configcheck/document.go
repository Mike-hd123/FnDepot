package configcheck

import (
	"bytes"
	"fmt"
	"strconv"
	"strings"
	"unicode/utf8"

	"gopkg.in/yaml.v3"
)

// Doc 包装一份已解析的 Clash 配置，支持按键读写顶层字段。
//
// 采用 yaml.Node 而非 map[string]any，是为了保留键序：map 序列化时会按字母序
// 重排键，使生成的 config.yaml 结构与模板顺序不一致（可读性明显下降）。
type Doc struct {
	top *yaml.Node // 顶层映射节点（Kind=MappingNode）
}

// ParseDoc 解析内容为可编辑文档，要求顶层为映射但不校验 Clash 字段。
//
// 用于「骨架/模板」场景：模板本身可能尚无 proxies 等字段，需先解析再注入。
// 需要校验完整性的场景（如下载内容）应改用 ValidateClashConfig 或 ParseValidatedDoc。
func ParseDoc(content []byte) (*Doc, error) {
	var root yaml.Node
	if err := yaml.Unmarshal(content, &root); err != nil {
		return nil, fmt.Errorf("解析 YAML 失败: %s", reasonLine(err.Error()))
	}
	if len(root.Content) == 0 || root.Content[0].Kind != yaml.MappingNode {
		return nil, fmt.Errorf("顶层应为映射")
	}
	return &Doc{top: root.Content[0]}, nil
}

// ParseValidatedDoc 先做字段级校验，再解析为可编辑文档。
//
// 用于「外部输入」场景（订阅下载内容、用户可编辑文件）：内容必须是合法的
// Clash 配置，否则返回错误。
func ParseValidatedDoc(content []byte) (*Doc, error) {
	if err := ValidateClashConfig(content); err != nil {
		return nil, err
	}
	return ParseDoc(content)
}

// ParseMappingDoc 解析内容为映射文档；顶层不是映射时返回空文档而非错误。
//
// 用于「可编辑文件」场景：文件可能被用户改坏或为空，此时应产出可继续编辑的
// 文档，而不是让整个保存流程失败。
func ParseMappingDoc(content []byte) *Doc {
	var root yaml.Node
	if err := yaml.Unmarshal(content, &root); err != nil {
		return NewDoc()
	}
	if len(root.Content) == 0 || root.Content[0].Kind != yaml.MappingNode {
		return NewDoc()
	}
	return &Doc{top: root.Content[0]}
}

// NewDoc 创建空文档。
func NewDoc() *Doc {
	return &Doc{top: &yaml.Node{Kind: yaml.MappingNode, Tag: "!!map"}}
}

// Get 返回顶层键的值节点；不存在时返回 nil。
func (d *Doc) Get(key string) *yaml.Node {
	if idx, ok := d.find(key); ok {
		return d.top.Content[idx+1]
	}
	return nil
}

// Set 设置顶层键的值。键不存在时追加到末尾。
func (d *Doc) Set(key string, value any) error {
	valueNode, err := encodeValue(value)
	if err != nil {
		return err
	}
	if idx, ok := d.find(key); ok {
		d.top.Content[idx+1] = valueNode
		return nil
	}
	d.top.Content = append(d.top.Content,
		&yaml.Node{Kind: yaml.ScalarNode, Tag: "!!str", Value: key},
		valueNode)
	return nil
}

// SetNode 以现成节点设置顶层键（用于跨文档搬运子树，如 DNS 块）。
func (d *Doc) SetNode(key string, value *yaml.Node) {
	if idx, ok := d.find(key); ok {
		d.top.Content[idx+1] = value
		return
	}
	d.top.Content = append(d.top.Content,
		&yaml.Node{Kind: yaml.ScalarNode, Tag: "!!str", Value: key},
		value)
}

// Delete 删除顶层键，返回是否确实删除了。
func (d *Doc) Delete(key string) bool {
	idx, ok := d.find(key)
	if !ok {
		return false
	}
	d.top.Content = append(d.top.Content[:idx], d.top.Content[idx+2:]...)
	return true
}

// Bytes 序列化回 YAML 文本（2 空格缩进，与模板风格一致）。
//
// yaml.v3 会把 emoji 等 astral 平面字符输出为 \U0001F680 转义（语义等价，内核可
// 正常解析），但 Fluxor 的代理组名几乎都含 emoji，转义后配置可读性明显下降，
// 故这里统一还原为原字符。
func (d *Doc) Bytes() ([]byte, error) {
	var buf bytes.Buffer
	enc := yaml.NewEncoder(&buf)
	enc.SetIndent(2)
	if err := enc.Encode(d.top); err != nil {
		return nil, fmt.Errorf("序列化 YAML 失败: %w", err)
	}
	if err := enc.Close(); err != nil {
		return nil, fmt.Errorf("序列化 YAML 失败: %w", err)
	}
	return []byte(unescapeAstral(buf.String())), nil
}

// find 返回键在映射 Content 中的下标（valueNode 位于该下标 +1）。
// Content 以 [key0, value0, key1, value1, ...] 平铺存储。
func (d *Doc) find(key string) (int, bool) {
	for i := 0; i+1 < len(d.top.Content); i += 2 {
		if strings.EqualFold(d.top.Content[i].Value, key) {
			return i, true
		}
	}
	return 0, false
}

// encodeValue 把任意值编码为 YAML 节点。
func encodeValue(value any) (*yaml.Node, error) {
	var node yaml.Node
	if err := node.Encode(value); err != nil {
		return nil, fmt.Errorf("编码 YAML 值失败: %w", err)
	}
	return &node, nil
}

// unescapeAstral 把 yaml.v3 为 astral 平面字符（4 字节 UTF-8，如 emoji）生成的
// \Uxxxxxxxx 转义还原为原字符。
//
// 仅处理码点 >= 0x10000 的转义：BMP 内的 \uXXXX 保留原样，避免把其它合法转义
// （含 yaml.v3 用于特殊字符的标准转义）误还原而破坏语义。
func unescapeAstral(s string) string {
	if !strings.Contains(s, `\U`) {
		return s
	}
	var b strings.Builder
	b.Grow(len(s))
	for i := 0; i < len(s); i++ {
		// \U0001F680 共 10 字节，下面要切 s[i+2:i+10]，故必须保证 i+10 <= len(s)。
		// 此前守卫写作 i+8 >= len(s)（比切片上界少 2 字节），当 \U 序列紧贴文本
		// 末尾时 s[i+2:i+10] 会越界 panic；而调用方（订阅下载、配置生成的
		// goroutine）没有 recover，会直接终止整个进程。
		if s[i] != '\\' || i+10 > len(s) || s[i+1] != 'U' {
			b.WriteByte(s[i])
			continue
		}
		cp, err := strconv.ParseUint(s[i+2:i+10], 16, 32)
		r := rune(cp)
		if err != nil || cp < 0x10000 || !utf8.ValidRune(r) {
			b.WriteByte(s[i])
			continue
		}
		b.WriteRune(r)
		i += 9
	}
	return b.String()
}
