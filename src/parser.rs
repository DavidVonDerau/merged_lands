use crate::terrain::{TerrainMap, Vec2, Vec3};
use crate::{GridAccessor2D, SquareGridIterator};
use std::default::default;
use std::sync::Arc;
use std::{io, str};
use tes3::esp::{Landscape, LandscapeFlags, Plugin};

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
    land.vertex_normals.as_ref().map(|record| {
        convert_terrain_map(&record.data, |value| Vec3 {
            x: value[0],
            y: value[1],
            z: value[2],
        })
    })
}

pub fn vertex_colors(land: &Landscape) -> Option<TerrainMap<Vec3<u8>, 65>> {
    land.vertex_colors.as_ref().map(|record| {
        convert_terrain_map(&record.data, |value| Vec3 {
            x: value[0],
            y: value[1],
            z: value[2],
        })
    })
}

pub fn world_map_data(land: &Landscape) -> Option<TerrainMap<u8, 9>> {
    // TODO(dvd): remove this conversion 'value as u8' when library is fixed
    land.world_map_data
        .as_ref()
        .map(|record| convert_terrain_map(&record.data, |value| value as u8))
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
    Vec2 {
        x: coords.0,
        y: coords.1,
    }
}

pub fn parse_records(plugin_name: &str) -> io::Result<Arc<Plugin>> {
    let mut plugin = Plugin::new();
    let file_path = format!("Data Files/{}", plugin_name);
    plugin.load_path(file_path)?;
    Ok(Arc::new(plugin))
}
