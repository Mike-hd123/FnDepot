//! tree-sitter 代码符号提取器
//!
//! 输入文件名 + 内容，输出符号列表（kind/name/start_line/end_line/signature）。
//! 非代码文件返回空列表，走原有分块逻辑。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub kind: SymbolKind,
    pub name: String,
    pub qualified_name: String,
    pub start_line: usize, // 0-indexed
    pub end_line: usize,   // 0-indexed, inclusive
    pub signature: Option<String>,
    pub docstring: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Struct,
    Interface,
    Enum,
    Variable,
    Constant,
    TypeAlias,
    Namespace,
}

impl SymbolKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SymbolKind::Function => "function",
            SymbolKind::Method => "method",
            SymbolKind::Class => "class",
            SymbolKind::Struct => "struct",
            SymbolKind::Interface => "interface",
            SymbolKind::Enum => "enum",
            SymbolKind::Variable => "variable",
            SymbolKind::Constant => "constant",
            SymbolKind::TypeAlias => "type_alias",
            SymbolKind::Namespace => "namespace",
        }
    }
}

/// 语言到 tree-sitter Language 的映射
fn get_language(ext: &str) -> Option<tree_sitter::Language> {
    match ext {
        "ts" => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        "tsx" => Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
        "js" | "jsx" => Some(tree_sitter_javascript::LANGUAGE.into()),
        "py" => Some(tree_sitter_python::LANGUAGE.into()),
        "rs" => Some(tree_sitter_rust::LANGUAGE.into()),
        "go" => Some(tree_sitter_go::LANGUAGE.into()),
        "java" => Some(tree_sitter_java::LANGUAGE.into()),
        _ => None,
    }
}

/// 判断扩展名是否支持 AST 解析
pub fn is_supported_language(ext: &str) -> bool {
    get_language(ext).is_some()
}

/// 解析代码文件，提取符号列表
pub fn extract_symbols(filename: &str, content: &str) -> Vec<Symbol> {
    let ext = filename.rsplit('.').next().unwrap_or("").to_lowercase();
    let lang = match get_language(&ext) {
        Some(l) => l,
        None => return vec![],
    };

    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&lang).is_err() {
        return vec![];
    }

    let tree = match parser.parse(content, None) {
        Some(t) => t,
        None => return vec![],
    };

    let source = content.as_bytes();
    let root = tree.root_node();
    let mut symbols = Vec::new();
    walk_node(root, source, &ext, &mut symbols, "", "");
    symbols
}

/// 递归遍历 AST 节点，提取目标符号
fn walk_node(
    node: tree_sitter::Node,
    source: &[u8],
    ext: &str,
    symbols: &mut Vec<Symbol>,
    parent_name: &str,
    parent_kind: &str,
) {
    let kind = node.kind();
    let symbol_info = match ext {
        "ts" | "tsx" | "js" | "jsx" => check_ts_js_node(&kind, &node, source),
        "py" => check_python_node(&kind, &node, source, parent_kind),
        "rs" => check_rust_node(&kind, &node, source, parent_kind),
        "go" => check_go_node(&kind, &node, source),
        "java" => check_java_node(&kind, &node, source),
        _ => None,
    };

    let mut next_parent = parent_name.to_string();
    let next_kind = kind.to_string();

    if let Some((sym_kind, name, signature)) = symbol_info {
        let qualified = if parent_name.is_empty() {
            name.clone()
        } else {
            format!("{}.{}", parent_name, name)
        };

        let start_line = node.start_position().row;
        let end_line = node.end_position().row;

        let docstring = if ext == "py" {
            extract_python_docstring(&node, source)
        } else {
            None
        };

        symbols.push(Symbol {
            kind: sym_kind,
            name: name.clone(),
            qualified_name: qualified,
            start_line,
            end_line,
            signature,
            docstring,
        });

        next_parent = name;
    }

    // 递归遍历子节点
    let child_count = node.child_count();
    for i in 0..child_count {
        if let Some(child) = node.child(i) {
            walk_node(child, source, ext, symbols, &next_parent, &next_kind);
        }
    }
}

// ─── 辅助：从 named field 提取文本 ────────────────────────

fn field_text(node: &tree_sitter::Node, field: &str, source: &[u8]) -> Option<String> {
    let child = node.child_by_field_name(field)?;
    child.utf8_text(source).ok().map(|s| s.to_string())
}

/// 提取节点第一行作为签名
fn first_line(node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
    let text = node.utf8_text(source).ok()?;
    text.lines().next().map(|s| s.trim().to_string())
}

// ─── TypeScript / JavaScript ──────────────────────────────

fn check_ts_js_node(
    kind: &str,
    node: &tree_sitter::Node,
    source: &[u8],
) -> Option<(SymbolKind, String, Option<String>)> {
    match kind {
        "function_declaration" => {
            let name = field_text(node, "name", source).unwrap_or_default();
            let sig = first_line(node, source);
            Some((SymbolKind::Function, name, sig))
        }
        "class_declaration" => {
            let name = field_text(node, "name", source).unwrap_or_default();
            Some((SymbolKind::Class, name, None))
        }
        "method_definition" => {
            let name = field_text(node, "name", source).unwrap_or_default();
            let sig = first_line(node, source);
            Some((SymbolKind::Method, name, sig))
        }
        "interface_declaration" => {
            let name = field_text(node, "name", source).unwrap_or_default();
            Some((SymbolKind::Interface, name, None))
        }
        "enum_declaration" => {
            let name = field_text(node, "name", source).unwrap_or_default();
            Some((SymbolKind::Enum, name, None))
        }
        "type_alias_declaration" => {
            let name = field_text(node, "name", source).unwrap_or_default();
            Some((SymbolKind::TypeAlias, name, None))
        }
        "lexical_declaration" | "variable_declaration" => {
            // const X = ... → 大写开头视为常量/组件
            let declarator = node.child_by_field_name("declarator")?;
            let name = field_text(&declarator, "name", source).unwrap_or_default();
            if name
                .chars()
                .next()
                .map(|c| c.is_uppercase())
                .unwrap_or(false)
            {
                return Some((SymbolKind::Constant, name, None));
            }
            None
        }
        _ => None,
    }
}

// ─── Python ───────────────────────────────────────────────

fn check_python_node(
    kind: &str,
    node: &tree_sitter::Node,
    source: &[u8],
    parent_kind: &str,
) -> Option<(SymbolKind, String, Option<String>)> {
    match kind {
        "function_definition" => {
            let name = field_text(node, "name", source).unwrap_or_default();
            let sig = first_line(node, source);
            // tree-sitter Python: class_definition → block → function_definition
            // 所以直接 parent 是 block 时，说明在 class 内部
            let sym_kind = if parent_kind == "block" {
                SymbolKind::Method
            } else {
                SymbolKind::Function
            };
            Some((sym_kind, name, sig))
        }
        "class_definition" => {
            let name = field_text(node, "name", source).unwrap_or_default();
            Some((SymbolKind::Class, name, None))
        }
        _ => None,
    }
}

// ─── Rust ─────────────────────────────────────────────────

fn check_rust_node(
    kind: &str,
    node: &tree_sitter::Node,
    source: &[u8],
    parent_kind: &str,
) -> Option<(SymbolKind, String, Option<String>)> {
    match kind {
        "function_item" => {
            let name = field_text(node, "name", source).unwrap_or_default();
            let sig = first_line(node, source);
            // tree-sitter Rust: implementation_item → declaration_list → function_item
            let sym_kind = match parent_kind {
                "declaration_list" => SymbolKind::Method,
                _ => SymbolKind::Function,
            };
            Some((sym_kind, name, sig))
        }
        "struct_item" => {
            let name = field_text(node, "name", source).unwrap_or_default();
            Some((SymbolKind::Struct, name, None))
        }
        "enum_item" => {
            let name = field_text(node, "name", source).unwrap_or_default();
            Some((SymbolKind::Enum, name, None))
        }
        "trait_item" => {
            let name = field_text(node, "name", source).unwrap_or_default();
            Some((SymbolKind::Interface, name, None))
        }
        "type_item" => {
            let name = field_text(node, "name", source).unwrap_or_default();
            Some((SymbolKind::TypeAlias, name, None))
        }
        "implementation_item" => {
            let name = field_text(node, "type", source).unwrap_or_default();
            Some((SymbolKind::Namespace, name, None))
        }
        _ => None,
    }
}

// ─── Go ───────────────────────────────────────────────────

fn check_go_node(
    kind: &str,
    node: &tree_sitter::Node,
    source: &[u8],
) -> Option<(SymbolKind, String, Option<String>)> {
    match kind {
        "function_declaration" => {
            let name = field_text(node, "name", source).unwrap_or_default();
            let sig = first_line(node, source);
            Some((SymbolKind::Function, name, sig))
        }
        "method_declaration" => {
            let name = field_text(node, "name", source).unwrap_or_default();
            let sig = first_line(node, source);
            Some((SymbolKind::Method, name, sig))
        }
        "type_spec" => {
            // type Server struct{...} → type_spec 节点有 name + type
            let name = field_text(node, "name", source).unwrap_or_default();
            let type_kind = node
                .child_by_field_name("type")
                .map(|t| t.kind().to_string());
            let sym_kind = match type_kind.as_deref() {
                Some("struct_type") => SymbolKind::Struct,
                Some("interface_type") => SymbolKind::Interface,
                _ => SymbolKind::TypeAlias,
            };
            Some((sym_kind, name, None))
        }
        _ => None,
    }
}

// ─── Java ─────────────────────────────────────────────────

fn check_java_node(
    kind: &str,
    node: &tree_sitter::Node,
    source: &[u8],
) -> Option<(SymbolKind, String, Option<String>)> {
    match kind {
        "method_declaration" | "constructor_declaration" => {
            let name = field_text(node, "name", source).unwrap_or_default();
            let sig = first_line(node, source);
            Some((SymbolKind::Method, name, sig))
        }
        "class_declaration" => {
            let name = field_text(node, "name", source).unwrap_or_default();
            Some((SymbolKind::Class, name, None))
        }
        "interface_declaration" => {
            let name = field_text(node, "name", source).unwrap_or_default();
            Some((SymbolKind::Interface, name, None))
        }
        "enum_declaration" => {
            let name = field_text(node, "name", source).unwrap_or_default();
            Some((SymbolKind::Enum, name, None))
        }
        "record_declaration" => {
            let name = field_text(node, "name", source).unwrap_or_default();
            Some((SymbolKind::Struct, name, None))
        }
        _ => None,
    }
}

// ─── Python docstring 提取 ────────────────────────────────

fn extract_python_docstring(node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
    let body = node.child_by_field_name("body")?;
    let first_stmt = body.child(0)?;
    if first_stmt.kind() == "expression_statement" {
        let string_node = first_stmt.child(0)?;
        if string_node.kind() == "string" {
            return string_node.utf8_text(source).ok().map(|s| s.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_supported() {
        assert!(is_supported_language("ts"));
        assert!(is_supported_language("tsx"));
        assert!(is_supported_language("js"));
        assert!(is_supported_language("py"));
        assert!(is_supported_language("rs"));
        assert!(is_supported_language("go"));
        assert!(is_supported_language("java"));
        assert!(!is_supported_language("rb"));
        assert!(!is_supported_language("cpp"));
    }

    #[test]
    fn test_extract_typescript_symbols() {
        let code = r#"
import { Router } from 'express';

export function handleRequest(req: Request, res: Response) {
    return res.json({ ok: true });
}

class UserController {
    async getUsers(req: Request, res: Response) {
        const users = await User.findAll();
        return res.json(users);
    }
}
"#;
        let symbols = extract_symbols("test.ts", code);
        assert!(symbols.len() >= 3);
        assert!(symbols
            .iter()
            .any(|s| s.name == "handleRequest" && s.kind == SymbolKind::Function));
        assert!(symbols
            .iter()
            .any(|s| s.name == "UserController" && s.kind == SymbolKind::Class));
        assert!(symbols
            .iter()
            .any(|s| s.name == "getUsers" && s.kind == SymbolKind::Method));
    }

    #[test]
    fn test_extract_python_symbols() {
        let code = r#"
class DataProcessor:
    def process(self, data):
        """Process the data."""
        return data.strip()

def main():
    processor = DataProcessor()
    return processor.process("hello")
"#;
        let symbols = extract_symbols("test.py", code);
        assert!(symbols
            .iter()
            .any(|s| s.name == "DataProcessor" && s.kind == SymbolKind::Class));
        assert!(symbols
            .iter()
            .any(|s| s.name == "process" && s.kind == SymbolKind::Method));
        assert!(symbols
            .iter()
            .any(|s| s.name == "main" && s.kind == SymbolKind::Function));
    }

    #[test]
    fn test_extract_rust_symbols() {
        let code = r#"
pub struct User {
    pub name: String,
}

impl User {
    pub fn new(name: String) -> Self {
        Self { name }
    }
}

fn main() {
    let user = User::new("test".into());
}
"#;
        let symbols = extract_symbols("test.rs", code);
        assert!(symbols
            .iter()
            .any(|s| s.name == "User" && s.kind == SymbolKind::Struct));
        assert!(symbols
            .iter()
            .any(|s| s.name == "new" && s.kind == SymbolKind::Method));
        assert!(symbols
            .iter()
            .any(|s| s.name == "main" && s.kind == SymbolKind::Function));
    }

    #[test]
    fn test_extract_go_symbols() {
        let code = r#"
package main

type Server struct {
    addr string
}

type Handler interface {
    Handle(req Request) error
}

func main() {
    s := Server{addr: ":8080"}
    s.run()
}

func (s *Server) run() {
    // ...
}
"#;
        let symbols = extract_symbols("test.go", code);
        assert!(symbols
            .iter()
            .any(|s| s.name == "Server" && s.kind == SymbolKind::Struct));
        assert!(symbols
            .iter()
            .any(|s| s.name == "Handler" && s.kind == SymbolKind::Interface));
        assert!(symbols
            .iter()
            .any(|s| s.name == "main" && s.kind == SymbolKind::Function));
        assert!(symbols
            .iter()
            .any(|s| s.name == "run" && s.kind == SymbolKind::Method));
    }
}
