#![feature(slice_flatten)]
#![feature(let_else)]
#![feature(default_free_fn)]
#![feature(try_blocks)]
#![feature(anonymous_lifetime_in_impl_trait)]
#![feature(map_many_mut)]
#![feature(const_for)]

use crate::io::meta_schema::MetaType;
use crate::io::parsed_plugins::{ParsedPlugin, ParsedPlugins};
use crate::io::save_to_image::save_landmass_images;
use crate::io::save_to_plugin::{convert_landmass_diff_to_landmass, save_plugin};
use crate::land::conversions::{coordinates, landscape_flags};
use crate::land::landscape_diff::LandscapeDiff;
use crate::land::terrain_map::{LandData, Vec2};
use crate::land::textures::{IndexVTEX, KnownTextures, RemappedTextures};
use crate::merge::cells::merge_cells;
use crate::merge::merge_strategy::apply_merge_strategy;
use crate::merge::relative_terrain_map::{IsModified, RelativeTerrainMap};
use crate::repair::cleaning::{clean_known_textures, clean_landmass_diff};
use crate::repair::debugging::add_debug_vertex_colors_to_landmass;
use crate::repair::seam_detection::repair_landmass_seams;
use anyhow::{anyhow, Context, Result};
use hashbrown::HashMap;
use itertools::Itertools;
use log::{debug, error, info, trace, warn};
use owo_colors::OwoColorize;
use simplelog::{
    ColorChoice, CombinedLogger, ConfigBuilder, LevelFilter, LevelPadding, TermLogger,
    TerminalMode, WriteLogger,
};
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::process::exit;
use std::sync::Arc;
use std::time::Instant;
use tes3::esp::{Landscape, LandscapeFlags, LandscapeTexture, ObjectFlags};

mod io;
mod land;
mod merge;
mod repair;

/// A [Landmass] represents a collection of [Landscape] and the associated [ParsedPlugin].
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

    fn insert_land(&mut self, coords: Vec2<i32>, plugin: &Arc<ParsedPlugin>, land: &Landscape) {
        self.plugins.insert(coords, plugin.clone());
        self.land.insert(coords, land.clone());
    }

    /// Returns an [Iterator] over the [Landscape] ordered by `x` and `y` coordinates.
    fn sorted(&self) -> impl Iterator<Item = (&Vec2<i32>, &Landscape)> {
        self.land.iter().sorted_by_key(|f| (f.0.x, f.0.y))
    }
}

/// A [LandmassDiff] represents a collection of [LandscapeDiff] and the associated [ParsedPlugin].
pub struct LandmassDiff {
    plugin: Arc<ParsedPlugin>,
    land: HashMap<Vec2<i32>, LandscapeDiff>,
}

impl LandmassDiff {
    fn new(plugin: Arc<ParsedPlugin>) -> Self {
        Self {
            plugin,
            land: HashMap::new(),
        }
    }

    /// Returns an [Iterator] over the [LandscapeDiff] ordered by `x` and `y` coordinates.
    fn sorted(&self) -> impl Iterator<Item = (&Vec2<i32>, &LandscapeDiff)> {
        self.land.iter().sorted_by_key(|f| (f.0.x, f.0.y))
    }
}

mod cli {
    use crate::ParsedPlugins;
    use anyhow::{anyhow, Context, Result};
    use clap::{AppSettings, ArgEnum, Parser};
    use log::LevelFilter;
    use shadow_rs::shadow;
    use std::path::PathBuf;

    shadow!(build);

    #[derive(Copy, PartialEq, Eq, Debug, Hash, Clone, ArgEnum)]
    pub enum CliLevelFilter {
        Off,
        Error,
        Warn,
        Info,
        Debug,
        Trace,
    }

    #[derive(Copy, PartialEq, Eq, Debug, Hash, Clone, ArgEnum)]
    pub enum SortOrder {
        Default,
        None,
    }

    impl From<CliLevelFilter> for LevelFilter {
        fn from(v: CliLevelFilter) -> Self {
            match v {
                CliLevelFilter::Off => LevelFilter::Off,
                CliLevelFilter::Error => LevelFilter::Error,
                CliLevelFilter::Warn => LevelFilter::Warn,
                CliLevelFilter::Info => LevelFilter::Info,
                CliLevelFilter::Debug => LevelFilter::Debug,
                CliLevelFilter::Trace => LevelFilter::Trace,
            }
        }
    }

    #[derive(Parser, Debug)]
    #[clap(author = "DVD")]
    #[clap(about = "Merges lands.")]
    #[clap(version = build::CLAP_LONG_VERSION)]
    #[clap(long_about = None)] // Read from `Cargo.toml`
    #[clap(global_setting(AppSettings::DeriveDisplayOrder))]
    pub struct Cli {
        #[clap(long, value_parser, default_value_t = String::from("."))]
        /// The directory containing the `Conflicts` folder.
        /// This is also where the `log_file` will be stored.
        merged_lands_dir: String,

        #[clap(long, value_parser, default_value_t = String::from("Data Files"))]
        /// The absolute or relative path to the `Data Files` folder containing plugins.
        data_files_dir: String,

        #[clap(long, value_parser, default_value_t = String::from("Merged Lands.esp"))]
        /// The name of the output file. This will be written to `output_file_dir`.
        pub output_file: String,

        #[clap(long, value_parser)]
        /// The directory for the `output_file`.
        /// If not provided, this is the same as `data_files_dir`.
        output_file_dir: Option<String>,

        #[clap(value_parser, required = false)]
        /// An ordered list of plugins.
        /// If this is not provided, the tool will look for an `.ini` file
        /// in the directory above the `Data Files` and parse that for plugins.
        input_file_names: Vec<String>,

        #[clap(long, arg_enum, value_parser, default_value_t = SortOrder::Default)]
        /// The method of sorting plugins.
        /// `none` is only valid if `input_file_names` are provided.
        pub sort_order: SortOrder,

        #[clap(long, value_parser, default_value_t = String::from("merged_lands.log"))]
        /// The name of the log file. This will be written to `merged_lands_dir`.
        pub log_file: String,

        #[clap(long, arg_enum, value_parser, default_value_t = CliLevelFilter::Debug)]
        /// The level of logging.
        /// If set to Off, no log will will be written.
        pub log_level: CliLevelFilter,

        #[clap(long, value_parser, default_value_t = 8)]
        /// The size of the application's stack in MB.
        stack_size_mb: u8,

        #[clap(long, value_parser)]
        /// The application will remove all CELL records when this flag is provided.
        pub remove_cell_records: bool,

        #[clap(long, value_parser)]
        /// The application will color the LAND vertex colors to show conflicts.
        pub add_debug_vertex_colors: bool,

        #[clap(long, value_parser)]
        /// The application will wait for the user to hit the ENTER key before closing.
        pub wait_for_exit: bool,
    }

    impl Cli {
        pub fn read_args() -> Cli {
            let args = wild::args();
            Cli::parse_from(args)
        }

        pub fn plugins(&self) -> Option<&[String]> {
            (!self.input_file_names.is_empty()).then_some(&self.input_file_names)
        }

        pub fn should_write_log_file(&self) -> bool {
            self.log_level != CliLevelFilter::Off
        }

        pub fn merged_lands_dir(&self) -> Result<PathBuf> {
            let dir = &self.merged_lands_dir;
            Ok(PathBuf::from(dir))
        }

        pub fn data_files_dir(&self) -> Result<PathBuf> {
            let dir = &self.data_files_dir;
            ParsedPlugins::check_dir_exists(dir)
                .with_context(|| anyhow!("Invalid `Data Files` directory"))?;
            Ok(PathBuf::from(dir))
        }

        pub fn output_file_dir(&self) -> Result<PathBuf> {
            let dir = self
                .output_file_dir
                .as_ref()
                .unwrap_or(&self.data_files_dir);
            ParsedPlugins::check_dir_exists(dir)
                .with_context(|| anyhow!("Invalid output file directory"))?;
            Ok(PathBuf::from(dir))
        }

        pub fn stack_size(&self) -> usize {
            (self.stack_size_mb as usize) * 1024 * 1024
        }
    }
}

use cli::Cli;

/// Handles CLI arguments, log initialization, and the creation of a worker thread
/// for running the actual [merge_all] function.
fn main() -> Result<()> {
    let cli = Cli::read_args();
    let wait_for_exit = cli.wait_for_exit;

    init_log(&cli);

    let work_thread = std::thread::Builder::new()
        .stack_size(cli.stack_size())
        .spawn(move || merge_all(&cli))
        .expect("unable to create worker thread");

    if let Err(e) = work_thread.join().expect("unable to join worker thread") {
        error!(
            "{}",
            format!("An unexpected error occurred: {:?}", e.bold()).bright_red()
        );

        wait_for_user_exit(wait_for_exit);
        exit(1);
    }

    wait_for_user_exit(wait_for_exit);
    Ok(())
}

fn wait_for_user_exit(wait_for_exit: bool) {
    if !wait_for_exit {
        return;
    }

    println!();
    println!("Press Enter to exit.");
    let mut buf = [0; 1];
    std::io::stdin().read(&mut buf).ok();
}

/// The main function.
fn merge_all(cli: &Cli) -> Result<()> {
    let start = Instant::now();

    let mut known_textures = KnownTextures::new();

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

    let data_files = cli.data_files_dir()?;
    let plugin_names = cli.plugins();
    let parsed_plugins = ParsedPlugins::new(&data_files, plugin_names, cli.sort_order)?;

    let reference_landmass = Arc::new(create_tes3_landmass(
        "ReferenceLandmass.esp",
        parsed_plugins.masters.iter(),
        &mut known_textures,
    ));

    // TODO(dvd): #feature Support "ignored" maps for hiding differences that we don't care about.

    let modded_landmasses = parsed_plugins
        .plugins
        .iter()
        .flat_map(|plugin| {
            if plugin.meta.meta_type == MetaType::MergedLands {
                trace!("Skipping {}", plugin.name);
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
    }

    // We fix seams as a post-processing step because individual mods can introduce
    // tears into the landscape that would be fixed by subsequent mods. (e.g. patches)
    // If we try to fix the seams early, sadness results.
    repair_landmass_seams(&mut merged_lands);

    // STEP 4:
    //  - Produce images of the final merge results.
    info!(":: Summarizing Conflicts ::");

    let merged_lands_dir = cli.merged_lands_dir()?;
    for modded_landmass in modded_landmasses.iter() {
        save_landmass_images(&merged_lands_dir, &merged_lands, modded_landmass);
    }

    let debug_vertex_colors = cli.add_debug_vertex_colors;
    if debug_vertex_colors {
        warn!(":: Adding Debug Colors ::");
        for modded_landmass in modded_landmasses.iter() {
            add_debug_vertex_colors_to_landmass(&mut merged_lands, modded_landmass);
        }
    }

    // STEP 5:
    // - Iterate through cells in MergedLands.esp and drop anything that is unchanged from the
    //   reference landmass created for MergedLands.esp.
    // - Update all LandData flags to match TES3 expectations.
    // - Run a final seam detection and assert that no seams were found.
    // [IMPLEMENTATION NOTE] This is an optimization to make MergedLands.esp friendlier.
    info!(":: Cleaning Land ::");

    clean_landmass_diff(&mut merged_lands, &modded_landmasses);

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

    let cells = merge_cells(&parsed_plugins);

    let output_file_dir = cli.output_file_dir()?;
    let file_name = &cli.output_file;
    let include_cell_records = !cli.remove_cell_records;
    save_plugin(
        &data_files,
        &output_file_dir,
        file_name,
        cli.sort_order,
        &landmass,
        &known_textures,
        include_cell_records.then_some(&cells),
    )?;

    info!(":: Finished ::");
    info!("Time Elapsed: {:?}", Instant::now().duration_since(start));

    Ok(())
}

/// Initializes a [TermLogger] and [WriteLogger]. If the [WriteLogger] cannot be initialized,
/// then the program will continue with only the [TermLogger].
fn init_log(cli: &Cli) -> bool {
    let config = ConfigBuilder::default()
        .set_time_level(LevelFilter::Off)
        .set_thread_level(LevelFilter::Off)
        .set_location_level(LevelFilter::Off)
        .set_target_level(LevelFilter::Off)
        .set_level_padding(LevelPadding::Right)
        .build();

    let get_log_file_path = || {
        let merged_lands_dir = cli.merged_lands_dir();
        let log_file_name = &cli.log_file;
        let log_file_path: Result<PathBuf> = match merged_lands_dir {
            Ok(path) => Ok([path, PathBuf::from(log_file_name)].iter().collect()),
            Err(e) => Err(e),
        };
        log_file_path
    };

    let write_logger = cli.should_write_log_file().then(|| {
        let log_file_path = get_log_file_path()?;
        File::create(&log_file_path)
            .map(|file| WriteLogger::new(cli.log_level.into(), config.clone(), file))
            .with_context(|| {
                anyhow!(
                    "Unable to create log file at {}",
                    log_file_path.to_string_lossy()
                )
            })
    });

    let term_logger = TermLogger::new(
        LevelFilter::Debug,
        config,
        TerminalMode::Mixed,
        ColorChoice::Auto,
    );

    match write_logger {
        Some(Ok(write_logger)) => {
            CombinedLogger::init(vec![term_logger, write_logger]).expect("safe");
            trace!(
                "Log file will be saved to {}",
                get_log_file_path().expect("safe").to_string_lossy()
            );

            true
        }
        Some(Err(e)) => {
            CombinedLogger::init(vec![term_logger]).expect("safe");
            error!(
                "{} {}",
                format!(
                    "Failed to create log file at {}",
                    get_log_file_path()
                        .unwrap_or_else(|_| PathBuf::from(&cli.log_file))
                        .to_string_lossy()
                        .bold()
                )
                .bright_red(),
                format!("due to: {:?}", e.bold()).bright_red()
            );

            false
        }
        None => {
            trace!("No log file will be created.");
            CombinedLogger::init(vec![term_logger]).expect("safe");
            false
        }
    }
}

/// Copy [Landscape] records from `plugin` and remap the texture indices with [RemappedTextures].
fn try_copy_landscape_and_remap_textures(
    plugin: &Arc<ParsedPlugin>,
    remapped_textures: &RemappedTextures,
) -> Option<Landmass> {
    let mut landmass = Landmass::new(plugin.clone());

    if plugin.records.objects_of_type::<Landscape>().any(|_| true) {
        debug!("Creating landmass from {}", plugin.name);
    }

    for land in plugin.records.objects_of_type::<Landscape>() {
        let mut updated_land = land.clone();

        if let Some(texture_indices) = updated_land.texture_indices.as_mut() {
            for idx in texture_indices.data.flatten_mut() {
                *idx = remapped_textures
                    .remapped_index(IndexVTEX::new(*idx))
                    .as_u16();
            }
        }

        let coords = coordinates(land);
        landmass.insert_land(coords, plugin, &updated_land);
    }

    if !landmass.land.is_empty() {
        Some(landmass)
    } else {
        None
    }
}

/// Creates a [Landmass] from the `plugin` and updates [KnownTextures].
fn try_create_landmass(
    plugin: &Arc<ParsedPlugin>,
    known_textures: &mut KnownTextures,
) -> Option<Landmass> {
    if plugin
        .records
        .objects_of_type::<LandscapeTexture>()
        .any(|_| true)
    {
        debug!("Remapping textures from {}", plugin.name);
    }

    let mut remapped_textures = RemappedTextures::new(known_textures);
    for texture in plugin.records.objects_of_type::<LandscapeTexture>() {
        known_textures.add_remapped_texture(plugin, texture, &mut remapped_textures);
    }

    try_copy_landscape_and_remap_textures(plugin, &remapped_textures)
}

/// Returns a "merged" [Landscape] combining `rhs` and `lhs` by stomping over
/// any changes in `lhs` with the records from `rhs`.
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

    if new_data.uses_world_map_data() {
        if let Some(world_map_data) = rhs.world_map_data.as_ref() {
            land.world_map_data = Some(world_map_data.clone());
        }
    }

    land.landscape_flags = old_data;

    land
}

/// Creates a single [Landmass] by calling [merge_tes3_landscape] on all `landmasses`.
fn merge_tes3_landmasses(
    plugin: &Arc<ParsedPlugin>,
    landmasses: impl Iterator<Item = Landmass>,
) -> Landmass {
    let mut merged_landmass = Landmass::new(plugin.clone());

    for landmass in landmasses {
        for (coords, land) in landmass.land.iter() {
            let merged_land = if merged_landmass.land.contains_key(coords) {
                merge_tes3_landscape(merged_landmass.land.get(coords).expect("safe"), land)
            } else {
                land.clone()
            };

            merged_landmass.insert_land(*coords, &landmass.plugin, &merged_land);
        }
    }

    merged_landmass
}

/// Given a [ParsedPlugin] and a specific [Landscape], returns [LandData] representing
/// what should be used when creating or merging a [LandscapeDiff].
fn find_allowed_data(plugin: &ParsedPlugin, land: &Landscape) -> LandData {
    let mut allowed_data: LandData = landscape_flags(land).into();

    if !plugin.meta.height_map.included {
        allowed_data.remove(LandData::VERTEX_HEIGHTS | LandData::VERTEX_NORMALS);
    }

    if !plugin.meta.vertex_colors.included {
        allowed_data.remove(LandData::VERTEX_COLORS);
    }

    if !plugin.meta.texture_indices.included {
        allowed_data.remove(LandData::TEXTURES);
    }

    if !plugin.meta.world_map_data.included {
        allowed_data.remove(LandData::WORLD_MAP);
    }

    allowed_data
}

/// Creates a [LandmassDiff] representing the set of [LandscapeDiff] between the
/// `landmass` and `reference` [Landmass].
fn find_landmass_diff(landmass: &Landmass, reference: Arc<Landmass>) -> LandmassDiff {
    let mut landmass_diff = LandmassDiff::new(landmass.plugin.clone());

    for (coords, land) in landmass.land.iter() {
        let reference_land = reference.land.get(coords);
        let allowed_data = find_allowed_data(&landmass.plugin, land);
        let landscape_diff = LandscapeDiff::from_difference(land, reference_land, allowed_data);
        landmass_diff.land.insert(*coords, landscape_diff);
    }

    landmass_diff
}

/// Merges `old` and `new` [LandscapeDiff].
fn merge_landscape_diff(
    plugin: &Arc<ParsedPlugin>,
    old: &LandscapeDiff,
    new: &LandscapeDiff,
) -> LandscapeDiff {
    let mut merged = old.clone();
    merged.plugins.push((plugin.clone(), new.modified_data()));

    let coords = merged.coords;

    merged.height_map = apply_merge_strategy(
        coords,
        plugin,
        "height_map",
        old.height_map.as_ref(),
        new.height_map.as_ref(),
        plugin.meta.height_map.conflict_strategy,
    );

    merged.vertex_normals = apply_merge_strategy(
        coords,
        plugin,
        "vertex_normals",
        old.vertex_normals.as_ref(),
        new.vertex_normals.as_ref(),
        plugin.meta.height_map.conflict_strategy,
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
        plugin.meta.world_map_data.conflict_strategy,
    );

    merged.vertex_colors = apply_merge_strategy(
        coords,
        plugin,
        "vertex_colors",
        old.vertex_colors.as_ref(),
        new.vertex_colors.as_ref(),
        plugin.meta.vertex_colors.conflict_strategy,
    );

    merged.texture_indices = apply_merge_strategy(
        coords,
        plugin,
        "texture_indices",
        old.texture_indices.as_ref(),
        new.texture_indices.as_ref(),
        plugin.meta.texture_indices.conflict_strategy,
    );

    merged
}

/// Merges `plugin` [LandmassDiff] into `merged` [LandmassDiff].
fn merge_landmass_into(merged: &mut LandmassDiff, plugin: &LandmassDiff) {
    debug!(
        "Merging {} LAND records from {} into {}",
        plugin.land.len(),
        plugin.plugin.name,
        merged.plugin.name
    );

    for (coords, land) in plugin.sorted() {
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
    }
}

/// Creates a [Landmass] from `parsed_plugins` and updates [KnownTextures].
fn create_tes3_landmass(
    plugin_name: &str,
    parsed_plugins: impl Iterator<Item = &Arc<ParsedPlugin>>,
    known_textures: &mut KnownTextures,
) -> Landmass {
    let plugin = Arc::new(ParsedPlugin::empty(plugin_name));
    let master_landmasses = parsed_plugins.flat_map(|esm| try_create_landmass(esm, known_textures));
    merge_tes3_landmasses(&plugin, master_landmasses)
}

/// Creates a [LandmassDiff] representing a set of empty [LandscapeDiff] for the `reference` [Landmass].
/// Prior to returning, the [LandmassDiff] will be updated by [repair_landmass_seams].
fn create_merged_lands_from_reference(reference: Arc<Landmass>) -> LandmassDiff {
    let mut landmass_diff = LandmassDiff::new(reference.plugin.clone());

    for (coords, land) in reference.land.iter() {
        let allowed_data = landscape_flags(land).into();
        let plugin = reference.plugins.get(coords).expect("safe");
        let landscape_diff = LandscapeDiff::from_reference(plugin.clone(), land, allowed_data);
        assert!(!landscape_diff.is_modified());
        landmass_diff.land.insert(*coords, landscape_diff);
    }

    for (_, land) in landmass_diff.land.iter_mut() {
        assert_eq!(land.plugins.len(), 1);
        let modified_data = land.modified_data();
        let plugin_data = land.plugins.get_mut(0).expect("safe");
        plugin_data.1 = modified_data;
    }

    landmass_diff
}
