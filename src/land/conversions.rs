use crate::land::grid_access::{GridAccessor2D, SquareGridIterator};
use crate::land::terrain_map::{TerrainMap, Vec2, Vec3};
use std::default::default;
use tes3::esp::{Landscape, LandscapeFlags};

pub fn convert_terrain_map<I: Copy, U: Copy + Default, const T: usize>(
    original: &TerrainMap<I, T>,
    conversion: fn(I) -> U,
) -> TerrainMap<U, T> {
    let mut new = [[default(); T]; T];

    for coords in original.iter_grid() {
        *new.get_mut(coords) = conversion(original.get(coords));
    }

    new
}

pub fn vertex_normals(land: &Landscape) -> Option<TerrainMap<Vec3<i8>, 65>> {
    land.vertex_normals
        .as_ref()
        .map(|record| convert_terrain_map(&record.data, Vec3::from))
}

pub fn vertex_colors(land: &Landscape) -> Option<TerrainMap<Vec3<u8>, 65>> {
    land.vertex_colors
        .as_ref()
        .map(|record| convert_terrain_map(&record.data, Vec3::from))
}

pub fn world_map_data(land: &Landscape) -> Option<TerrainMap<u8, 9>> {
    land.world_map_data
        .as_ref()
        .map(|record| convert_terrain_map(&record.data, |value| value))
}

pub fn texture_indices(land: &Landscape) -> Option<TerrainMap<u16, 16>> {
    land.texture_indices
        .as_ref()
        .map(|record| convert_terrain_map(&record.data, |value| value))
}

pub fn landscape_flags(land: &Landscape) -> LandscapeFlags {
    land.landscape_flags
}

pub fn coordinates(land: &Landscape) -> Vec2<i32> {
    let coords = land.grid;
    Vec2::new(coords.0, coords.1)
}
