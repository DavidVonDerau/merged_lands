use crate::grid_access::{GridAccessor2D, GridPosition2D, SquareGridIterator};
use crate::{DataFlags, TerrainMap, LAND};

pub fn calculate_height_map<const T: usize>(land: &LAND) -> Option<TerrainMap<i32, T>> {
    if !land.included_data.contains(DataFlags::VNML_VHGT_WNAM) {
        return None;
    }

    let height_data = land.height_data.as_ref().unwrap();

    let mut grid_height = [[0; T]; T];
    let mut height = height_data.offset as i32;
    for y in 0..T {
        for x in 0..T {
            let coords = GridPosition2D { x, y };
            height += height_data.differences.get(coords) as i32;
            *grid_height.get_mut(coords) = height;
        }

        height = grid_height.get(GridPosition2D { x: 0, y });
    }

    let scale_factor = 8;
    for coords in grid_height.iter_grid() {
        *grid_height.get_mut(coords) *= scale_factor;
    }

    Some(grid_height)
}
