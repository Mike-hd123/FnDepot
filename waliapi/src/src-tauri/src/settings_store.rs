//! 设置存储抽象：桌面端走 tauri-plugin-store，headless（waliapi-web）走 JSON 文件。
//!
//! 两种后端共享同一套扁平点分键（如 `server.port`、`security.enabled`），
//! 文件格式与 tauri-plugin-store 的输出一致（顶层 JSON 对象，键原样存放）。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::Value;

pub trait SettingsBackend: Send + Sync {
    fn get(&self, key: &str) -> Option<Value>;
    /// 批量写入并持久化。
    fn set_many(&self, entries: &[(String, Value)]) -> Result<(), String>;
}

#[derive(Clone)]
pub struct SettingsStore {
    inner: Arc<dyn SettingsBackend>,
}

impl SettingsStore {
    pub fn new(inner: Arc<dyn SettingsBackend>) -> Self {
        Self { inner }
    }

    pub fn file(path: PathBuf) -> Self {
        Self::new(Arc::new(FileSettingsBackend::new(path)))
    }

    pub fn get(&self, key: &str) -> Option<Value> {
        self.inner.get(key)
    }

    pub fn set_many(&self, entries: &[(String, Value)]) -> Result<(), String> {
        self.inner.set_many(entries)
    }

    pub fn get_bool(&self, key: &str, default: bool) -> bool {
        self.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
    }

    pub fn get_str(&self, key: &str, default: &str) -> String {
        self.get(key)
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| default.to_string())
    }

    pub fn get_u64(&self, key: &str, default: u64) -> u64 {
        self.get(key).and_then(|v| v.as_u64()).unwrap_or(default)
    }
}

/// headless 模式：单个 JSON 文件，读取即解析，写入整文件回写。
pub struct FileSettingsBackend {
    path: PathBuf,
    write_lock: Mutex<()>,
}

impl FileSettingsBackend {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            write_lock: Mutex::new(()),
        }
    }

    fn read_all(&self) -> serde_json::Map<String, Value> {
        let Ok(content) = std::fs::read_to_string(&self.path) else {
            return serde_json::Map::new();
        };
        match serde_json::from_str::<Value>(&content) {
            Ok(Value::Object(map)) => map,
            _ => serde_json::Map::new(),
        }
    }

    fn write_all(&self, map: &serde_json::Map<String, Value>) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let tmp = self.path.with_extension("json.tmp");
        let content = serde_json::to_string_pretty(&Value::Object(map.clone()))
            .map_err(|e| e.to_string())?;
        std::fs::write(&tmp, content).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())?;
        Ok(())
    }
}

impl SettingsBackend for FileSettingsBackend {
    fn get(&self, key: &str) -> Option<Value> {
        self.read_all().remove(key)
    }

    fn set_many(&self, entries: &[(String, Value)]) -> Result<(), String> {
        let _guard = self.write_lock.lock().map_err(|e| e.to_string())?;
        let mut map = self.read_all();
        for (key, value) in entries {
            map.insert(key.clone(), value.clone());
        }
        self.write_all(&map)
    }
}

/// 默认设置文件路径（与数据目录同级）。
pub fn default_settings_path(data_dir: &Path) -> PathBuf {
    data_dir.join("settings.json")
}
