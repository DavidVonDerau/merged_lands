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

use crate::io::active_plugin_paths::ActivePluginPaths;
use crate::io::read_plugin::parse_records;
use crate::io::save_to_image::save_landmass_images;
use crate::io::save_to_plugin::{convert_landmass_diff_to_landmass, save_plugin};
use crate::land::conversions::{coordinates, landscape_flags};
use crate::land::landscape_diff::LandscapeDiff;
use crate::land::terrain_map::{LandData, Vec2};
use crate::merge::merge_strategy::apply_merge_strategy;
use crate::merge::overwrite_strategy::OverwriteStrategy;
use crate::merge::relative_terrain_map::{IsModified, RelativeTerrainMap};
use crate::merge::resolve_conflict_strategy::ResolveConflictStrategy;
use crate::repair::cleaning::{clean_known_textures, clean_landmass_diff};
use crate::repair::seam_detection::repair_landmass_seams;

use crate::land::textures::{KnownTextures, RemappedTextures};
use itertools::Itertools;
use log::{debug, error, info, trace};
use mimalloc::MiMalloc;
use simplelog::{
    ColorChoice, CombinedLogger, ConfigBuilder, LevelFilter, LevelPadding, TermLogger,
    TerminalMode, WriteLogger,
};
use std::collections::{HashMap, HashSet};
use std::default::default;
use std::fs::File;
use std::io::Read;
use std::str;
use std::sync::Arc;
use std::time::Instant;
use tes3::esp::{Landscape, LandscapeFlags, LandscapeTexture, Plugin};

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

pub struct Landmass {
    plugin: String,
    data: Arc<Plugin>,
    land: HashMap<Vec2<i32>, Landscape>,
    plugins: HashMap<Vec2<i32>, String>,
}

impl Landmass {
    fn new(plugin: String, data: Arc<Plugin>) -> Self {
        Self {
            plugin,
            data,
            land: HashMap::new(),
            plugins: HashMap::new(),
        }
    }
}

pub struct LandmassDiff {
    plugin: String,
    data: Arc<Plugin>,
    land: HashMap<Vec2<i32>, LandscapeDiff>,
    possible_seams: HashSet<Vec2<i32>>,
}

impl LandmassDiff {
    fn new(plugin: String, data: Arc<Plugin>) -> Self {
        Self {
            plugin,
            data,
            land: HashMap::new(),
            possible_seams: HashSet::new(),
        }
    }

    fn iter_land(&self) -> impl Iterator<Item = (&Vec2<i32>, &LandscapeDiff)> {
        self.land.iter().sorted_by_key(|f| (f.0.x, f.0.y))
    }
}

fn main() {
    init_log();

    const STACK_SIZE: usize = 8 * 1024 * 1024;

    let work_thread = std::thread::Builder::new()
        .stack_size(STACK_SIZE)
        .spawn(merge_all)
        .expect("unable to create worker thread");

    work_thread.join().expect("unable to join worker thread");

    // TODO(dvd): Make this configurable.
    let wait = false;
    if wait {
        wait_for_user_exit();
    }
}

fn merge_all() {
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

    let active_plugin_paths = ActivePluginPaths::new("./Morrowind.ini");

    // TODO(dvd): Inspect Sadrith Mora dock expansion vs BCoM.
    // TODO(dvd): Inspect BCoM Balmora Docks patch vs BCoM.

    // TODO(dvd): Handle deletions correctly.
    // TODO(dvd): Merge object flags correctly.

    // TODO(dvd): Parse all masters + plugins first, so that they don't need to be reparsed later.

    let reference_landmass = Arc::new(create_tes3_landmass(
        "ReferenceLandmass.esp",
        active_plugin_paths.masters.iter(),
        &mut known_textures,
    ));

    // TODO(dvd): This order seems incorrect in the actual Morrowind directory -- WTF?
    let modded_landmasses = active_plugin_paths
        .plugins
        .iter()
        .flat_map(|path| {
            if path == "Merged Lands.esp" {
                // TODO(dvd): Print error if a plugin of type merged_lands is found, but it has a different name.
                // TODO(dvd): Replace this hack with the meta file.
                return None;
            }

            // TODO(dvd): Support additional masters, e.g. for patches.
            try_create_landmass(path, &mut known_textures)
                .map(|landmass| find_landmass_diff(&landmass, reference_landmass.clone()))
        })
        .collect_vec();

    debug!(
        "Found {} masters and {} plugins.",
        active_plugin_paths.masters.len(),
        active_plugin_paths.plugins.len(),
    );
    debug!("Found {} unique LTEX records.", known_textures.len());
    debug!("{} plugins contain LAND records.", modded_landmasses.len());

    // STEP 2:
    // Create the MergedLands.esp:
    //  - Calculate the "naive" TES3 merge of the ordered ESMs.
    info!(":: Creating Reference Land ::");

    debug!(
        "Reference contains {} LAND records.",
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
        // TODO(dvd): Maybe don't merge a mod here if it has specific instructions, e.g. overwrite.
        merge_landmass_into(&mut merged_lands, modded_landmass);
        repair_landmass_seams(&mut merged_lands);
    }

    // TODO(dvd): Go through mods with specific patch instructions and apply them here?

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
        clean_known_textures(&active_plugin_paths, &merged_lands, &mut known_textures);

    // STEP 7:
    // Convert "height map" representation of LAND records to "xy delta + offset" representation.
    // Remap texture indices.
    info!(":: Converting to LAND Records ::");

    let landmass = convert_landmass_diff_to_landmass(&merged_lands, &remapped_textures);

    // STEP 7:
    // Save to an ESP.
    //  - [IMPLEMENTATION NOTE] Reuse last modified date if the ESP already exists.
    info!(":: Saving ::");

    save_plugin("Merged Lands", &landmass, &known_textures);

    debug!("Saved {} LTEX records.", known_textures.len());
    debug!("Saved {} LAND records.", landmass.land.len());

    info!(":: Finished ::");
    info!("Time Elapsed: {:?}", Instant::now().duration_since(start))
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
                "Failed to create log file at {} due to: {}",
                log_file_name, e
            );
        }
    }
}

fn wait_for_user_exit() {
    println!();
    println!("Press Enter to exit.");
    let mut buf = [0; 1];
    std::io::stdin().read(&mut buf).ok();
}

fn try_copy_landscape_and_remap_textures(
    plugin: &str,
    data: Arc<Plugin>,
    remapped_textures: &RemappedTextures,
) -> Option<Landmass> {
    let mut landmass = Landmass::new(plugin.to_string(), data);

    for land in landmass.data.objects_of_type::<Landscape>() {
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

fn try_create_landmass(plugin: &str, known_textures: &mut KnownTextures) -> Option<Landmass> {
    let data = parse_records(plugin).ok()?;

    let mut remapped_textures = RemappedTextures::new(known_textures);
    for texture in data.objects_of_type::<LandscapeTexture>() {
        known_textures.add_remapped_texture(texture, &mut remapped_textures);
    }

    try_copy_landscape_and_remap_textures(plugin, data, &remapped_textures)
}

fn merge_tes3_landscape(lhs: &Landscape, rhs: &Landscape) -> Landscape {
    let mut land = lhs.clone();

    let mut old_data = landscape_flags(lhs);
    let new_data = landscape_flags(rhs);

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

fn merge_tes3_landmasses(name: &str, landmasses: impl Iterator<Item = Landmass>) -> Landmass {
    let mut merged_landmass = Landmass::new(name.to_string(), Arc::new(Plugin::new()));

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
    let mut landmass_diff = LandmassDiff::new(landmass.plugin.clone(), landmass.data.clone());

    let is_cantons = landmass.plugin == "Cantons_on_the_Global_Map_v1.1.esp";

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

fn merge_landscape_diff(plugin: &str, old: &LandscapeDiff, new: &LandscapeDiff) -> LandscapeDiff {
    let mut merged = old.clone();
    merged
        .plugins
        .push((plugin.to_string(), new.modified_data()));

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
    // TODO(dvd): Track plugin names on individual LAND records.
    trace!(
        "Merging {} LAND records from {} into {}",
        plugin.land.len(),
        plugin.plugin,
        merged.plugin
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
    name: &str,
    active_plugin_paths: impl Iterator<Item = &String>,
    known_textures: &mut KnownTextures,
) -> Landmass {
    let master_landmasses =
        active_plugin_paths.flat_map(|esm| try_create_landmass(esm, known_textures));

    // TODO(dvd): This can fail to detect missing plugins, because `parse_landmass` doesn't differentiate between file failures and empty lands.
    merge_tes3_landmasses(name, master_landmasses)
}

fn create_merged_lands_from_reference(reference_landmass: Arc<Landmass>) -> LandmassDiff {
    let mut landmass_diff = LandmassDiff::new(
        reference_landmass.plugin.clone(),
        reference_landmass.data.clone(),
    );

    for (coords, land) in reference_landmass.land.iter() {
        let allowed_data = landscape_flags(land).into();
        let plugin = reference_landmass.plugins.get(coords).expect("safe");
        let landscape_diff = LandscapeDiff::from_reference(plugin, land, allowed_data);
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
