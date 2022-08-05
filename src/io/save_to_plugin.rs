use crate::land::conversions::convert_terrain_map;
use crate::land::height_map::calculate_vertex_heights_tes3;
use crate::land::landscape_diff::LandscapeDiff;
use crate::land::terrain_map::Vec3;
use crate::land::textures::{KnownTextures, RemappedTextures};
use crate::merge::relative_terrain_map::{recompute_vertex_normals, DefaultRelativeTerrainMap};
use crate::{Landmass, LandmassDiff};
use filetime::FileTime;
use log::{error, trace};
use std::default::default;
use std::path::PathBuf;
use tes3::esp::{
    FixedString, Header, Landscape, LandscapeFlags, Plugin, TES3Object, TextureIndices,
    VertexColors, VertexNormals, WorldMapData,
};

fn convert_landscape_diff_to_landscape(
    landscape: &LandscapeDiff,
    remapped_textures: &RemappedTextures,
) -> Landscape {
    let mut new_landscape: Landscape = default();

    assert!(!landscape.plugins.is_empty());
    for (plugin, modified_data) in landscape.plugins.iter() {
        if modified_data.is_empty() {
            continue;
        }

        trace!(
            "({:>4}, {:>4}) | {:<50} | {:?}",
            landscape.coords.x,
            landscape.coords.y,
            plugin,
            modified_data
        );
    }

    new_landscape.flags = landscape.flags;
    new_landscape.grid = (landscape.coords.x, landscape.coords.y);
    new_landscape.landscape_flags = LandscapeFlags::USES_VERTEX_HEIGHTS_AND_NORMALS
        | LandscapeFlags::USES_VERTEX_COLORS
        | LandscapeFlags::USES_TEXTURES
        | LandscapeFlags::UNKNOWN;

    let height_map = landscape
        .height_map
        .as_ref()
        .unwrap_or(&DefaultRelativeTerrainMap::HEIGHT_MAP);
    let vertex_normals = landscape
        .vertex_normals
        .as_ref()
        .unwrap_or(&DefaultRelativeTerrainMap::VERTEX_NORMALS);

    new_landscape.vertex_heights = Some(calculate_vertex_heights_tes3(&height_map.to_terrain()));

    new_landscape.vertex_normals = Some(VertexNormals {
        data: Box::new(convert_terrain_map(
            &recompute_vertex_normals(height_map, vertex_normals),
            Vec3::into,
        )),
    });

    if let Some(vertex_colors) = landscape.vertex_colors.as_ref() {
        new_landscape.vertex_colors = Some(VertexColors {
            data: Box::new(convert_terrain_map(&vertex_colors.to_terrain(), Vec3::into)),
        });
    }

    if let Some(texture_indices) = landscape.texture_indices.as_ref() {
        let mut texture_indices = texture_indices.to_terrain();

        for idx in texture_indices.flatten_mut() {
            *idx = remapped_textures.remapped_index(*idx);
        }

        new_landscape.texture_indices = Some(TextureIndices {
            data: Box::new(texture_indices),
        });
    }

    if let Some(world_map_data) = landscape.world_map_data.as_ref() {
        new_landscape.world_map_data = Some(WorldMapData {
            data: Box::new(world_map_data.to_terrain()),
        });
    }

    new_landscape
}

pub fn convert_landmass_diff_to_landmass(
    landmass: &LandmassDiff,
    remapped_textures: &RemappedTextures,
) -> Landmass {
    let mut new_landmass = Landmass::new(landmass.plugin.clone(), landmass.data.clone());

    for (coords, land) in landmass.iter_land() {
        new_landmass.land.insert(
            *coords,
            convert_landscape_diff_to_landscape(land, remapped_textures),
        );
    }

    new_landmass
}

pub fn save_plugin(name: &str, landmass: &Landmass, known_textures: &KnownTextures) {
    let merged_filepath = PathBuf::from(format!("Data Files/{}.esp", name));
    let last_modified_time = merged_filepath
        .metadata()
        .map(|metadata| FileTime::from_last_modification_time(&metadata))
        .unwrap_or_else(|_| FileTime::now());

    let mut plugin = Plugin::new();

    let header = Header {
        author: FixedString("Merged Lands by DVD".to_string()),
        description: FixedString(
            "Merges landscape changes inside of cells. Place at end of load order.".to_string(),
        ),
        // TODO(dvd): Create masters from ESMs + ESPs that contributed modified LAND or LTEX records.
        ..default()
    };
    plugin.objects.push(TES3Object::Header(header));

    for texture in known_textures.sorted() {
        plugin
            .objects
            .push(TES3Object::LandscapeTexture(texture.clone()));
    }

    for land in landmass.land.values() {
        plugin.objects.push(TES3Object::Landscape(land.clone()));
    }

    // Save the file & set the last modified time.
    match plugin.save_path(&merged_filepath) {
        Ok(_) => match filetime::set_file_mtime(merged_filepath, last_modified_time) {
            Ok(_) => {}
            Err(e) => {
                error!(
                    "Unable to set last modified date on plugin {} due to: {}",
                    name, e
                )
            }
        },
        Err(e) => {
            error!("Unable to save plugin {} due to: {}", name, e)
        }
    }

    // TODO(dvd): Save the TOML file.
}
