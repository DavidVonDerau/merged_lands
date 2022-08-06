use crate::io::parsed_plugins::sort_plugins;
use crate::land::conversions::convert_terrain_map;
use crate::land::height_map::calculate_vertex_heights_tes3;
use crate::land::landscape_diff::LandscapeDiff;
use crate::land::terrain_map::Vec3;
use crate::land::textures::{KnownTextures, RemappedTextures};
use crate::merge::relative_terrain_map::{recompute_vertex_normals, DefaultRelativeTerrainMap};
use crate::{Landmass, LandmassDiff, ParsedPlugins};
use anyhow::{anyhow, Context, Result};
use filesize::file_real_size;
use filetime::FileTime;
use itertools::Itertools;
use log::{debug, trace};
use std::collections::HashSet;
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
            plugin.name,
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
    let mut new_landmass = Landmass::new(landmass.plugin.clone());

    for (coords, land) in landmass.iter_land() {
        new_landmass.land.insert(
            *coords,
            convert_landscape_diff_to_landscape(land, remapped_textures),
        );
        new_landmass
            .plugins
            .insert(*coords, land.plugins.last().expect("safe").0.clone());
    }

    new_landmass
}

fn to_master_record(data_files: &str, name: String) -> (String, u64) {
    let merged_filepath: PathBuf = [data_files, &name].iter().collect();
    let file_size = file_real_size(merged_filepath).unwrap_or(0);
    (name, file_size)
}

pub fn save_plugin(
    data_files: &str,
    name: &str,
    landmass: &Landmass,
    known_textures: &KnownTextures,
) -> Result<()> {
    ParsedPlugins::check_data_files(data_files)
        .with_context(|| anyhow!("Unable to save file {}", name))?;

    let merged_filepath: PathBuf = [data_files, name].iter().collect();
    let last_modified_time = merged_filepath
        .metadata()
        .map(|metadata| FileTime::from_last_modification_time(&metadata))
        .unwrap_or_else(|_| FileTime::now());

    let mut plugin = Plugin::new();

    debug!("Determining plugin dependencies");

    let masters = {
        let mut dependencies = HashSet::new();

        // Add plugins that contribute textures.
        for texture in known_textures.sorted() {
            dependencies.insert(&*texture.plugin);
        }

        // Add plugins used for the land.
        for plugin in landmass.plugins.values() {
            dependencies.insert(plugin);
        }

        let mut masters = dependencies
            .drain()
            .map(|plugin| plugin.name.to_string())
            .collect_vec();

        sort_plugins(data_files, &mut masters)
            .with_context(|| anyhow!("Unknown load order for {} dependencies", name))?;

        Some(
            masters
                .into_iter()
                .map(|plugin| to_master_record(data_files, plugin))
                .collect_vec(),
        )
    };

    for (idx, master) in masters.as_ref().expect("safe").iter().enumerate() {
        trace!("Master  | {:>4} | {:<50} | {:>10}", idx, master.0, master.1);
    }

    let header = Header {
        author: FixedString("Merged Lands by DVD".to_string()),
        description: FixedString(
            "Merges landscape changes inside of cells. Place at end of load order.".to_string(),
        ),
        masters,
        ..default()
    };
    plugin.objects.push(TES3Object::Header(header));

    debug!("Saving {} LTEX records", known_textures.len());
    for known_texture in known_textures.sorted() {
        trace!(
            "Texture | {:>4} | {:<30} | {}",
            known_texture.index(),
            known_texture.id(),
            known_texture.plugin.name
        );
        plugin.objects.push(TES3Object::LandscapeTexture(
            known_texture.clone_landscape_texture(),
        ));
    }

    debug!("Saving {} LAND records", landmass.land.len());
    for land in landmass.land.values() {
        plugin.objects.push(TES3Object::Landscape(land.clone()));
    }

    // Save the file & set the last modified time.

    plugin
        .save_path(&merged_filepath)
        .with_context(|| anyhow!("Unable to save plugin {}", name))?;

    filetime::set_file_mtime(merged_filepath, last_modified_time)
        .with_context(|| anyhow!("Unable to set last modified date on plugin {}", name))?;

    // TODO(dvd): Save the TOML file.

    Ok(())
}
