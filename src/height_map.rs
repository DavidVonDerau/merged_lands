use crate::grid_access::{GridAccessor2D, GridPosition2D, SquareGridIterator};
use crate::{landscape_flags, TerrainMap};
use std::default::default;
use tes3::esp::{Landscape, LandscapeFlags, VertexHeights};

pub const CELL_SIZE: usize = 65;
pub const HEIGHT_MAP_SCALE_FACTOR: i32 = 8;
pub const HEIGHT_MAP_SCALE_FACTOR_F32: f32 = HEIGHT_MAP_SCALE_FACTOR as f32;

fn truncate_gradient(gradient: &mut i32) {
    if *gradient > 127 {
        *gradient = 127;
    } else if *gradient < -128 {
        *gradient = -128;
    }
}

fn calculate_vertex_heights<const T: usize>(
    height_map: &TerrainMap<i32, T>,
) -> (f32, TerrainMap<i8, T>) {
    let mut terrain32 = [[0i32; T]; T];
    let mut terrain = [[default(); T]; T];

    let get_pixel = |y: usize, x: usize| (height_map[y][x] / HEIGHT_MAP_SCALE_FACTOR) as i32;
    let offset = get_pixel(0, 0) as f32;

    let get_pixel_with_offset = |y, x| get_pixel(y, x) - offset as i32;

    // Compute the first column.
    for y in 1..T {
        terrain32[y][0] = get_pixel_with_offset(y, 0) - get_pixel_with_offset(y - 1, 0);
        truncate_gradient(&mut terrain32[y][0]);
    }

    // Compute each row.
    for y in 0..T {
        for x in 1..T {
            terrain32[y][x] = get_pixel_with_offset(y, x) - get_pixel_with_offset(y, x - 1);
            truncate_gradient(&mut terrain32[y][x]);
        }
    }

    for coords in terrain32.iter_grid() {
        *terrain.get_mut(coords) = terrain32.get(coords) as i8;
    }

    (offset, terrain)
}

pub fn calculate_vertex_heights_tes3(height_map: &TerrainMap<i32, CELL_SIZE>) -> VertexHeights {
    let (offset, terrain) = calculate_vertex_heights(height_map);
    VertexHeights {
        offset,
        data: Box::new(terrain),
    }
}

fn calculate_height_map<const T: usize>(vertex_heights: &VertexHeights) -> TerrainMap<i32, T> {
    let mut grid_height = [[0; T]; T];
    let mut height = vertex_heights.offset as i32;

    for y in 0..T {
        for x in 0..T {
            let coords = GridPosition2D { x, y };
            height += vertex_heights.data.get(coords) as i32;
            *grid_height.get_mut(coords) = height;
        }

        height = grid_height.get(GridPosition2D { x: 0, y });
    }

    for coords in grid_height.iter_grid() {
        *grid_height.get_mut(coords) *= HEIGHT_MAP_SCALE_FACTOR;
    }

    grid_height
}

pub fn try_calculate_height_map(land: &Landscape) -> Option<TerrainMap<i32, 65>> {
    let included_data = landscape_flags(land);
    if !included_data.contains(LandscapeFlags::USES_VERTEX_HEIGHTS_AND_NORMALS) {
        return None;
    }

    let grid_height = calculate_height_map(land.vertex_heights.as_ref().unwrap());

    // IMPORTANT(dvd): Sanity check that this conversion works.
    let vertex_heights_into = calculate_vertex_heights_tes3(&grid_height);
    let grid_heights_return = calculate_height_map::<65>(&vertex_heights_into);
    for coords in grid_height.iter_grid() {
        let lhs = grid_height.get(coords);
        let rhs = grid_heights_return.get(coords);
        assert_eq!(
            lhs,
            rhs,
            "delta did not match at {:?} (possibly due to height? {} {})",
            coords,
            land.vertex_heights.as_ref().unwrap().offset,
            vertex_heights_into.offset
        );
    }

    Some(grid_height)
}
