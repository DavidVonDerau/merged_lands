use crate::land::conversions::landscape_flags;
use crate::land::grid_access::{GridAccessor2D, Index2D, SquareGridIterator};
use crate::land::terrain_map::{TerrainMap, Vec3};
use log::warn;
use num_traits::Pow;
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
            let coords = Index2D::new(x, y);
            height += vertex_heights.data.get(coords) as i32;
            *grid_height.get_mut(coords) = height;
        }

        height = grid_height.get(Index2D::new(0, y));
    }

    for coords in grid_height.iter_grid() {
        *grid_height.get_mut(coords) *= HEIGHT_MAP_SCALE_FACTOR;
    }

    grid_height
}

pub fn calculate_vertex_normals_map<const T: usize>(
    height_map: &TerrainMap<i32, T>,
) -> TerrainMap<Vec3<i8>, T> {
    fn fix_coords<const T: usize>(coords: Index2D) -> Index2D {
        let x = if coords.x + 1 == T {
            coords.x - 1
        } else {
            coords.x
        };

        let y = if coords.y + 1 == T {
            coords.y - 1
        } else {
            coords.y
        };

        Index2D::new(x, y)
    }

    let mut terrain = [[default(); T]; T];

    for coords in height_map.iter_grid() {
        let fixed_coords = fix_coords::<T>(coords);

        let coords_x1 = Index2D::new(fixed_coords.x + 1, fixed_coords.y);

        let h = height_map.get(fixed_coords) as f32 / HEIGHT_MAP_SCALE_FACTOR_F32;
        let x1 = height_map.get(coords_x1) as f32 / HEIGHT_MAP_SCALE_FACTOR_F32;
        let v1 = Vec3 {
            x: 128f32 / HEIGHT_MAP_SCALE_FACTOR_F32,
            y: 0f32,
            z: (x1 - h) as f32,
        };

        let coords_y1 = Index2D::new(fixed_coords.x, fixed_coords.y + 1);
        let y1 = height_map.get(coords_y1) as f32 / HEIGHT_MAP_SCALE_FACTOR_F32;
        let v2 = Vec3 {
            x: 0f32,
            y: 128f32 / HEIGHT_MAP_SCALE_FACTOR_F32,
            z: (y1 - h) as f32,
        };

        let mut normal = Vec3 {
            x: v1.y * v2.z - v1.z * v2.y,
            y: v1.z * v2.x - v1.x * v2.z,
            z: v1.x * v2.y - v1.y * v2.x,
        };

        let squared: f32 = normal.x.pow(2) + normal.y.pow(2) + normal.z.pow(2);
        let hyp: f32 = squared.sqrt() / 127.0f32;

        normal.x /= hyp;
        normal.y /= hyp;
        normal.z /= hyp;

        *terrain.get_mut(coords) = Vec3::new(normal.x as i8, normal.y as i8, normal.z as i8);
    }

    terrain
}
pub fn try_calculate_height_map(land: &Landscape) -> Option<TerrainMap<i32, 65>> {
    let included_data = landscape_flags(land);
    if !included_data.contains(LandscapeFlags::USES_VERTEX_HEIGHTS_AND_NORMALS) {
        return None;
    }

    let Some(grid_height) = land.vertex_heights.as_ref().map(calculate_height_map) else {
        warn!(
            "({:>4}, {:>4}) {:<15} | missing vertex_heights",
            land.grid.0,
            land.grid.1,
            "height_map"
        );
        return None;
    };

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
            land.vertex_heights.as_ref().expect("safe").offset,
            vertex_heights_into.offset
        );
    }

    Some(grid_height)
}
