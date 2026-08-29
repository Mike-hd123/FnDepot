//! Legacy flat channel selection (T05 keeps this as the flag-off fallback).
//!
//! The model-first [`crate::core::route_plan::authorize_and_plan`] replaces
//! this path.  While the `new_routeplan` feature flag is OFF, handlers still
//! use [`Dispatcher::select_channels`].  This module is NOT deleted in T05
//! (the brief forbids it) — it is listed for T06 removal in the task report.
//!
//! The priority/weight ordering now delegates to the shared
//! [`order_by_priority_weight`] helper so both paths keep identical semantics.

use crate::adaptor::ChannelConfig;
use crate::core::route_plan::order_by_priority_weight;
use crate::db::models::Channel;
use serde_json::Value;

/// Legacy flat failover selector.
pub struct Dispatcher;

impl Dispatcher {
    /// Build an ordered failover queue based on priority, weight, and model
    /// support.  Kept byte-compatible with the pre-refactor behavior.
    pub fn select_channels(channels: &[Channel], requested_model: &str) -> Vec<Channel> {
        let candidates: Vec<Channel> = channels
            .iter()
            .filter(|c| {
                if c.status != 1 {
                    return false;
                }
                let models: Vec<String> = serde_json::from_str(&c.models).unwrap_or_default();
                if models.is_empty() || models.iter().any(|m| m == requested_model) {
                    return true;
                }
                // Also check model_mapping keys — mapped model names are accepted.
                let mapping: Value = serde_json::from_str(&c.model_mapping).unwrap_or_default();
                if let Some(obj) = mapping.as_object() {
                    return obj.contains_key(requested_model);
                }
                false
            })
            .cloned()
            .collect();

        if candidates.is_empty() {
            return Vec::new();
        }

        let mut rng = rand::rng();
        order_by_priority_weight(candidates, &mut rng)
    }

    pub fn channel_to_config(channel: &Channel) -> ChannelConfig {
        let models: Vec<String> = serde_json::from_str(&channel.models).unwrap_or_default();
        let model_mapping: Value = serde_json::from_str(&channel.model_mapping).unwrap_or_default();
        let extra: Value = serde_json::from_str(&channel.config).unwrap_or_default();

        ChannelConfig {
            base_url: channel.base_url.clone(),
            api_key: channel.api_key.clone(),
            models,
            model_mapping,
            extra,
            timeout_secs: channel.timeout_secs.max(1) as u64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(id: &str, models: &[&str], priority: i64, weight: i64) -> Channel {
        Channel {
            id: id.into(),
            name: format!("ch-{}", id),
            channel_type: "openai".into(),
            base_url: "https://api.openai.com/v1".into(),
            api_key: "sk-test".into(),
            models: serde_json::to_string(
                &models.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            )
            .unwrap(),
            status: 1,
            priority,
            weight,
            config: "{}".into(),
            model_mapping: "{}".into(),
            timeout_secs: 30,
            protocol: None,
            provider: None,
            native_base_url: None,
            native_endpoints: None,
            preset_revision: None,
            identity_revision: 0,
            legacy_executor_override: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            last_test_at: None,
            last_test_ok: None,
        }
    }

    #[test]
    fn filters_disabled_and_unsupported_models() {
        let disabled = channel("d", &["m"], 1, 1);
        let mut disabled = disabled;
        disabled.status = 0;
        let ok = channel("ok", &["m"], 1, 1);
        let other = channel("other", &["other-model"], 1, 1);
        let selected = Dispatcher::select_channels(&[disabled, ok, other], "m");
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].id, "ok");
    }

    #[test]
    fn empty_models_is_wildcard() {
        let wild = channel("w", &[], 1, 1);
        let selected = Dispatcher::select_channels(&[wild], "anything");
        assert_eq!(selected.len(), 1);
    }

    #[test]
    fn higher_priority_first_regardless_of_weight() {
        let low = channel("low", &["m"], 10, 1000);
        let high = channel("high", &["m"], 50, 1);
        let selected = Dispatcher::select_channels(&[low, high], "m");
        assert_eq!(selected[0].id, "high");
        assert_eq!(selected[1].id, "low");
    }

    #[test]
    fn same_priority_keeps_weight_semantics() {
        // Equal weights: every candidate appears exactly once.
        let c1 = channel("c1", &["m"], 10, 1);
        let c2 = channel("c2", &["m"], 10, 1);
        let c3 = channel("c3", &["m"], 10, 1);
        let selected = Dispatcher::select_channels(&[c1, c2, c3], "m");
        assert_eq!(selected.len(), 3);
        let mut ids: Vec<&str> = selected.iter().map(|c| c.id.as_str()).collect();
        ids.sort_unstable();
        assert_eq!(ids, vec!["c1", "c2", "c3"]);
    }
}
