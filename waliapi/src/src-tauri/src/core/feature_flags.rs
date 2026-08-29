//! Feature-flag entry points for the channel-protocol refactor (T00 decision 10).
//!
//! Routing capability switches for the channel-protocol refactor.
//!
//! | Flag                    | Gates                                                     |
//! |-------------------------|-----------------------------------------------------------|
//! | `features.new_routeplan`    | Model-first RoutePlan path in the HTTP handlers. Enabled by default in production. |
//! | `features.cross_protocol_codec` | G2 conversion groups (Chat→Anthropic, Messages→Chat, Responses→Chat). Enabled by default in production. |
//! | `features.native_responses` | Responses G1 native `/responses` group. Enabled by default in production. |
//! | `features.ollama_native`    | Native Ollama `/api/chat` group (added by T06 to the Chat matrix; OFF until the executor + downstream Chat chain pass their tests). |
//! | `routing.prefer_auth_accounts` | Sort auth-account candidates before channel candidates within the same route group. |
//! | `routing.prefer_same_protocol` | Prefer candidates that can serve the request without protocol conversion. |

use crate::settings_store::SettingsStore;

/// Snapshot of the four routing feature flags.  Defaults are all-off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FeatureFlags {
    pub new_routeplan: bool,
    pub cross_protocol_codec: bool,
    pub native_responses: bool,
    pub ollama_native: bool,
    pub prefer_auth_accounts: bool,
    pub prefer_same_protocol: bool,
}

impl FeatureFlags {
    /// Everything off: legacy flat routing (production default until rollout).
    pub fn all_off() -> Self {
        Self::default()
    }

    /// Everything on — used only by tests / manual staging.
    pub fn all_on() -> Self {
        Self {
            new_routeplan: true,
            cross_protocol_codec: true,
            native_responses: true,
            ollama_native: true,
            prefer_auth_accounts: false,
            prefer_same_protocol: false,
        }
    }

    /// Whether any conversion group may be built.
    pub fn conversions_enabled(&self) -> bool {
        self.cross_protocol_codec
    }
}

/// Read the routing flags from the settings store（桌面 tauri store / headless JSON 文件）。
///
/// Core routing capabilities are no longer user-facing rollout switches:
/// protocol conversion and native Responses are baseline compatibility features.
/// The planner never reads the store itself — handlers read it once and pass a
/// snapshot so the planner stays pure and deterministic in tests.
pub fn read_feature_flags(settings: &SettingsStore) -> FeatureFlags {
    FeatureFlags {
        new_routeplan: true,
        cross_protocol_codec: true,
        native_responses: true,
        ollama_native: settings.get_bool("features.ollama_native", false),
        prefer_auth_accounts: settings.get_bool("routing.prefer_auth_accounts", false),
        prefer_same_protocol: settings.get_bool("routing.prefer_same_protocol", false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_all_off() {
        let f = FeatureFlags::default();
        assert!(!f.new_routeplan);
        assert!(!f.cross_protocol_codec);
        assert!(!f.native_responses);
        assert!(!f.ollama_native);
        assert!(!f.prefer_auth_accounts);
        assert!(!f.prefer_same_protocol);
        assert!(!f.conversions_enabled());
    }

    #[test]
    fn all_on_enables_conversions() {
        let f = FeatureFlags::all_on();
        assert!(f.new_routeplan);
        assert!(f.conversions_enabled());
    }
}
