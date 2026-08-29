//! Web 管理面板 SSE 事件桥 + 统一事件出口。
//!
//! 桌面端通过 `app.emit(event, payload)` 把进度事件推给 Webview；
//! Web 面板没有 Webview，改为订阅 `/admin/api/events` SSE 流。
//! `EventSink` 对两种运行模式提供统一入口：桌面模式同时投递到 Webview 与
//! SSE 广播，headless 模式仅投递到 SSE 广播。

use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;
use tokio::sync::broadcast;

/// broadcast channel 容量；超出后最旧的事件被丢弃（lagged 客户端跳过）。
pub const EVENT_CHANNEL_CAPACITY: usize = 100;

#[derive(Clone, Debug)]
pub struct AdminEvent {
    pub event: String,
    pub payload: Value,
}

/// 统一事件出口。桌面 emit 以闭包形式注入，避免 headless 二进制链接 wry。
#[derive(Clone)]
pub struct EventSink {
    desktop_emit: Option<Arc<dyn Fn(&str, Value) + Send + Sync>>,
    tx: broadcast::Sender<AdminEvent>,
}

impl EventSink {
    pub fn headless(tx: broadcast::Sender<AdminEvent>) -> Self {
        Self {
            desktop_emit: None,
            tx,
        }
    }

    pub fn desktop(
        emit: impl Fn(&str, Value) + Send + Sync + 'static,
        tx: broadcast::Sender<AdminEvent>,
    ) -> Self {
        Self {
            desktop_emit: Some(Arc::new(emit)),
            tx,
        }
    }

    pub fn emit(&self, event: &str, payload: Value) {
        if let Some(emit) = &self.desktop_emit {
            emit(event, payload.clone());
        }
        let _ = self.tx.send(AdminEvent {
            event: event.to_string(),
            payload,
        });
    }

    pub fn emit_json<T: Serialize>(&self, event: &str, payload: T) {
        let value = serde_json::to_value(payload).unwrap_or(Value::Null);
        self.emit(event, value);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AdminEvent> {
        self.tx.subscribe()
    }
}
