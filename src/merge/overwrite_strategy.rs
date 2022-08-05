use crate::land::grid_access::SquareGridIterator;
use crate::land::terrain_map::Vec2;
use crate::merge::conflict::ConflictResolver;
use crate::merge::merge_strategy::MergeStrategy;
use crate::merge::relative_terrain_map::RelativeTerrainMap;
use crate::merge::relative_to::RelativeTo;
use std::default::default;

#[derive(Default)]
pub struct OverwriteStrategy {}

impl MergeStrategy for OverwriteStrategy {
    fn apply<U: RelativeTo + ConflictResolver, const T: usize>(
        &self,
        _coords: Vec2<i32>,
        _plugin: &str,
        _value: &str,
        lhs: &RelativeTerrainMap<U, T>,
        rhs: &RelativeTerrainMap<U, T>,
    ) -> RelativeTerrainMap<U, T> {
        let mut new = lhs.clone();

        for coords in new.iter_grid() {
            let lhs_diff = lhs.has_difference(coords);
            let rhs_diff = rhs.has_difference(coords);

            let mut diff = default();
            if lhs_diff && !rhs_diff {
                diff = lhs.get_difference(coords);
            } else if !lhs_diff && rhs_diff {
                diff = rhs.get_difference(coords);
            } else if !lhs_diff && !rhs_diff {
                // NOP.
            } else {
                // Conflict -- choose rhs.
                diff = rhs.get_difference(coords);
            }

            new.set_difference(coords, diff);
        }

        new
    }
}
