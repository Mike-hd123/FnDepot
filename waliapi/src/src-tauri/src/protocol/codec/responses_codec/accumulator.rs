use super::super::error::{FeatureKind, UnsupportedFeatures};
use super::super::sse;
use serde_json::Value;
use std::collections::BTreeMap;

/// A complete-record SSE accumulator for non-stream account requests.
#[derive(Default)]
pub struct ResponsesEventAccumulator {
    pending: Vec<u8>,
    completed: Option<Value>,
    output_items: BTreeMap<u64, Value>,
    failed: Option<String>,
}
impl ResponsesEventAccumulator {
    pub fn push(&mut self, bytes: &[u8]) -> Result<(), UnsupportedFeatures> {
        self.pending.extend_from_slice(bytes);
        while let Some(end) = sse::record_end(&self.pending) {
            let record: Vec<u8> = self.pending.drain(..end).collect();
            self.record(&record)?;
        }
        Ok(())
    }
    fn record(&mut self, record: &[u8]) -> Result<(), UnsupportedFeatures> {
        let payload = sse::parse_data_payload(record)?;
        if payload.is_empty() || payload == "[DONE]" {
            return Ok(());
        }
        let event: Value = serde_json::from_str(&payload).map_err(|_| {
            UnsupportedFeatures::single(
                FeatureKind::UnknownEvent,
                "/",
                "Responses SSE data is not JSON",
            )
        })?;
        match event.get("type").and_then(Value::as_str) {
            Some("response.output_item.done") => {
                if let Some(item) = event.get("item").cloned() {
                    let index = event
                        .get("output_index")
                        .and_then(Value::as_u64)
                        .unwrap_or(self.output_items.len() as u64);
                    self.output_items.insert(index, item);
                }
            }
            Some("response.completed") => self.completed = event.get("response").cloned(),
            Some("response.failed") | Some("response.incomplete") => {
                self.failed = Some("Responses upstream reported a terminal failure".to_string())
            }
            _ => {}
        }
        Ok(())
    }
    pub fn finish(mut self) -> Result<Value, UnsupportedFeatures> {
        if !self.pending.is_empty() {
            return Err(UnsupportedFeatures::single(
                FeatureKind::UnknownEvent,
                "/",
                "Responses SSE ended mid-record",
            ));
        }
        if self.failed.is_some() {
            return Err(UnsupportedFeatures::single(
                FeatureKind::UnknownEvent,
                "/",
                "Responses upstream failed",
            ));
        }
        let mut completed = self.completed.take().ok_or_else(|| {
            UnsupportedFeatures::single(
                FeatureKind::UnknownEvent,
                "/",
                "Responses SSE ended without response.completed",
            )
        })?;
        let needs_output_backfill = completed
            .get("output")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty);
        if needs_output_backfill && !self.output_items.is_empty() {
            let output = self.output_items.into_values().collect::<Vec<_>>();
            if let Some(object) = completed.as_object_mut() {
                object.insert("output".to_owned(), Value::Array(output));
            }
        }
        Ok(completed)
    }
}
