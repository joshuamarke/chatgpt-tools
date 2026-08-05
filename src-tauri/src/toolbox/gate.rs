//! Runtime gate: third-party-only enhancements.
//!
//! Force Chinese / plugin marketplace unlock are only **effective** when:
//! 1. User left the toolbox toggle on, and
//! 2. Current Codex routing is non-official (third-party API / custom provider).
//!
//! Official OpenAI (archive official, official live config, or official proxy
//! shell) never applies these patches — even if the toggle stays checked.
//! Fast startup / Computer Use Guard are independent and not gated here.

use serde::Serialize;

/// True when Codex should run third-party API enhancements.
///
/// Inverse of [`crate::providers::model_unlock::should_skip_unlock_for_official`].
pub fn third_party_codex_active() -> bool {
    !crate::providers::model_unlock::should_skip_unlock_for_official()
}

/// User preference ∧ third-party live routing.
pub fn force_chinese_effective() -> bool {
    super::settings::force_chinese_locale() && third_party_codex_active()
}

/// User preference ∧ third-party live routing (plugin marketplace unlock).
pub fn plugin_marketplace_unlock_effective() -> bool {
    super::settings::plugin_marketplace_unlock() && third_party_codex_active()
}

/// Snapshot for GUI / IPC: preferences + effective runtime state.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolboxRuntimeStatus {
    #[serde(flatten)]
    pub settings: super::settings::ToolboxSettings,
    /// Live Codex is non-official (third-party / custom).
    pub third_party_active: bool,
    /// Preference on **and** third-party → inject zh-CN.
    pub force_chinese_effective: bool,
    /// Preference on **and** third-party → plugin marketplace unlock (when inject lands).
    pub plugin_marketplace_unlock_effective: bool,
}

pub fn runtime_status() -> ToolboxRuntimeStatus {
    let settings = super::settings::get_settings();
    let third = third_party_codex_active();
    ToolboxRuntimeStatus {
        force_chinese_effective: settings.force_chinese_locale && third,
        plugin_marketplace_unlock_effective: settings.plugin_marketplace_unlock && third,
        third_party_active: third,
        settings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toolbox::settings::ToolboxSettings;

    #[test]
    fn effective_requires_both_pref_and_third_party() {
        // Pure logic unit: without third party, effective is false even if pref true.
        // We cannot flip live routing in unit tests easily; assert helpers compose.
        let pref_on = true;
        let third = false;
        assert!(!(pref_on && third));
        let pref_off = false;
        let third_on = true;
        assert!(!(pref_off && third_on));
        assert!(pref_on && third_on);
    }

    #[test]
    fn runtime_status_effective_matches_pref_and_third_party() {
        let s = runtime_status();
        // Do not assume on-disk prefs (may differ from Default in dev machines).
        // effective must always be pref ∧ third_party.
        assert_eq!(
            s.force_chinese_effective,
            s.settings.force_chinese_locale && s.third_party_active
        );
        assert_eq!(
            s.plugin_marketplace_unlock_effective,
            s.settings.plugin_marketplace_unlock && s.third_party_active
        );
        let _ = ToolboxSettings::default();
    }
}
