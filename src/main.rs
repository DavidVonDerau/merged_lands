#![feature(slice_flatten)]
#![feature(let_else)]
#![feature(default_free_fn)]
#![feature(try_blocks)]
#![feature(anonymous_lifetime_in_impl_trait)]
#![feature(map_many_mut)]

mod io;
mod land;
mod merge;
mod repair;

use crate::io::parsed_plugins::{ParsedPlugin, ParsedPlugins};
use crate::io::save_to_image::save_landmass_images;
use crate::io::save_to_plugin::{convert_landmass_diff_to_landmass, save_plugin};
use crate::land::conversions::{coordinates, landscape_flags};
use crate::land::landscape_diff::LandscapeDiff;
use crate::land::terrain_map::{LandData, Vec2};
use crate::land::textures::{KnownTextures, RemappedTextures};
use crate::merge::merge_strategy::apply_merge_strategy;
use crate::merge::overwrite_strategy::OverwriteStrategy;
use crate::merge::relative_terrain_map::{IsModified, RelativeTerrainMap};
use crate::merge::resolve_conflict_strategy::ResolveConflictStrategy;
use crate::repair::cleaning::{clean_known_textures, clean_landmass_diff};
use crate::repair::seam_detection::repair_landmass_seams;

use anyhow::Result;
use clap::Command;
use itertools::Itertools;
use log::{debug, error, info, trace};
use mimalloc::MiMalloc;
use owo_colors::OwoColorize;
use simplelog::{
    ColorChoice, CombinedLogger, ConfigBuilder, LevelFilter, LevelPadding, TermLogger,
    TerminalMode, WriteLogger,
};
use std::collections::{HashMap, HashSet};
use std::default::default;
use std::fs::File;
use std::process::exit;
use std::sync::Arc;
use std::time::Instant;
use tes3::esp::{Landscape, LandscapeFlags, LandscapeTexture, ObjectFlags};

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

pub struct Landmass {
    plugin: Arc<ParsedPlugin>,
    land: HashMap<Vec2<i32>, Landscape>,
    plugins: HashMap<Vec2<i32>, Arc<ParsedPlugin>>,
}

impl Landmass {
    fn new(plugin: Arc<ParsedPlugin>) -> Self {
        Self {
            plugin,
            land: HashMap::new(),
            plugins: HashMap::new(),
        }
    }
}

pub struct LandmassDiff {
    plugin: Arc<ParsedPlugin>,
    land: HashMap<Vec2<i32>, LandscapeDiff>,
    possible_seams: HashSet<Vec2<i32>>,
}

impl LandmassDiff {
    fn new(plugin: Arc<ParsedPlugin>) -> Self {
        Self {
            plugin,
            land: HashMap::new(),
            possible_seams: HashSet::new(),
        }
    }

    fn iter_land(&self) -> impl Iterator<Item = (&Vec2<i32>, &LandscapeDiff)> {
        self.land.iter().sorted_by_key(|f| (f.0.x, f.0.y))
    }
}

fn main() -> Result<()> {
    Command::new("merged_lands").get_matches();

    init_log();

    const STACK_SIZE: usize = 8 * 1024 * 1024;

    let work_thread = std::thread::Builder::new()
        .stack_size(STACK_SIZE)
        .spawn(merge_all)
        .expect("unable to create worker thread");

    if let Err(e) = work_thread.join().expect("unable to join worker thread") {
        error!(
            "{}",
            format!("An unexpected error occurred: {:?}", e.bold()).bright_red()
        );
        exit(1);
    }

    Ok(())
}

fn merge_all() -> Result<()> {
    let start = Instant::now();

    let mut known_textures = KnownTextures::new();

    // TODO(dvd): Read CLI args, or config args.

    // STEP 1:
    // For each Plugin, ordered by last modified:
    //  - Get or create reference landmass.
    //      - References are created by a list of ESMs / ESPs.
    //      - By default, the references are pulled from the TES3 header.
    //      - If the plugin has an associated `.mergedlands.meta`, read additional references from that.
    //      - Order the list by ESMs then ESPs, then within each category, order by last modified date.
    //      - [WARN] The current plugin loads before one or more of the references.
    //      - Calculate the "naive" TES3 merge of the ordered ESMs / ESPs.
    //  - Calculate diff from reference landmass.
    //  => return LandmassDiff

    // [IMPLEMENTATION NOTE] Whenever an ESM or ESP is loaded, all LTEX records are registered with
    // the KnownTextures and all texture indices in LAND records are updated accordingly.

    // [IMPLEMENTATION NOTE] Each loaded Plugin is stored in an Arc<...> with any data from the
    // optional `.mergedlands.toml` if it existed. The Arc<...> is copied into each LandscapeDiff.
    info!(":: Parsing Plugins ::");

    let data_files = "Data Files";
    let file_name = "Merged Lands.esp";
    let plugin_names = None;

    let parsed_plugins = ParsedPlugins::new(data_files, plugin_names)?;

    let reference_landmass = Arc::new(create_tes3_landmass(
        "ReferenceLandmass.esp",
        parsed_plugins.masters.iter(),
        &mut known_textures,
    ));

    // TODO(dvd): Generate ESP with vertex colors showing minor vs major conflicts.
    // TODO(dvd): Support "ignored" maps for hiding differences that we don't care about.

    let modded_landmasses = parsed_plugins
        .plugins
        .iter()
        .flat_map(|plugin| {
            if plugin.name == file_name {
                // TODO(dvd): Print error if a plugin of type merged_lands is found, but it has a different name.
                // TODO(dvd): Replace this hack with the meta file.
                return None;
            }

            try_create_landmass(plugin, &mut known_textures)
                .map(|landmass| find_landmass_diff(&landmass, reference_landmass.clone()))
        })
        .collect_vec();

    debug!(
        "Found {} masters and {} plugins",
        parsed_plugins.masters.len(),
        parsed_plugins.plugins.len(),
    );
    debug!("Found {} unique LTEX records", known_textures.len());
    debug!("{} plugins contain LAND records", modded_landmasses.len());

    // STEP 2:
    // Create the MergedLands.esp:
    //  - Calculate the "naive" TES3 merge of the ordered ESMs.
    info!(":: Creating Reference Land ::");

    debug!(
        "Reference contains {} LAND records",
        reference_landmass.land.len()
    );

    let mut merged_lands = create_merged_lands_from_reference(reference_landmass);

    // STEP 3:
    // For each LandmassDiff, [IMPLEMENTATION NOTE] same order as Plugin:
    //  - Merge into `MergedLands.esp`.
    //     - If LAND does not exist in MergedLands.esp, insert.
    //     - Else, apply merge strategies.
    //        - Each merge is applied to the result of any previous merge.
    //        - Each merge is tracked so it can be referenced in the future.
    //        - Merge strategies may use the optional `.mergedlands.toml` for conflict resolution.
    //  - Iterate through updated landmass and check for seams on any modified cell.
    info!(":: Merging Lands ::");

    for modded_landmass in modded_landmasses.iter() {
        merge_landmass_into(&mut merged_lands, modded_landmass);
        repair_landmass_seams(&mut merged_lands);
    }

    // STEP 4:
    //  - Produce images of the final merge results.
    info!(":: Summarizing Conflicts ::");

    for modded_landmass in modded_landmasses.iter() {
        save_landmass_images(&mut merged_lands, modded_landmass);
    }

    // STEP 5:
    // - Iterate through cells in MergedLands.esp and drop anything that is unchanged from the
    //   reference landmass created for MergedLands.esp.
    // - Update all LandData flags to match TES3 expectations.
    // - Run a final seam detection and assert that no seams were found.
    // [IMPLEMENTATION NOTE] This is an optimization to make MergedLands.esp friendlier.
    info!(":: Cleaning Land ::");

    clean_landmass_diff(&mut merged_lands);

    // ---------------------------------------------------------------------------------------------
    // [IMPLEMENTATION NOTE] Below this line, the merged landmass cannot be diff'd against plugins.
    // ---------------------------------------------------------------------------------------------

    // STEP 6:
    // Update LTEX records to only include textures in use in modified cells.
    info!(":: Updating LTEX Records ::");

    let remapped_textures =
        clean_known_textures(&parsed_plugins, &merged_lands, &mut known_textures);

    // STEP 7:
    // Convert "height map" representation of LAND records to "xy delta + offset" representation.
    // Remap texture indices.
    info!(":: Converting to LAND Records ::");

    let landmass = convert_landmass_diff_to_landmass(&merged_lands, &remapped_textures);

    // STEP 7:
    // Save to an ESP.
    //  - [IMPLEMENTATION NOTE] Reuse last modified date if the ESP already exists.
    info!(":: Saving ::");

    save_plugin(data_files, file_name, &landmass, &known_textures)?;

    info!(":: Finished ::");
    info!("Time Elapsed: {:?}", Instant::now().duration_since(start));

    Ok(())
}

fn init_log() {
    let config = ConfigBuilder::default()
        .set_time_level(LevelFilter::Off)
        .set_thread_level(LevelFilter::Off)
        .set_location_level(LevelFilter::Off)
        .set_target_level(LevelFilter::Off)
        .set_level_padding(LevelPadding::Right)
        .build();

    let log_file_name = "merged_lands.log";
    let write_logger = File::create(log_file_name)
        .map(|file| WriteLogger::new(LevelFilter::Trace, config.clone(), file));

    let term_logger = TermLogger::new(
        LevelFilter::Trace,
        config,
        TerminalMode::Mixed,
        ColorChoice::Auto,
    );

    match write_logger {
        Ok(write_logger) => {
            CombinedLogger::init(vec![term_logger, write_logger]).expect("safe");
            trace!("Log contents will be saved to {}", log_file_name);
        }
        Err(e) => {
            CombinedLogger::init(vec![term_logger]).expect("safe");
            error!(
                "{} {}",
                format!("Failed to create log file at {}", log_file_name.bold()).bright_red(),
                format!("due to: {:?}", e.bold()).bright_red()
            );
        }
    }
}

fn try_copy_landscape_and_remap_textures(
    plugin: &Arc<ParsedPlugin>,
    remapped_textures: &RemappedTextures,
) -> Option<Landmass> {
    let mut landmass = Landmass::new(plugin.clone());

    for land in landmass.plugin.records.objects_of_type::<Landscape>() {
        let mut updated_land = land.clone();
        if let Some(texture_indices) = updated_land.texture_indices.as_mut() {
            for idx in texture_indices.data.flatten_mut() {
                *idx = remapped_textures.remapped_index(*idx);
            }
        }
        landmass.land.insert(coordinates(land), updated_land);
    }

    if !landmass.land.is_empty() {
        Some(landmass)
    } else {
        None
    }
}

fn try_create_landmass(
    plugin: &Arc<ParsedPlugin>,
    known_textures: &mut KnownTextures,
) -> Option<Landmass> {
    let mut remapped_textures = RemappedTextures::new(known_textures);
    for texture in plugin.records.objects_of_type::<LandscapeTexture>() {
        known_textures.add_remapped_texture(plugin, texture, &mut remapped_textures);
    }

    try_copy_landscape_and_remap_textures(plugin, &remapped_textures)
}

fn merge_tes3_landscape(lhs: &Landscape, rhs: &Landscape) -> Landscape {
    let mut land = lhs.clone();

    let mut old_data = landscape_flags(lhs);
    let new_data = landscape_flags(rhs);

    assert_eq!(lhs.flags, rhs.flags, "expected identical LAND flags");
    assert!(
        !rhs.flags.contains(ObjectFlags::DELETED),
        "tried to add deleted LAND"
    );

    if new_data.contains(LandscapeFlags::USES_VERTEX_HEIGHTS_AND_NORMALS) {
        if let Some(vertex_heights) = rhs.vertex_heights.as_ref() {
            old_data |= LandscapeFlags::USES_VERTEX_HEIGHTS_AND_NORMALS;
            land.vertex_heights = Some(vertex_heights.clone());
        }
        if let Some(vertex_normals) = rhs.vertex_normals.as_ref() {
            old_data |= LandscapeFlags::USES_VERTEX_HEIGHTS_AND_NORMALS;
            land.vertex_normals = Some(vertex_normals.clone());
        }
    }

    if new_data.contains(LandscapeFlags::USES_VERTEX_COLORS) {
        if let Some(vertex_colors) = rhs.vertex_colors.as_ref() {
            old_data |= LandscapeFlags::USES_VERTEX_COLORS;
            land.vertex_colors = Some(vertex_colors.clone());
        }
    }

    if new_data.contains(LandscapeFlags::USES_TEXTURES) {
        if let Some(texture_indices) = rhs.texture_indices.as_ref() {
            old_data |= LandscapeFlags::USES_TEXTURES;
            land.texture_indices = Some(texture_indices.clone());
        }
    }

    if new_data.intersects(LandscapeFlags::USES_WORLD_MAP_DATA) {
        if let Some(world_map_data) = rhs.world_map_data.as_ref() {
            land.world_map_data = Some(world_map_data.clone());
        }
    }

    land.landscape_flags = old_data;

    land
}

fn merge_tes3_landmasses(
    plugin: &Arc<ParsedPlugin>,
    landmasses: impl Iterator<Item = Landmass>,
) -> Landmass {
    let mut merged_landmass = Landmass::new(plugin.clone());

    for landmass in landmasses {
        for (coords, land) in landmass.land.iter() {
            if merged_landmass.land.contains_key(coords) {
                let merged_land =
                    merge_tes3_landscape(merged_landmass.land.get(coords).expect("safe"), land);
                merged_landmass.land.insert(*coords, merged_land);
            } else {
                merged_landmass.land.insert(*coords, land.clone());
            }

            merged_landmass
                .plugins
                .insert(*coords, landmass.plugin.clone());
        }
    }

    merged_landmass
}

fn find_landmass_diff(landmass: &Landmass, reference: Arc<Landmass>) -> LandmassDiff {
    let mut landmass_diff = LandmassDiff::new(landmass.plugin.clone());

    let is_cantons = landmass.plugin.name == "Cantons_on_the_Global_Map_v1.1.esp";

    for (coords, land) in landmass.land.iter() {
        let reference_land = reference.land.get(coords);

        let allowed_data = if is_cantons {
            // TODO(dvd): Replace this hack with the meta file.
            LandData::WORLD_MAP
        } else {
            landscape_flags(land).into()
        };

        landmass_diff.land.insert(
            *coords,
            LandscapeDiff::from_difference(land, reference_land, allowed_data),
        );
    }

    landmass_diff
}

fn merge_landscape_diff(
    plugin: &Arc<ParsedPlugin>,
    old: &LandscapeDiff,
    new: &LandscapeDiff,
) -> LandscapeDiff {
    let mut merged = old.clone();
    merged.plugins.push((plugin.clone(), new.modified_data()));

    // TODO(dvd): Support changing the merge strategy.
    // TODO(dvd): Add "Ignore" strategy -- e.g. for use with stuff like patched BCoM docks.

    let merge_strategy: ResolveConflictStrategy = default();
    let overwrite_strategy: OverwriteStrategy = default();

    let coords = merged.coords;

    merged.height_map = apply_merge_strategy(
        coords,
        plugin,
        "height_map",
        old.height_map.as_ref(),
        new.height_map.as_ref(),
        &merge_strategy,
    );

    merged.vertex_normals = apply_merge_strategy(
        coords,
        plugin,
        "vertex_normals",
        old.vertex_normals.as_ref(),
        new.vertex_normals.as_ref(),
        &merge_strategy,
    );

    if let Some(vertex_normals) = merged.vertex_normals.as_ref() {
        merged.vertex_normals = Some(LandscapeDiff::apply_mask(
            vertex_normals,
            merged
                .height_map
                .as_ref()
                .map(RelativeTerrainMap::differences),
        ));
    }

    if merged.vertex_normals.is_modified() {
        assert!(merged.height_map.is_modified());
    }

    merged.world_map_data = apply_merge_strategy(
        coords,
        plugin,
        "world_map_data",
        old.world_map_data.as_ref(),
        new.world_map_data.as_ref(),
        &merge_strategy,
    );

    merged.vertex_colors = apply_merge_strategy(
        coords,
        plugin,
        "vertex_colors",
        old.vertex_colors.as_ref(),
        new.vertex_colors.as_ref(),
        &merge_strategy,
    );

    merged.texture_indices = apply_merge_strategy(
        coords,
        plugin,
        "texture_indices",
        old.texture_indices.as_ref(),
        new.texture_indices.as_ref(),
        &overwrite_strategy,
    );

    merged
}

fn merge_landmass_into(merged: &mut LandmassDiff, plugin: &LandmassDiff) {
    trace!(
        "Merging {} LAND records from {} into {}",
        plugin.land.len(),
        plugin.plugin.name,
        merged.plugin.name
    );

    for (coords, land) in plugin.iter_land() {
        if merged.land.contains_key(coords) {
            let merged_land = merged.land.get(coords).expect("safe");
            merged.land.insert(
                *coords,
                merge_landscape_diff(&plugin.plugin, merged_land, land),
            );
        } else {
            let mut merged_land = land.clone();
            merged_land
                .plugins
                .push((plugin.plugin.clone(), land.modified_data()));
            merged.land.insert(*coords, merged_land);
        }

        merged.possible_seams.insert(*coords);
    }
}

fn create_tes3_landmass(
    plugin_name: &str,
    parsed_plugins: impl Iterator<Item = &Arc<ParsedPlugin>>,
    known_textures: &mut KnownTextures,
) -> Landmass {
    let plugin = Arc::new(ParsedPlugin::new(plugin_name));
    let master_landmasses = parsed_plugins.flat_map(|esm| try_create_landmass(esm, known_textures));
    merge_tes3_landmasses(&plugin, master_landmasses)
}

fn create_merged_lands_from_reference(reference_landmass: Arc<Landmass>) -> LandmassDiff {
    let mut landmass_diff = LandmassDiff::new(reference_landmass.plugin.clone());

    for (coords, land) in reference_landmass.land.iter() {
        let allowed_data = landscape_flags(land).into();
        let plugin = reference_landmass.plugins.get(coords).expect("safe");
        let landscape_diff = LandscapeDiff::from_reference(plugin.clone(), land, allowed_data);
        assert!(!landscape_diff.is_modified());
        landmass_diff.land.insert(*coords, landscape_diff);
        landmass_diff.possible_seams.insert(*coords);
    }

    repair_landmass_seams(&mut landmass_diff);

    for (_, land) in landmass_diff.land.iter_mut() {
        assert_eq!(land.plugins.len(), 1);
        let modified_data = land.modified_data();
        let plugin_data = land.plugins.get_mut(0).expect("safe");
        plugin_data.1 = modified_data;
    }

    landmass_diff
}
