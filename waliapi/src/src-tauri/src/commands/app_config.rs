use crate::db::repository::Repository;
use crate::AppState;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

// ── 数据结构 ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub label: String,
    pub icon: String,
    pub description: String,
    pub config_path: String,
    pub config_format: String,
    pub available: bool,
    pub applied: bool,
    pub download_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigContent {
    pub exists: bool,
    pub content: String,
    pub error: Option<String>,
}

// ── 应用定义 ──

struct AppDef {
    name: &'static str,
    label: &'static str,
    icon: &'static str,
    description: &'static str,
    config_format: &'static str,
    download_url: &'static str,
    config_dir_fn: fn() -> PathBuf,
    config_file: &'static str,
    check_installed_fn: fn(&PathBuf) -> bool,
}

fn home_dir() -> PathBuf {
    if let Ok(path) = std::env::var("WALIAPI_TARGET_HOME") {
        let path = path.trim();
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

const APPS: &[AppDef] = &[
    AppDef {
        name: "claude-code",
        label: "Claude Code",
        icon: "terminal",
        description: "Anthropic 的命令行 AI 编程助手，读取 ~/.claude/settings.json 中的 env 配置",
        config_format: "JSON (~/.claude/settings.json)",
        download_url: "https://docs.anthropic.com/en/docs/claude-code/overview",
        config_dir_fn: || home_dir().join(".claude"),
        config_file: "settings.json",
        check_installed_fn: |dir| dir.exists() || home_dir().join(".claude.json").exists(),
    },
    AppDef {
        name: "codex",
        label: "Codex CLI",
        icon: "code",
        description: "OpenAI Codex 命令行工具，读取 ~/.codex/auth.json 和 config.toml",
        config_format: "JSON + TOML (~/.codex/)",
        download_url: "https://github.com/openai/codex",
        config_dir_fn: || home_dir().join(".codex"),
        config_file: "config.toml",
        check_installed_fn: |dir| dir.exists(),
    },
    AppDef {
        name: "gemini-cli",
        label: "Gemini CLI",
        icon: "boxes",
        description: "Google Gemini 命令行工具，读取 ~/.gemini/.env 和 settings.json",
        config_format: "ENV + JSON (~/.gemini/)",
        download_url: "https://github.com/google-gemini/gemini-cli",
        config_dir_fn: || home_dir().join(".gemini"),
        config_file: ".env",
        check_installed_fn: |dir| dir.exists(),
    },
    AppDef {
        name: "claude-desktop",
        label: "Claude Desktop",
        icon: "sparkles",
        description: "Anthropic 桌面应用，读取 claude_desktop_config.json",
        config_format: "JSON (claude_desktop_config.json)",
        download_url: "https://claude.ai/download",
        config_dir_fn: || {
            #[cfg(target_os = "macos")]
            {
                home_dir().join("Library/Application Support/Claude")
            }
            #[cfg(target_os = "windows")]
            {
                home_dir().join("AppData/Roaming/Claude")
            }
            #[cfg(target_os = "linux")]
            {
                home_dir().join(".config/Claude")
            }
        },
        config_file: "claude_desktop_config.json",
        check_installed_fn: |dir| dir.exists(),
    },
    AppDef {
        name: "opencode",
        label: "OpenCode",
        icon: "wrench",
        description: "开源 AI 编程工具，读取 opencode.json 中的 provider 配置",
        config_format: "JSON (~/.config/opencode/opencode.json)",
        download_url: "https://opencode.ai",
        config_dir_fn: || home_dir().join(".config/opencode"),
        config_file: "opencode.json",
        check_installed_fn: |dir| dir.exists(),
    },
    AppDef {
        name: "openclaw",
        label: "OpenClaw",
        icon: "bot",
        description: "开源 Agent 框架，读取配置文件中的 provider 段",
        config_format: "JSON (~/.qclaw/)",
        download_url: "https://openclaw.ai",
        config_dir_fn: || home_dir().join(".qclaw"),
        config_file: "config.json",
        check_installed_fn: |dir| dir.exists(),
    },
    AppDef {
        name: "hermes",
        label: "Hermes Agent",
        icon: "code",
        description: "Hermes Agent 框架，读取配置文件中的 custom_providers 段",
        config_format: "TOML/JSON (Hermes config)",
        download_url: "https://github.com/openai/hermes",
        config_dir_fn: || home_dir().join(".hermes"),
        config_file: "config.json",
        check_installed_fn: |dir| dir.exists(),
    },
    AppDef {
        name: "walicode",
        label: "WaLiCode",
        icon: "code",
        description: "AI Coding Assistant，写入 ai_settings.json 中的 provider 和 apiKey 配置",
        config_format: "JSON (~/Library/Application Support/WaLiCode/ai_settings.json)",
        download_url: "https://walicode.xiaofuge.cn/",
        #[cfg(target_os = "macos")]
        config_dir_fn: || home_dir().join("Library/Application Support/WaLiCode"),
        #[cfg(target_os = "windows")]
        config_dir_fn: || home_dir().join("AppData/Roaming/WaLiCode"),
        #[cfg(target_os = "linux")]
        config_dir_fn: || home_dir().join(".config/walicode"),
        config_file: "ai_settings.json",
        check_installed_fn: |dir| dir.exists(),
    },
];

// ── 原子写入 ──

fn atomic_write(path: &PathBuf, data: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    let tmp = path.with_extension(format!(
        "tmp.{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    fs::write(&tmp, data).map_err(|e| format!("写入临时文件失败: {e}"))?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("替换文件失败: {e}")
    })?;
    Ok(())
}

fn read_json_file<T: serde::de::DeserializeOwned>(path: &PathBuf) -> Result<T, String> {
    let content = fs::read_to_string(path).map_err(|e| format!("读取文件失败: {e}"))?;
    serde_json::from_str(&content).map_err(|e| format!("解析 JSON 失败: {e}"))
}

fn write_json_file<T: Serialize>(path: &PathBuf, data: &T) -> Result<(), String> {
    let json = to_pretty_json(data).map_err(|e| format!("序列化 JSON 失败: {e}"))?;
    atomic_write(path, json.as_bytes())
}

/// 自定义 JSON pretty printer，不转义 non-ASCII 字符
fn to_pretty_json<T: Serialize>(data: &T) -> Result<String, String> {
    let value = serde_json::to_value(data).map_err(|e| format!("{e}"))?;
    let mut out = String::new();
    write_value(&mut out, &value, 0);
    Ok(out)
}

fn write_indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn write_value(out: &mut String, v: &serde_json::Value, depth: usize) {
    match v {
        serde_json::Value::Null => out.push_str("null"),
        serde_json::Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        serde_json::Value::Number(n) => out.push_str(&n.to_string()),
        serde_json::Value::String(s) => write_json_string(out, s),
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                out.push_str("[]");
            } else {
                out.push('[');
                for (i, item) in arr.iter().enumerate() {
                    out.push('\n');
                    write_indent(out, depth + 1);
                    write_value(out, item, depth + 1);
                    if i < arr.len() - 1 {
                        out.push(',');
                    }
                }
                out.push('\n');
                write_indent(out, depth);
                out.push(']');
            }
        }
        serde_json::Value::Object(obj) => {
            if obj.is_empty() {
                out.push_str("{}");
            } else {
                out.push('{');
                let len = obj.len();
                for (i, (k, val)) in obj.iter().enumerate() {
                    out.push('\n');
                    write_indent(out, depth + 1);
                    write_json_string(out, k);
                    out.push_str(": ");
                    write_value(out, val, depth + 1);
                    if i < len - 1 {
                        out.push(',');
                    }
                }
                out.push('\n');
                write_indent(out, depth);
                out.push('}');
            }
        }
    }
}

/// 写入 JSON 字符串，只转义必要的控制字符，保留 non-ASCII 原文
fn write_json_string(out: &mut String, s: &str) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if c.is_control() => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c), // 非 ASCII 字符（中文等）直接保留
        }
    }
    out.push('"');
}

// ── 备份与恢复 ──

fn backup_path(config_path: &PathBuf) -> PathBuf {
    let mut name = config_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    name.push_str(".waliapi-backup");
    config_path.with_file_name(name)
}

fn backup_config(config_path: &PathBuf) -> Result<(), String> {
    if config_path.exists() {
        let content = fs::read(config_path).map_err(|e| format!("读取配置失败: {e}"))?;
        atomic_write(&backup_path(config_path), &content)?;
    }
    Ok(())
}

fn restore_config(config_path: &PathBuf) -> Result<(), String> {
    let backup = backup_path(config_path);
    if backup.exists() {
        let content = fs::read(&backup).map_err(|e| format!("读取备份失败: {e}"))?;
        atomic_write(config_path, &content)?;
        let _ = fs::remove_file(&backup);
        Ok(())
    } else {
        Err("没有找到备份文件".to_string())
    }
}

// ── 获取 WaLiAPI 网关信息 ──

async fn get_waliapi_url(state: &Arc<AppState>) -> String {
    if let Ok(public_url) = std::env::var("WALIAPI_PUBLIC_URL") {
        let public_url = public_url.trim().trim_end_matches('/');
        if !public_url.is_empty() {
            return public_url.to_string();
        }
    }
    let port = *state.server_port.read().await;
    format!("http://127.0.0.1:{}", port)
}

#[allow(dead_code)]
fn get_waliapi_key(state: &Arc<AppState>) -> Result<String, String> {
    let repo = Repository::new(state.db.pool.clone());
    let keys = tokio::task::block_in_place(|| {
        tauri::async_runtime::handle().block_on(async { repo.get_all_api_keys().await })
    })
    .map_err(|e| format!("获取 API Key 失败: {e}"))?;

    keys.into_iter()
        .find(|k| k.status == 1)
        .map(|k| k.key)
        .ok_or_else(|| "没有可用的 API Key，请先在「密钥」页创建".to_string())
}

// ── 各应用配置写入逻辑 ──

fn write_claude_code(
    config_dir: &PathBuf,
    waliapi_url: &str,
    waliapi_key: &str,
    model: &str,
) -> Result<(), String> {
    let settings_path = config_dir.join("settings.json");
    let mut settings: serde_json::Value = if settings_path.exists() {
        read_json_file(&settings_path).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    if let Some(obj) = settings.as_object_mut() {
        obj.insert(
            "env".to_string(),
            serde_json::json!({
                "ANTHROPIC_BASE_URL": waliapi_url,
                "ANTHROPIC_API_KEY": waliapi_key,
                "ANTHROPIC_MODEL": model
            }),
        );
        obj.insert("_waliapi".to_string(), serde_json::json!(true));
    }

    write_json_file(&settings_path, &settings)
}

fn write_codex(
    config_dir: &PathBuf,
    waliapi_url: &str,
    waliapi_key: &str,
    model: &str,
) -> Result<(), String> {
    use toml_edit::DocumentMut;

    // Codex 鉴权方式：experimental_bearer_token 作为 Bearer token 发给上游
    // 不写 auth.json 的 OPENAI_API_KEY，避免 Codex 拿它去 OpenAI 验证
    // （参照 cc-switch 的做法，只通过 experimental_bearer_token 传递 key）

    let config_path = config_dir.join("config.toml");
    let existing_text = if config_path.exists() {
        std::fs::read_to_string(&config_path)
            .map_err(|e| format!("Failed to read config.toml: {e}"))?
    } else {
        String::new()
    };

    let mut doc = existing_text
        .parse::<DocumentMut>()
        .map_err(|e| format!("Failed to parse config.toml: {e}"))?;

    // Set model_provider and model at top level
    doc["model_provider"] = toml_edit::value("waliapi");
    doc["model"] = toml_edit::value(model);

    // Ensure [model_providers] table exists
    if doc.get("model_providers").is_none() {
        let mut table = toml_edit::Table::new();
        table.set_implicit(true);
        doc["model_providers"] = toml_edit::Item::Table(table);
    }

    // Insert/update [model_providers.waliapi] preserving other providers
    if let Some(providers) = doc["model_providers"].as_table_mut() {
        let waliapi_entry = providers.entry("waliapi");
        let provider_table =
            waliapi_entry.or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
        if let Some(table) = provider_table.as_table_mut() {
            table["name"] = toml_edit::value("WaLiAPI Gateway");
            table["base_url"] =
                toml_edit::value(format!("{}/v1", waliapi_url.trim_end_matches('/')));
            table["wire_api"] = toml_edit::value("responses");
            table["experimental_bearer_token"] = toml_edit::value(waliapi_key);
            // Ensure requires_openai_auth is NOT set for waliapi provider
            // This prevents Codex from trying to validate the token with OpenAI
            table.remove("requires_openai_auth");
        }

        // If there's a legacy 'custom' provider with requires_openai_auth = true
        // and base_url pointing to a third-party (non-OpenAI) endpoint,
        // remove requires_openai_auth to prevent auth conflicts
        if let Some(custom_table) = providers.get_mut("custom") {
            if let Some(t) = custom_table.as_table_mut() {
                if t.contains_key("requires_openai_auth") {
                    t.remove("requires_openai_auth");
                }
            }
        }
    }

    atomic_write(&config_path, doc.to_string().as_bytes())?;
    Ok(())
}

fn write_gemini_cli(
    config_dir: &PathBuf,
    waliapi_url: &str,
    waliapi_key: &str,
    model: &str,
) -> Result<(), String> {
    let env_path = config_dir.join(".env");
    let env_content = format!(
        "# Generated by WaLiAPI\nGEMINI_API_KEY={}\nGEMINI_BASE_URL={}\nGEMINI_MODEL={}\n",
        waliapi_key, waliapi_url, model
    );
    atomic_write(&env_path, env_content.as_bytes())?;

    let settings_path = config_dir.join("settings.json");
    if !settings_path.exists() {
        write_json_file(&settings_path, &serde_json::json!({}))?;
    }
    Ok(())
}

fn write_claude_desktop(
    config_dir: &PathBuf,
    waliapi_url: &str,
    waliapi_key: &str,
    model: &str,
) -> Result<(), String> {
    let config_path = config_dir.join("claude_desktop_config.json");
    let mut config: serde_json::Value = if config_path.exists() {
        read_json_file(&config_path).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    if let Some(obj) = config.as_object_mut() {
        obj.insert(
            "apiKeyHelper".to_string(),
            serde_json::json!(format!("echo '{}'", waliapi_key)),
        );
        obj.insert("apiBaseUrl".to_string(), serde_json::json!(waliapi_url));
        obj.insert("defaultModel".to_string(), serde_json::json!(model));
        obj.insert("_waliapi".to_string(), serde_json::json!(true));
    }

    write_json_file(&config_path, &config)
}

fn write_opencode(
    config_dir: &PathBuf,
    waliapi_url: &str,
    waliapi_key: &str,
    model: &str,
) -> Result<(), String> {
    let config_path = config_dir.join("opencode.json");
    let mut config: serde_json::Value = if config_path.exists() {
        read_json_file(&config_path).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({"$schema": "https://opencode.ai/config.json"})
    };

    if let Some(obj) = config.as_object_mut() {
        let provider = serde_json::json!({
            "npm": "@ai-sdk/openai-compatible",
            "name": "WaLiAPI Gateway",
            "options": {
                "baseURL": format!("{}/v1", waliapi_url),
                "apiKey": waliapi_key
            },
            "models": {
                "waliapi-default": { "name": model }
            }
        });
        if let Some(providers) = obj.get_mut("provider").and_then(|v| v.as_object_mut()) {
            providers.insert("waliapi".to_string(), provider);
        } else {
            obj.insert(
                "provider".to_string(),
                serde_json::json!({"waliapi": provider}),
            );
        }
    }

    write_json_file(&config_path, &config)
}

fn write_openclaw(
    config_dir: &PathBuf,
    waliapi_url: &str,
    waliapi_key: &str,
    model: &str,
) -> Result<(), String> {
    let config_path = config_dir.join("config.json");
    let mut config: serde_json::Value = if config_path.exists() {
        read_json_file(&config_path).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    if let Some(obj) = config.as_object_mut() {
        obj.insert(
            "baseUrl".to_string(),
            serde_json::json!(format!("{}/v1", waliapi_url)),
        );
        obj.insert("apiKey".to_string(), serde_json::json!(waliapi_key));
        obj.insert("model".to_string(), serde_json::json!(model));
        obj.insert("_waliapi".to_string(), serde_json::json!(true));
    }

    write_json_file(&config_path, &config)
}

fn write_hermes(
    config_dir: &PathBuf,
    waliapi_url: &str,
    waliapi_key: &str,
    model: &str,
) -> Result<(), String> {
    let config_path = config_dir.join("config.json");
    let mut config: serde_json::Value = if config_path.exists() {
        read_json_file(&config_path).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    if let Some(obj) = config.as_object_mut() {
        if let Some(providers) = obj
            .get_mut("custom_providers")
            .and_then(|v| v.as_array_mut())
        {
            providers.retain(|p| p.get("id").and_then(|v| v.as_str()) != Some("waliapi"));
            let mut entry = serde_json::Map::new();
            entry.insert("id".to_string(), serde_json::json!("waliapi"));
            entry.insert("name".to_string(), serde_json::json!("WaLiAPI Gateway"));
            entry.insert(
                "base_url".to_string(),
                serde_json::json!(format!("{}/v1", waliapi_url)),
            );
            entry.insert("api_key".to_string(), serde_json::json!(waliapi_key));
            entry.insert("default_model".to_string(), serde_json::json!(model));
            providers.push(serde_json::Value::Object(entry));
        } else {
            let mut entry = serde_json::Map::new();
            entry.insert("id".to_string(), serde_json::json!("waliapi"));
            entry.insert("name".to_string(), serde_json::json!("WaLiAPI Gateway"));
            entry.insert(
                "base_url".to_string(),
                serde_json::json!(format!("{}/v1", waliapi_url)),
            );
            entry.insert("api_key".to_string(), serde_json::json!(waliapi_key));
            entry.insert("default_model".to_string(), serde_json::json!(model));
            obj.insert(
                "custom_providers".to_string(),
                serde_json::Value::Array(vec![serde_json::Value::Object(entry)]),
            );
        }
    }

    write_json_file(&config_path, &config)
}

fn write_walicode(
    config_dir: &PathBuf,
    waliapi_url: &str,
    waliapi_key: &str,
    model: &str,
) -> Result<(), String> {
    let base_url = format!("{}/v1", waliapi_url.trim_end_matches('/'));

    // WaLiCode 有两个可能的配置路径：
    //   1. 标准路径 ~/.config/walicode/ai_settings.json (settings_write_path 写入位置)
    //   2. 旧版路径 ~/Library/Application Support/WaLiCode/ai_settings.json (legacy)
    // WaLiCode 读取时优先查标准路径，fallback 到旧路径
    // 我们需要同时写入两个路径，确保不管走哪个都能读到

    let standard_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("walicode");
    let paths_to_write: Vec<PathBuf> = if *config_dir == standard_dir {
        vec![standard_dir.join("ai_settings.json")]
    } else {
        // 两个路径都写
        vec![
            standard_dir.join("ai_settings.json"),
            config_dir.join("ai_settings.json"),
        ]
    };

    // 读取已有配置：优先标准路径，其次旧路径
    let existing_config: serde_json::Value = paths_to_write
        .iter()
        .find_map(|p| {
            if p.exists() {
                read_json_file(p).ok()
            } else {
                None
            }
        })
        .unwrap_or_else(|| serde_json::json!({}));

    let mut config = existing_config;

    if let Some(obj) = config.as_object_mut() {
        // 在 customProviders 数组中查找或创建 waliapi provider
        let providers = obj
            .entry("customProviders".to_string())
            .or_insert_with(|| serde_json::json!([]));

        let mut found = false;
        if let Some(arr) = providers.as_array_mut() {
            for p in arr.iter_mut() {
                if p.get("id").and_then(|v| v.as_str()) == Some("waliapi") {
                    p["name"] = serde_json::json!("WaLiAPI");
                    p["apiKey"] = serde_json::json!(waliapi_key);
                    p["baseUrl"] = serde_json::json!(&base_url);
                    p["model"] = serde_json::json!(model);
                    p["apiFormat"] = serde_json::json!("openai");
                    p["enabled"] = serde_json::json!(true);
                    // 更新 customModels 列表
                    if let Some(cm) = p.get("customModels").and_then(|v| v.as_array()) {
                        if !cm.iter().any(|m| m.as_str() == Some(model)) {
                            if let Some(cm) =
                                p.get_mut("customModels").and_then(|v| v.as_array_mut())
                            {
                                cm.insert(0, serde_json::json!(model));
                            }
                        }
                    } else {
                        p["customModels"] = serde_json::json!([model]);
                    }
                    found = true;
                    break;
                }
            }

            if !found {
                arr.push(serde_json::json!({
                    "id": "waliapi",
                    "name": "WaLiAPI",
                    "apiKey": waliapi_key,
                    "baseUrl": base_url,
                    "model": model,
                    "customModels": [model],
                    "apiFormat": "openai",
                    "enabled": true
                }));
            }
        }

        // 激活 waliapi provider
        obj.insert(
            "activeCustomProviderId".to_string(),
            serde_json::json!("waliapi"),
        );
        // providerType 必须设为 custom，否则前端不会走 custom provider 分支
        obj.insert("providerType".to_string(), serde_json::json!("custom"));
        obj.insert("provider".to_string(), serde_json::json!("openai"));
        // 同步顶级字段（CLI resolve_effective_settings 的 fallback）
        obj.insert("apiKey".to_string(), serde_json::json!(waliapi_key));
        obj.insert("baseUrl".to_string(), serde_json::json!(&base_url));
        obj.insert("model".to_string(), serde_json::json!(model));
        obj.insert("_waliapi".to_string(), serde_json::json!(true));
    }

    // 写入所有目标路径
    let mut errors = Vec::new();
    for path in &paths_to_write {
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                if let Err(e) = fs::create_dir_all(parent) {
                    errors.push(format!("创建目录 {} 失败: {}", parent.display(), e));
                    continue;
                }
            }
        }
        if let Err(e) = write_json_file(path, &config) {
            errors.push(format!("写入 {} 失败: {}", path.display(), e));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

// ── 检测是否已由 WaLiAPI 配置 ──

fn detect_applied(config_path: &PathBuf, app_name: &str) -> bool {
    if !config_path.exists() {
        return false;
    }
    let content = match fs::read_to_string(config_path) {
        Ok(c) => c,
        Err(_) => return false,
    };

    match app_name {
        "claude-code" | "claude-desktop" | "openclaw" => {
            let v: serde_json::Value = match serde_json::from_str(&content) {
                Ok(v) => v,
                Err(_) => return false,
            };
            v.get("_waliapi").and_then(|v| v.as_bool()).unwrap_or(false)
        }
        "codex" => content.contains("WaLiAPI") || content.contains("waliapi"),
        "gemini-cli" => content.contains("WaLiAPI"),
        "opencode" => {
            let v: serde_json::Value = match serde_json::from_str(&content) {
                Ok(v) => v,
                Err(_) => return false,
            };
            v.pointer("/provider/waliapi").is_some()
        }
        "hermes" => {
            let v: serde_json::Value = match serde_json::from_str(&content) {
                Ok(v) => v,
                Err(_) => return false,
            };
            v.get("custom_providers")
                .and_then(|v| v.as_array())
                .and_then(|arr| {
                    arr.iter()
                        .find(|p| p.get("id").and_then(|v| v.as_str()) == Some("waliapi"))
                })
                .is_some()
        }
        "walicode" => {
            // 检查两个可能的路径：旧路径（config_path）和标准路径
            let standard_path = dirs::config_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("walicode")
                .join("ai_settings.json");
            let check_path = if standard_path.exists() {
                &standard_path
            } else {
                config_path
            };
            if !check_path.exists() {
                return false;
            }
            let content = match fs::read_to_string(check_path) {
                Ok(c) => c,
                Err(_) => return false,
            };
            let v: serde_json::Value = match serde_json::from_str(&content) {
                Ok(v) => v,
                Err(_) => return false,
            };
            // 检查 customProviders 中有 waliapi 且已激活
            let has_provider = v
                .get("customProviders")
                .and_then(|v| v.as_array())
                .and_then(|arr| {
                    arr.iter()
                        .find(|p| p.get("id").and_then(|v| v.as_str()) == Some("waliapi"))
                })
                .is_some();
            let is_active =
                v.get("activeCustomProviderId").and_then(|v| v.as_str()) == Some("waliapi");
            has_provider && is_active
        }
        _ => false,
    }
}

// ── Tauri Commands ──

#[tauri::command]
pub async fn get_app_configs(
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<Vec<AppInfo>, String> {
    get_app_configs_impl(state.inner()).await
}

pub async fn get_app_configs_impl(_state: &Arc<AppState>) -> Result<Vec<AppInfo>, String> {
    let apps: Vec<AppInfo> = APPS
        .iter()
        .map(|app| {
            let config_dir = (app.config_dir_fn)();
            let config_path = config_dir.join(app.config_file);
            let available = (app.check_installed_fn)(&config_dir);
            let applied = detect_applied(&config_path, app.name);

            AppInfo {
                name: app.name.to_string(),
                label: app.label.to_string(),
                icon: app.icon.to_string(),
                description: app.description.to_string(),
                config_path: config_path.to_string_lossy().to_string(),
                config_format: app.config_format.to_string(),
                available,
                applied,
                download_url: app.download_url.to_string(),
            }
        })
        .collect();

    Ok(apps)
}

#[tauri::command]
pub async fn apply_app_config(
    app_name: String,
    api_key: String,
    model: String,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<ApplyResult, String> {
    apply_app_config_impl(&app_name, &api_key, &model, state.inner()).await
}

pub async fn apply_app_config_impl(
    app_name: &str,
    api_key: &str,
    model: &str,
    state: &Arc<AppState>,
) -> Result<ApplyResult, String> {
    let waliapi_url = get_waliapi_url(state).await;

    let app_def = APPS
        .iter()
        .find(|a| a.name == app_name)
        .ok_or_else(|| format!("不支持的应用: {app_name}"))?;

    let config_dir = (app_def.config_dir_fn)();
    let config_path = config_dir.join(app_def.config_file);

    let _ = backup_config(&config_path);

    let result = match app_name {
        "claude-code" => write_claude_code(&config_dir, &waliapi_url, &api_key, &model),
        "codex" => write_codex(&config_dir, &waliapi_url, &api_key, &model),
        "gemini-cli" => write_gemini_cli(&config_dir, &waliapi_url, &api_key, &model),
        "claude-desktop" => write_claude_desktop(&config_dir, &waliapi_url, &api_key, &model),
        "opencode" => write_opencode(&config_dir, &waliapi_url, &api_key, &model),
        "openclaw" => write_openclaw(&config_dir, &waliapi_url, &api_key, &model),
        "hermes" => write_hermes(&config_dir, &waliapi_url, &api_key, &model),
        "walicode" => write_walicode(&config_dir, &waliapi_url, &api_key, &model),
        _ => return Err(format!("不支持的应用: {app_name}")),
    };

    match result {
        Ok(()) => {
            let msg = if app_name == "walicode" {
                format!(
                    "配置已写入。请重启 WaLiCode 使配置生效（WaLiCode 会使用本地缓存覆盖旧配置）"
                )
            } else {
                format!("配置已写入 {}", config_path.display())
            };
            Ok(ApplyResult {
                success: true,
                message: msg,
            })
        }
        Err(e) => {
            let _ = restore_config(&config_path);
            Ok(ApplyResult {
                success: false,
                message: e,
            })
        }
    }
}

#[tauri::command]
pub async fn clear_app_config(app_name: String) -> Result<ApplyResult, String> {
    clear_app_config_impl(&app_name).await
}

pub async fn clear_app_config_impl(app_name: &str) -> Result<ApplyResult, String> {
    let app_def = APPS
        .iter()
        .find(|a| a.name == app_name)
        .ok_or_else(|| format!("不支持的应用: {app_name}"))?;

    let config_dir = (app_def.config_dir_fn)();
    let config_path = config_dir.join(app_def.config_file);

    match restore_config(&config_path) {
        Ok(()) => Ok(ApplyResult {
            success: true,
            message: format!("已恢复 {} 的原始配置", app_def.label),
        }),
        Err(e) => Ok(ApplyResult {
            success: false,
            message: format!("恢复失败: {e}"),
        }),
    }
}

#[tauri::command]
pub async fn get_app_config_content(app_name: String) -> Result<ConfigContent, String> {
    get_app_config_content_impl(&app_name).await
}

pub async fn get_app_config_content_impl(app_name: &str) -> Result<ConfigContent, String> {
    let app_def = APPS
        .iter()
        .find(|a| a.name == app_name)
        .ok_or_else(|| format!("不支持的应用: {app_name}"))?;

    let config_dir = (app_def.config_dir_fn)();
    let config_path = config_dir.join(app_def.config_file);

    // WaLiCode 特殊处理：优先读标准路径 ~/.config/walicode/ai_settings.json
    let config_path = if app_name == "walicode" {
        let standard_path = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("walicode")
            .join("ai_settings.json");
        if standard_path.exists() {
            standard_path
        } else {
            config_path
        }
    } else {
        config_path
    };

    if !config_path.exists() {
        return Ok(ConfigContent {
            exists: false,
            content: String::new(),
            error: None,
        });
    }

    match fs::read_to_string(&config_path) {
        Ok(content) => Ok(ConfigContent {
            exists: true,
            content,
            error: None,
        }),
        Err(e) => Ok(ConfigContent {
            exists: true,
            content: String::new(),
            error: Some(format!("读取失败: {e}")),
        }),
    }
}

#[tauri::command]
pub async fn open_config_folder(app_name: String) -> Result<(), String> {
    let config_dir = prepare_app_config_path_impl(&app_name).await?;

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&config_dir)
            .spawn()
            .map_err(|e| format!("打开文件夹失败: {e}"))?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&config_dir)
            .spawn()
            .map_err(|e| format!("打开文件夹失败: {e}"))?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&config_dir)
            .spawn()
            .map_err(|e| format!("打开文件夹失败: {e}"))?;
    }

    Ok(())
}

/// Prepare and return the server-side configuration directory. Browser clients
/// cannot open a native file manager on a remote Linux host, so the Web bridge
/// returns this path while still preserving the desktop command's behavior.
pub async fn prepare_app_config_path_impl(app_name: &str) -> Result<String, String> {
    let app_def = APPS
        .iter()
        .find(|a| a.name == app_name)
        .ok_or_else(|| format!("不支持的应用: {app_name}"))?;

    let config_dir = (app_def.config_dir_fn)();

    // 如果目录不存在，尝试创建
    if !config_dir.exists() {
        fs::create_dir_all(&config_dir).map_err(|e| format!("创建目录失败: {e}"))?;
    }

    // 如果配置文件不存在，先创建一个空文件
    let config_path = config_dir.join(app_def.config_file);
    if !config_path.exists() {
        atomic_write(&config_path, b"{}")?;
    }

    Ok(config_dir.to_string_lossy().to_string())
}
