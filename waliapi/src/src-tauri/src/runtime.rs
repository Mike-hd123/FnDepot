//! Runtime boundary shared by the desktop application and the headless server.
//!
//! Keeping the Tauri handle behind this small abstraction lets the HTTP server
//! run in a Linux process without creating a WebView. Headless notifications
//! are intentionally best-effort no-ops; they are UI progress updates only.
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_store::StoreExt;

#[derive(Clone)]
pub enum RuntimeHandle {
    Desktop(AppHandle),
    Headless(Arc<HeadlessRuntime>),
}

#[derive(Debug)]
pub struct HeadlessRuntime {
    data_dir: PathBuf,
    settings_path: PathBuf,
    events: tokio::sync::broadcast::Sender<RuntimeEvent>,
}

#[derive(Debug, Clone)]
pub struct RuntimeEvent {
    pub name: String,
    pub payload: Value,
}

/// Minimal equivalent of Tauri's path resolver used by existing HTTP service
/// handlers. It deliberately exposes only the application data directory.
pub struct RuntimePathResolver<'a>(&'a RuntimeHandle);

impl RuntimeHandle {
    pub fn desktop(app: AppHandle) -> Self {
        Self::Desktop(app)
    }

    pub fn headless(data_dir: impl Into<PathBuf>) -> Result<Self, String> {
        let data_dir = data_dir.into();
        std::fs::create_dir_all(&data_dir).map_err(|e| format!("create data directory: {e}"))?;
        let (events, _) = tokio::sync::broadcast::channel(256);
        Ok(Self::Headless(Arc::new(HeadlessRuntime {
            settings_path: data_dir.join("settings.json"),
            data_dir,
            events,
        })))
    }

    pub fn app_data_dir(&self) -> Result<PathBuf, String> {
        match self {
            Self::Desktop(app) => app.path().app_data_dir().map_err(|e| e.to_string()),
            Self::Headless(runtime) => Ok(runtime.data_dir.clone()),
        }
    }

    pub fn path(&self) -> RuntimePathResolver<'_> {
        RuntimePathResolver(self)
    }

    pub fn emit(&self, event: &str, payload: Value) -> Result<(), String> {
        match self {
            Self::Desktop(app) => app.emit(event, payload).map_err(|e| e.to_string()),
            Self::Headless(runtime) => {
                let _ = runtime.events.send(RuntimeEvent {
                    name: event.to_owned(),
                    payload,
                });
                Ok(())
            }
        }
    }

    pub fn subscribe(&self) -> Option<tokio::sync::broadcast::Receiver<RuntimeEvent>> {
        match self {
            Self::Desktop(_) => None,
            Self::Headless(runtime) => Some(runtime.events.subscribe()),
        }
    }

    pub fn setting(&self, key: &str) -> Option<Value> {
        match self {
            Self::Desktop(app) => app.store("settings.json").ok()?.get(key),
            Self::Headless(runtime) => runtime.read_settings().get(key).cloned(),
        }
    }

    pub fn set_settings(&self, settings: &serde_json::Map<String, Value>) -> Result<(), String> {
        match self {
            Self::Desktop(app) => {
                let store = app.store("settings.json").map_err(|e| e.to_string())?;
                for (key, value) in settings {
                    store.set(key, value.clone());
                }
                store.save().map_err(|e| e.to_string())
            }
            Self::Headless(runtime) => runtime.write_settings(settings),
        }
    }

    pub fn is_headless(&self) -> bool {
        matches!(self, Self::Headless(_))
    }
}

impl RuntimePathResolver<'_> {
    pub fn app_data_dir(&self) -> Result<PathBuf, String> {
        self.0.app_data_dir()
    }
}

impl HeadlessRuntime {
    fn read_settings(&self) -> serde_json::Map<String, Value> {
        std::fs::read(&self.settings_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default()
    }

    fn write_settings(&self, updates: &serde_json::Map<String, Value>) -> Result<(), String> {
        let mut current = self.read_settings();
        current.extend(updates.clone());
        let encoded = serde_json::to_vec_pretty(&current).map_err(|e| e.to_string())?;
        let temporary = self.settings_path.with_extension("json.tmp");
        std::fs::write(&temporary, encoded).map_err(|e| format!("write settings: {e}"))?;
        std::fs::rename(&temporary, &self.settings_path)
            .map_err(|e| format!("commit settings: {e}"))
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }
}
