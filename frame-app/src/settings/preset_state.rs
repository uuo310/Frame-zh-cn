//! Custom Work State machine: a pure projection of a file's preset-binding state.
//!
//! The only irreducible stored state is `FileItem.custom_snapshot` (the most recent manual
//! edit). Everything else — which preset the config currently matches, whether a custom state
//! is active or merely suspended while the user views a preset — is *derived* here from
//! `(config, snapshot, presets)` by a single pure resolver. All preset UI (button label,
//! dropdown highlight, apply-to-all enablement) reads this resolver so there is one source of
//! truth and no scattered ad-hoc checks.
//!
//! The structural guarantee this provides: applying/viewing a preset from `CustomActive`
//! transitions to `CustomSuspended`, whose definition *retains* the snapshot. "Viewing a preset
//! does not destroy the custom state" is therefore guaranteed by the state model, not by luck.

use super::{configs_match, ConversionConfig, PresetDefinition};

/// The derived preset-binding state of a single file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PresetViewState {
    /// No snapshot and the config matches no preset (initial/default state).
    Unbound,
    /// No snapshot and the config matches preset `preset_id`.
    Matched { preset_id: String },
    /// A snapshot exists and the config equals it (the custom state is what's applied).
    CustomActive,
    /// A snapshot exists but the config differs from it: the custom state is suspended while
    /// the user views/applies something else. `viewing` is the preset the config currently
    /// matches, if any. The snapshot is retained by definition.
    CustomSuspended { viewing: Option<String> },
}

impl PresetViewState {
    /// Whether a custom work state exists (active or suspended).
    #[must_use]
    pub const fn has_custom(&self) -> bool {
        matches!(self, Self::CustomActive | Self::CustomSuspended { .. })
    }

    /// Whether there is a coherent config worth pushing via "apply to all".
    #[must_use]
    pub const fn has_pushable(&self) -> bool {
        matches!(
            self,
            Self::Matched { .. } | Self::CustomActive | Self::CustomSuspended { viewing: Some(_) }
        )
    }

    /// The preset id to show on the button / highlight in the dropdown, when the current config
    /// corresponds to a preset. `None` for custom-only or unbound states.
    #[must_use]
    pub fn preset_id(&self) -> Option<&str> {
        match self {
            Self::Matched { preset_id } | Self::CustomSuspended { viewing: Some(preset_id) } => {
                Some(preset_id.as_str())
            }
            Self::Unbound | Self::CustomActive | Self::CustomSuspended { viewing: None } => None,
        }
    }

    /// Whether the dropdown's synthetic "custom" row is the highlighted (current) row.
    #[must_use]
    pub const fn highlights_custom(&self) -> bool {
        matches!(self, Self::CustomActive)
    }
}

/// Resolve a file's preset-binding state from its config, optional custom snapshot, and the
/// preset list. Pure function; the single source of truth for all preset UI.
#[must_use]
pub fn resolve_preset_view_state(
    config: &ConversionConfig,
    snapshot: Option<&ConversionConfig>,
    presets: &[PresetDefinition],
) -> PresetViewState {
    if let Some(snapshot) = snapshot {
        return if configs_match(config, snapshot) {
            PresetViewState::CustomActive
        } else {
            PresetViewState::CustomSuspended {
                viewing: matched_preset_id(config, presets),
            }
        };
    }
    match matched_preset_id(config, presets) {
        Some(preset_id) => PresetViewState::Matched { preset_id },
        None => PresetViewState::Unbound,
    }
}

fn matched_preset_id(config: &ConversionConfig, presets: &[PresetDefinition]) -> Option<String> {
    presets
        .iter()
        .find(|preset| configs_match(config, &preset.config))
        .map(|preset| preset.id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preset(id: &str) -> PresetDefinition {
        PresetDefinition::built_in(id, id, ConversionConfig::default())
    }

    fn presets() -> Vec<PresetDefinition> {
        vec![preset("p1")]
    }

    #[test]
    fn unbound_when_no_snapshot_and_no_match() {
        let mut config = ConversionConfig::default();
        config.container = "mkv".to_string();
        let state = resolve_preset_view_state(&config, None, &presets());
        assert_eq!(state, PresetViewState::Unbound);
        assert!(!state.has_custom());
        assert!(!state.has_pushable());
        assert_eq!(state.preset_id(), None);
    }

    #[test]
    fn matched_when_config_equals_a_preset() {
        let state = resolve_preset_view_state(&ConversionConfig::default(), None, &presets());
        assert_eq!(
            state,
            PresetViewState::Matched {
                preset_id: "p1".to_string()
            }
        );
        assert!(state.has_pushable());
        assert_eq!(state.preset_id(), Some("p1"));
        assert!(!state.highlights_custom());
    }

    #[test]
    fn custom_active_when_config_equals_snapshot() {
        let mut config = ConversionConfig::default();
        config.container = "mkv".to_string();
        let snapshot = config.clone();
        let state = resolve_preset_view_state(&config, Some(&snapshot), &presets());
        assert_eq!(state, PresetViewState::CustomActive);
        assert!(state.has_custom());
        assert!(state.has_pushable());
        assert!(state.highlights_custom());
        assert_eq!(state.preset_id(), None);
    }

    /// The core guarantee: viewing/applying a preset while a custom snapshot exists must NOT
    /// lose the custom state — it becomes `CustomSuspended` retaining the snapshot.
    #[test]
    fn viewing_a_preset_suspends_but_retains_custom() {
        let mut custom = ConversionConfig::default();
        custom.container = "mkv".to_string();
        // User now views/applies preset p1: config becomes the preset config, snapshot kept.
        let viewing = ConversionConfig::default();
        let state = resolve_preset_view_state(&viewing, Some(&custom), &presets());
        assert_eq!(
            state,
            PresetViewState::CustomSuspended {
                viewing: Some("p1".to_string())
            }
        );
        assert!(state.has_custom(), "custom must survive viewing a preset");
        assert!(state.has_pushable());
        assert_eq!(state.preset_id(), Some("p1"));
        assert!(!state.highlights_custom());
    }

    #[test]
    fn suspended_without_viewing_when_config_matches_nothing() {
        let mut custom = ConversionConfig::default();
        custom.container = "mkv".to_string();
        let mut orphan = ConversionConfig::default();
        orphan.container = "avi".to_string();
        let state = resolve_preset_view_state(&orphan, Some(&custom), &presets());
        assert_eq!(
            state,
            PresetViewState::CustomSuspended { viewing: None }
        );
        assert!(state.has_custom());
        assert!(!state.has_pushable());
        assert_eq!(state.preset_id(), None);
    }
}
