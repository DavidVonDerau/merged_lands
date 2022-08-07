use crate::io::meta_schema::ConflictStrategy;
use crate::land::terrain_map::Vec2;
use crate::merge::conflict::ConflictResolver;
use crate::merge::ignore_strategy::IgnoreStrategy;
use crate::merge::overwrite_strategy::OverwriteStrategy;
use crate::merge::relative_terrain_map::{OptionalTerrainMap, RelativeTerrainMap};
use crate::merge::relative_to::RelativeTo;
use crate::merge::resolve_conflict_strategy::ResolveConflictStrategy;
use crate::ParsedPlugin;
use log::trace;
use std::default::default;

/// Types implementing [MergeStrategy] can create a new [RelativeTerrainMap] by combining
/// the `lhs` and `rhs` [RelativeTerrainMap]. The method for combining the maps is determined
/// by the type implementing [MergeStrategy::apply].
pub trait MergeStrategy {
    /// Combine the `lhs` and `rhs` [RelativeTerrainMap] into a new [RelativeTerrainMap].
    fn apply<U: RelativeTo + ConflictResolver, const T: usize>(
        &self,
        coords: Vec2<i32>,
        plugin: &ParsedPlugin,
        value: &str,
        lhs: &RelativeTerrainMap<U, T>,
        rhs: &RelativeTerrainMap<U, T>,
    ) -> RelativeTerrainMap<U, T>
    where
        <U as RelativeTo>::Delta: ConflictResolver;
}

/// Given optional `old` and `new` [RelativeTerrainMap], return an [OptionalTerrainMap]
/// representing either [None], the `old`, the `new`, or the merged combination of `old`
/// and `new` from applying the [MergeStrategy] `strategy` when both `old` and `new` are
/// [Some].
fn apply_strategy<U: RelativeTo + ConflictResolver, const T: usize>(
    coords: Vec2<i32>,
    plugin: &ParsedPlugin,
    value: &str,
    old: Option<&RelativeTerrainMap<U, T>>,
    new: Option<&RelativeTerrainMap<U, T>>,
    strategy: &impl MergeStrategy,
) -> OptionalTerrainMap<U, T>
where
    <U as RelativeTo>::Delta: ConflictResolver,
{
    if old.is_some() && new.is_some() {
        let merged = strategy.apply(
            coords,
            plugin,
            value,
            old.as_ref().expect("safe"),
            new.as_ref().expect("safe"),
        );

        Some(merged)
    } else if old.is_some() {
        old.cloned()
    } else if new.is_some() {
        new.cloned()
    } else {
        None
    }
}

/// Given optional `old` and `new` [RelativeTerrainMap], and a desired [ConflictStrategy],
/// apply the desired [MergeStrategy] as indicated by the `conflict_strategy`.
/// If `conflict_strategy` is [ConflictStrategy::Auto], use the [MergeStrategy] `auto_strategy`.
pub fn apply_preferred_strategy<U: RelativeTo + ConflictResolver, const T: usize>(
    coords: Vec2<i32>,
    plugin: &ParsedPlugin,
    value: &str,
    old: Option<&RelativeTerrainMap<U, T>>,
    new: Option<&RelativeTerrainMap<U, T>>,
    conflict_strategy: ConflictStrategy,
    auto_strategy: &impl MergeStrategy,
) -> OptionalTerrainMap<U, T>
where
    <U as RelativeTo>::Delta: ConflictResolver,
{
    let resolve_strategy: ResolveConflictStrategy = default();
    let overwrite_strategy: OverwriteStrategy = default();
    let ignore_strategy: IgnoreStrategy = default();

    if conflict_strategy != ConflictStrategy::Auto {
        trace!(
            "({:>4}, {:>4}) {:<15} | {:<50} | Strategy = {:?}",
            coords.x,
            coords.y,
            value,
            plugin.name,
            conflict_strategy
        );
    }

    match conflict_strategy {
        ConflictStrategy::Auto => apply_strategy(coords, plugin, value, old, new, auto_strategy),
        ConflictStrategy::Resolve => {
            apply_strategy(coords, plugin, value, old, new, &resolve_strategy)
        }
        ConflictStrategy::Overwrite => {
            apply_strategy(coords, plugin, value, old, new, &overwrite_strategy)
        }
        ConflictStrategy::Ignore => {
            apply_strategy(coords, plugin, value, old, new, &ignore_strategy)
        }
    }
}

/// Given optional `old` and `new` [RelativeTerrainMap], and a desired [ConflictStrategy],
/// apply the desired [MergeStrategy] as indicated by the `conflict_strategy`.
pub fn apply_merge_strategy<U: RelativeTo + ConflictResolver, const T: usize>(
    coords: Vec2<i32>,
    plugin: &ParsedPlugin,
    value: &str,
    old: Option<&RelativeTerrainMap<U, T>>,
    new: Option<&RelativeTerrainMap<U, T>>,
    conflict_strategy: ConflictStrategy,
) -> OptionalTerrainMap<U, T>
where
    <U as RelativeTo>::Delta: ConflictResolver,
{
    let resolve_strategy: ResolveConflictStrategy = default();
    let overwrite_strategy: OverwriteStrategy = default();

    match value {
        "height_map" | "world_map_data" | "vertex_colors" | "vertex_normals" => {
            apply_preferred_strategy(
                coords,
                plugin,
                value,
                old,
                new,
                conflict_strategy,
                &resolve_strategy,
            )
        }
        "texture_indices" => apply_preferred_strategy(
            coords,
            plugin,
            value,
            old,
            new,
            conflict_strategy,
            &overwrite_strategy,
        ),
        _ => {
            // TODO(dvd): #refactor Why aren't these enums?
            panic!("unexpected value {}", value);
        }
    }
}
