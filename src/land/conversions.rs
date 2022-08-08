use crate::land::grid_access::{GridAccessor2D, SquareGridIterator};
use crate::land::terrain_map::{TerrainMap, Vec2, Vec3};
use crate::land::textures::IndexVTEX;
use std::default::default;
use tes3::esp::{Landscape, LandscapeFlags};

/// Converts between [TerrainMap] using the provided `conversion` function.
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

/// Access the `vertex_normals` of a [Landscape] as a [TerrainMap].
pub fn vertex_normals(land: &Landscape) -> Option<TerrainMap<Vec3<i8>, 65>> {
    land.vertex_normals
        .as_ref()
        .map(|record| convert_terrain_map(&record.data, Vec3::from))
}

/// Access the `vertex_colors` of a [Landscape] as a [TerrainMap].
pub fn vertex_colors(land: &Landscape) -> Option<TerrainMap<Vec3<u8>, 65>> {
    land.vertex_colors
        .as_ref()
        .map(|record| convert_terrain_map(&record.data, Vec3::from))
}

/// Access the `world_map_data` of a [Landscape] as a [TerrainMap].
pub fn world_map_data(land: &Landscape) -> Option<TerrainMap<u8, 9>> {
    land.world_map_data
        .as_ref()
        .map(|record| convert_terrain_map(&record.data, |value| value))
}

/// Access the `texture_indices` of a [Landscape] as a [TerrainMap].
pub fn texture_indices(land: &Landscape) -> Option<TerrainMap<IndexVTEX, 16>> {
    land.texture_indices
        .as_ref()
        .map(|record| convert_terrain_map(&record.data, IndexVTEX::new))
}

/// Access the [LandscapeFlags] of a [Landscape].
pub fn landscape_flags(land: &Landscape) -> LandscapeFlags {
    land.landscape_flags
}

/// Access the `grid` location of a [Landscape] as a [Vec2].
pub fn coordinates(land: &Landscape) -> Vec2<i32> {
    let coords = land.grid;
    Vec2::new(coords.0, coords.1)
}
