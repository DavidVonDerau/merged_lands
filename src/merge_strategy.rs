use crate::{ConflictResolver, OptionalTerrain, RelativeTerrainMap, RelativeTo, SaveToImage, Vec2};

pub trait MergeStrategy {
    fn apply<U: RelativeTo + ConflictResolver, const T: usize>(
        &self,
        coords: Vec2<i32>,
        plugin: &str,
        value: &str,
        lhs: &RelativeTerrainMap<U, T>,
        rhs: &RelativeTerrainMap<U, T>,
    ) -> RelativeTerrainMap<U, T>
    where
        RelativeTerrainMap<U, T>: SaveToImage;
}

pub fn apply_merge_strategy<U: RelativeTo + ConflictResolver, F: MergeStrategy, const T: usize>(
    coords: Vec2<i32>,
    plugin: &str,
    value: &str,
    old: OptionalTerrain<U, T>,
    new: OptionalTerrain<U, T>,
    strategy: &F,
) -> OptionalTerrain<U, T>
where
    RelativeTerrainMap<U, T>: SaveToImage,
{
    if old.is_some() && new.is_some() {
        let merged = strategy.apply(
            coords,
            plugin,
            value,
            old.as_ref().unwrap(),
            new.as_ref().unwrap(),
        );

        Some(merged)
    } else if old.is_some() {
        old
    } else if new.is_some() {
        new
    } else {
        None
    }
}
