use crate::land::terrain_map::Vec2;
use crate::merge::conflict::ConflictResolver;
use crate::merge::relative_terrain_map::{OptionalTerrainMap, RelativeTerrainMap};
use crate::merge::relative_to::RelativeTo;
use crate::ParsedPlugin;

pub trait MergeStrategy {
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

pub fn apply_merge_strategy<U: RelativeTo + ConflictResolver, const T: usize>(
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
