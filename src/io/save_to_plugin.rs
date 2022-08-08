use crate::io::meta_schema::{MetaType, PluginMeta, VersionedPluginMeta};
use crate::io::parsed_plugins::{meta_name, sort_plugins, ParsedPlugin, ParsedPlugins};
use crate::land::conversions::convert_terrain_map;
use crate::land::height_map::calculate_vertex_heights_tes3;
use crate::land::landscape_diff::LandscapeDiff;
use crate::land::terrain_map::Vec3;
use crate::land::textures::{KnownTextures, RemappedTextures};
use crate::merge::cells::ModifiedCell;
use crate::merge::relative_terrain_map::{recompute_vertex_normals, DefaultRelativeTerrainMap};
use crate::{Landmass, LandmassDiff, Vec2};
use anyhow::{anyhow, Context, Result};
use filesize::file_real_size;
use filetime::FileTime;
use hashbrown::{HashMap, HashSet};
use itertools::Itertools;
use log::{debug, trace, warn};
use owo_colors::OwoColorize;
use std::default::default;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tes3::esp::{
    FixedString, Header, Landscape, LandscapeFlags, Plugin, TES3Object, TextureIndices,
    VertexColors, VertexNormals, WorldMapData,
};
use time::format_description;

/// Converts a [LandscapeDiff] to a [Landscape].
/// The [RemappedTextures] is used to update any texture indices.
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
            &recompute_vertex_normals(height_map, Some(vertex_normals)),
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
            data: Box::new(convert_terrain_map(&texture_indices, |v| v.as_u16())),
        });
    }

    if let Some(world_map_data) = landscape.world_map_data.as_ref() {
        new_landscape.world_map_data = Some(WorldMapData {
            data: Box::new(world_map_data.to_terrain()),
        });
    }

    new_landscape
}

/// Converts a [LandmassDiff] to a [Landmass].
/// The [RemappedTextures] is used to update any texture indices.
pub fn convert_landmass_diff_to_landmass(
    landmass: &LandmassDiff,
    remapped_textures: &RemappedTextures,
) -> Landmass {
    let mut new_landmass = Landmass::new(landmass.plugin.clone());

    for (coords, land) in landmass.sorted() {
        let landscape = convert_landscape_diff_to_landscape(land, remapped_textures);
        let last_plugin = land.plugins.last().expect("safe").clone().0;
        new_landmass.insert_land(*coords, &last_plugin, &landscape);
    }

    new_landmass
}

/// Creates a master record for plugin `name` by appending the size
/// of the file in bytes to the tuple `(name, file_size)`.
fn to_master_record(data_files: &str, name: String) -> (String, u64) {
    let merged_filepath: PathBuf = [data_files, &name].iter().collect();
    let file_size = file_real_size(merged_filepath).unwrap_or(0);
    (name, file_size)
}

/// Saves the [Landmass] with [KnownTextures].
pub fn save_plugin(
    data_files: &str,
    name: &str,
    landmass: &Landmass,
    known_textures: &KnownTextures,
    cells: &HashMap<Vec2<i32>, ModifiedCell>,
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

        let mut add_dependency =
            |dependency: &Arc<ParsedPlugin>| dependencies.insert(dependency.name.clone());

        // Add plugins that contribute textures.
        for texture in known_textures.sorted() {
            add_dependency(&texture.plugin);
        }

        // Add plugins used for the land.
        for plugin in landmass.plugins.values() {
            add_dependency(plugin);
        }

        // Add plugins that modified cells.
        for (coords, _) in landmass.sorted() {
            let cell = cells.get(coords).with_context(|| {
                anyhow!(
                    "Could not find CELL record for LAND with coordinates {:?}",
                    coords
                )
            })?;

            let plugin = cell.plugins.last().expect("safe");
            if add_dependency(plugin) {
                trace!(
                    "({:>4}, {:>4})   | {:<50} | {}",
                    coords.x,
                    coords.y,
                    plugin.name,
                    if cell.inner.id.is_empty() {
                        cell.inner.region.as_deref().unwrap_or("")
                    } else {
                        cell.inner.id.as_str()
                    }
                );
            }
        }

        let mut masters = dependencies.drain().collect_vec();

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

    let time_format =
        format_description::parse("[year]-[month]-[day] [hour]:[minute]").expect("safe");

    let generated_time = time::OffsetDateTime::now_local()
        .unwrap_or_else(|e| {
            warn!(
                "{}",
                format!("Unknown local date time offset: {}", e.bold()).yellow()
            );
            time::OffsetDateTime::now_utc()
        })
        .format(&time_format)
        .unwrap_or_else(|_| "unknown".into());

    let description = format!(
        "Merges landscape changes inside of cells. Place at end of load order. Generated at {}.",
        generated_time
    );

    let author = "Merged Lands by DVD".to_string();

    let header = Header {
        author: FixedString(author),
        description: FixedString(description.clone()),
        masters,
        ..default()
    };

    debug!("Saving 1 TES3 record");
    plugin.objects.push(TES3Object::Header(header));

    debug!("Saving {} LTEX records", known_textures.len());
    for known_texture in known_textures.sorted() {
        trace!(
            "Texture | {:>4} | {:<30} | {}",
            known_texture.index().as_u16(),
            known_texture.id(),
            known_texture.plugin.name
        );
        plugin.objects.push(TES3Object::LandscapeTexture(
            known_texture.clone_landscape_texture(),
        ));
    }

    debug!("Saving {} CELL and LAND records", landmass.land.len());
    for (coords, land) in landmass.sorted() {
        let cell = cells.get(coords).expect("safe");
        plugin.objects.push(TES3Object::Cell(cell.inner.clone()));
        plugin.objects.push(TES3Object::Landscape(land.clone()));
    }

    let meta_name = meta_name(name);
    let merged_meta: PathBuf = [data_files, &meta_name].iter().collect();

    let meta = VersionedPluginMeta::V0(PluginMeta {
        meta_type: MetaType::MergedLands,
        height_map: Default::default(),
        vertex_colors: Default::default(),
        texture_indices: Default::default(),
        world_map_data: Default::default(),
    });

    trace!("Saving meta file {}", meta_name);
    fs::write(merged_meta, toml::to_string(&meta).expect("safe"))
        .with_context(|| anyhow!("Unable to save plugin meta {}", meta_name))?;

    trace!("Saving file {}", name);
    plugin
        .save_path(&merged_filepath)
        .with_context(|| anyhow!("Unable to save plugin {}", name))?;

    trace!(" - Description: {}", description);

    trace!("Updating last modified time on {}", name);
    filetime::set_file_mtime(merged_filepath, last_modified_time)
        .with_context(|| anyhow!("Unable to set last modified date on plugin {}", name))?;

    Ok(())
}
