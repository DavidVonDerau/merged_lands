use crate::grid_access::{GridAccessor2D, SquareGridIterator};
use crate::{ConflictResolver, MergeStrategy, RelativeTerrainMap, RelativeTo, SaveToImage, Vec2};
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
    ) -> RelativeTerrainMap<U, T>
    where
        RelativeTerrainMap<U, T>: SaveToImage,
    {
        let mut relative = [[default(); T]; T];
        let mut has_difference = [[false; T]; T];

        for coords in relative.iter_grid() {
            let lhs_diff = lhs.has_difference.get(coords);
            let rhs_diff = rhs.has_difference.get(coords);

            let mut diff = default();
            if lhs_diff && !rhs_diff {
                diff = lhs.relative.get(coords);
            } else if !lhs_diff && rhs_diff {
                diff = rhs.relative.get(coords);
            } else if !lhs_diff && !rhs_diff {
                // NOP.
            } else {
                // Conflict -- choose rhs.
                diff = rhs.relative.get(coords);
            }

            *relative.get_mut(coords) = diff;
            *has_difference.get_mut(coords) = diff != default();
        }

        RelativeTerrainMap {
            reference: lhs.reference,
            relative,
            has_difference,
        }
    }
}
