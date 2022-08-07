use serde::{Deserialize, Serialize};
use std::default::default;

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Default)]
/// The type of the `.mergedlands.toml` meta file.
pub enum MetaType {
    #[default]
    /// A default [MetaType] created by the tool when no meta file existed.
    Auto,
    /// A patch [MetaType] used to control how the tool merges terrain.
    Patch,
    /// A marker [MetaType] so that the tool can ignore previous `Merged Lands.esp` results.
    MergedLands,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Default, Copy, Clone)]
/// The [ConflictStrategy] that the tool will use applying a [crate::merge::merge_strategy::MergeStrategy].
pub enum ConflictStrategy {
    #[default]
    /// Choose the best strategy.
    Auto,
    /// Merge both sides. This is the default for most conflicts.
    Resolve,
    /// Use this side of the conflict. This is the default for terrain indices.
    Overwrite,
    /// Use the other side of the conflict, i.e., drop this change.
    Ignore,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
/// The [MergeSettings] control how a part of a plugin should be processed.
pub struct MergeSettings {
    #[serde(default = "default_bool_true")]
    /// If `included` is `false` then any changes from the plugin will be dropped.
    pub included: bool,
    #[serde(default)]
    /// The [ConflictStrategy] to use for any conflicts found during a merge.
    pub conflict_strategy: ConflictStrategy,
}

impl Default for MergeSettings {
    /// The default [MergeSettings] are `included: true` and
    /// the [ConflictStrategy::Auto] `conflict_strategy`.
    fn default() -> Self {
        Self {
            included: true,
            conflict_strategy: default(),
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Default)]
/// A meta file describing how a plugin should be processed.
pub struct PluginMeta {
    /// The [MetaType] of this plugin.
    pub meta_type: MetaType,
    #[serde(skip_serializing_if = "skip_default")]
    #[serde(default)]
    /// The [MergeSettings] for the height map and associated vertex normals.
    pub height_map: MergeSettings,
    #[serde(skip_serializing_if = "skip_default")]
    #[serde(default)]
    /// The [MergeSettings] for the vertex colors.
    pub vertex_colors: MergeSettings,
    #[serde(skip_serializing_if = "skip_default")]
    #[serde(default)]
    /// The [MergeSettings] for the texture indices.
    pub texture_indices: MergeSettings,
    #[serde(skip_serializing_if = "skip_default")]
    #[serde(default)]
    /// The [MergeSettings] for the world map data.
    pub world_map_data: MergeSettings,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "version")]
/// A versioned [PluginMeta].
pub(in crate::io) enum VersionedPluginMeta {
    #[serde(rename = "0")]
    /// Initial release.
    V0(PluginMeta),
    #[serde(other)]
    /// An unknown version.
    Unsupported,
}

/// Helper function providing a default `true` value.
fn default_bool_true() -> bool {
    true
}

/// A function that returns `true` if the `field` is equal to [default()].
fn skip_default<T: Default + PartialEq>(field: &T) -> bool {
    field == &T::default()
}
