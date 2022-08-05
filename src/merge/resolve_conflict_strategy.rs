use crate::land::grid_access::SquareGridIterator;
use crate::land::terrain_map::Vec2;
use crate::merge::conflict::{ConflictResolver, ConflictType};
use crate::merge::merge_strategy::MergeStrategy;
use crate::merge::relative_terrain_map::RelativeTerrainMap;
use crate::merge::relative_to::RelativeTo;
use std::default::default;

#[derive(Default)]
pub struct ResolveConflictStrategy {}

impl MergeStrategy for ResolveConflictStrategy {
    fn apply<U: RelativeTo, const T: usize>(
        &self,
        _coords: Vec2<i32>,
        _plugin: &str,
        _value: &str,
        lhs: &RelativeTerrainMap<U, T>,
        rhs: &RelativeTerrainMap<U, T>,
    ) -> RelativeTerrainMap<U, T>
    where
        <U as RelativeTo>::Delta: ConflictResolver,
    {
        let mut new = lhs.clone();

        let params = default();

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
                let lhs_diff = lhs.get_difference(coords);
                let rhs_diff = rhs.get_difference(coords);

                match lhs_diff.average(rhs_diff, &params) {
                    None => {
                        diff = lhs.get_difference(coords);
                    }
                    Some(ConflictType::Minor(value)) => {
                        diff = value;
                    }
                    Some(ConflictType::Major(value)) => {
                        diff = value;
                    }
                }
            }

            new.set_difference(coords, diff);
        }

        new
    }
}
