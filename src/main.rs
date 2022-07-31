#![feature(slice_flatten)]
#![feature(let_else)]
#![feature(default_free_fn)]
#![feature(try_blocks)]
#![feature(anonymous_lifetime_in_impl_trait)]

mod active_plugin_paths;
mod conflict;
mod grid_access;
mod height_map;
mod merge_strategy;
mod overwrite_strategy;
mod parser;
mod relative_to;
mod resolve_conflict_strategy;
mod round_to;
mod save_to_image;
mod terrain;

use crate::active_plugin_paths::ActivePluginPaths;
use crate::conflict::{ConflictResolver, ConflictType};
use crate::grid_access::{GridAccessor2D, GridPosition2D, SquareGridIterator};
use crate::height_map::{
    calculate_vertex_heights_tes3, try_calculate_height_map, HEIGHT_MAP_SCALE_FACTOR_F32,
};
use crate::merge_strategy::{apply_merge_strategy, MergeStrategy};
use crate::overwrite_strategy::OverwriteStrategy;
use crate::parser::*;
use crate::relative_to::RelativeTo;
use crate::resolve_conflict_strategy::ResolveConflictStrategy;
use crate::round_to::RoundTo;
use crate::save_to_image::SaveToImage;
use crate::terrain::*;
use filetime::FileTime;
use itertools::Itertools;
use mimalloc::MiMalloc;
use num_traits::Pow;
use std::collections::HashMap;
use std::default::default;
use std::path::PathBuf;
use std::str;
use std::sync::Arc;
use tes3::esp::{
    FixedString, Header, Landscape, LandscapeFlags, LandscapeTexture, ObjectFlags, Plugin,
    TES3Object, TextureIndices, VertexColors, VertexNormals, WorldMapData,
};

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[derive(Copy, Clone)]
pub struct RelativeTerrainMap<U: RelativeTo, const T: usize> {
    reference: TerrainMap<U, T>,
    relative: TerrainMap<<U as RelativeTo>::Delta, T>,
    has_difference: TerrainMap<bool, T>,
}

pub trait IsModified {
    fn is_modified(&self) -> bool;

    fn num_differences(&self) -> usize;
}

impl<U: RelativeTo, const T: usize> IsModified for RelativeTerrainMap<U, T> {
    fn is_modified(&self) -> bool {
        self.has_difference.flatten().contains(&true)
    }

    fn num_differences(&self) -> usize {
        self.has_difference.flatten().iter().filter(|v| **v).count()
    }
}

pub type OptionalTerrain<U, const T: usize> = Option<RelativeTerrainMap<U, T>>;

impl<U: RelativeTo, const T: usize> IsModified for OptionalTerrain<U, T> {
    fn is_modified(&self) -> bool {
        self.map(|map| map.is_modified()).unwrap_or(false)
    }

    fn num_differences(&self) -> usize {
        self.map(|map| map.num_differences()).unwrap_or(0)
    }
}

fn calculate_relative_terrain_map<U: RelativeTo, const T: usize>(
    reference: &TerrainMap<U, T>,
    plugin: &TerrainMap<U, T>,
) -> RelativeTerrainMap<U, T> {
    let mut has_difference = [[false; T]; T];
    let mut relative = [[default(); T]; T];

    for coords in reference.iter_grid() {
        let diff = U::subtract(plugin.get(coords), reference.get(coords));
        *relative.get_mut(coords) = diff;
        *has_difference.get_mut(coords) = diff != default();
    }

    RelativeTerrainMap {
        reference: *reference,
        relative,
        has_difference,
    }
}

fn calculate_terrain_map<U: RelativeTo, const T: usize>(
    diff: &RelativeTerrainMap<U, T>,
) -> TerrainMap<U, T> {
    let mut terrain = [[default(); T]; T];

    for coords in diff.reference.iter_grid() {
        let diff = U::add(diff.reference.get(coords), diff.relative.get(coords));
        *terrain.get_mut(coords) = diff;
    }

    terrain
}

#[derive(Copy, Clone)]
struct LandscapeDiff {
    coordinates: Vec2<i32>,
    flags: ObjectFlags,
    height_map: OptionalTerrain<i32, 65>,
    vertex_normals: OptionalTerrain<Vec3<i8>, 65>,
    world_map_data: OptionalTerrain<u8, 9>,
    vertex_colors: OptionalTerrain<Vec3<u8>, 65>,
    texture_indices: OptionalTerrain<u16, 16>,
}

impl LandscapeDiff {
    pub fn is_modified(&self) -> bool {
        self.height_map.is_modified()
            || self.vertex_normals.is_modified()
            || self.world_map_data.is_modified()
            || self.vertex_colors.is_modified()
            || self.texture_indices.is_modified()
    }
}

fn apply_mask<U: RelativeTo, const T: usize>(
    old: &RelativeTerrainMap<U, T>,
    allow: Option<&TerrainMap<bool, T>>,
) -> RelativeTerrainMap<U, T> {
    let mut new = *old;

    if let Some(allowed) = allow {
        for coords in old
            .relative
            .iter_grid()
            .filter(|coords| !allowed.get(*coords))
        {
            *new.has_difference.get_mut(coords) = false;
            *new.relative.get_mut(coords) = default();
        }
    } else {
        for v in new.has_difference.flatten_mut() {
            *v = false;
        }

        for v in new.relative.flatten_mut() {
            *v = default();
        }
    }

    new
}

fn calculate_differences_with_mask<U: RelativeTo, const T: usize>(
    value: &str,
    should_include: bool,
    reference: Option<TerrainMap<U, T>>,
    plugin: Option<TerrainMap<U, T>>,
    use_mask: bool,
    allow: Option<&TerrainMap<bool, T>>,
) -> OptionalTerrain<U, T> {
    if !should_include {
        println!("{}: skipped", value);
        return None;
    }

    if plugin.is_none() {
        println!("{}: missing", value);
        return None;
    }

    let compare_with = reference.unwrap_or_else(|| [[default(); T]; T]);
    let relative = calculate_relative_terrain_map(&compare_with, plugin.as_ref().unwrap());
    if !relative.is_modified() {
        println!("{}: same", value);
        return None;
    }

    let updated = if use_mask {
        let masked = apply_mask(&relative, allow);
        masked.is_modified().then_some(masked)
    } else {
        Some(relative)
    };

    let num_differences = updated.map(|t| t.num_differences()).unwrap_or(0);

    if updated.is_some() {
        if reference.is_some() {
            println!(
                "{}: **DIFFERENT FROM REFERENCE** {}",
                value, num_differences
            );
        } else {
            println!("{}: **DIFFERENT FROM DEFAULT** {}", value, num_differences);
        }
    } else {
        println!("{}: ignored", value);
    }

    updated
}

fn calculate_differences<U: RelativeTo, const T: usize>(
    value: &str,
    should_include: bool,
    reference: Option<TerrainMap<U, T>>,
    plugin: Option<TerrainMap<U, T>>,
) -> OptionalTerrain<U, T> {
    calculate_differences_with_mask(value, should_include, reference, plugin, false, None)
}

fn calculate_reference<U: RelativeTo, const T: usize>(
    should_include: bool,
    plugin: Option<TerrainMap<U, T>>,
) -> OptionalTerrain<U, T> {
    if !should_include {
        return None;
    }

    plugin?;

    Some(calculate_relative_terrain_map(
        plugin.as_ref().unwrap(),
        plugin.as_ref().unwrap(),
    ))
}

fn fix_coords<const T: usize>(coords: GridPosition2D) -> GridPosition2D {
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

    GridPosition2D { x, y }
}

fn calculate_vertex_normals_map<const T: usize>(
    height_map: &TerrainMap<i32, T>,
) -> TerrainMap<Vec3<i8>, T> {
    let mut terrain = [[default(); T]; T];

    for coords in height_map.iter_grid() {
        let fixed_coords = fix_coords::<T>(coords);

        let coords_x1 = GridPosition2D {
            x: fixed_coords.x + 1,
            y: fixed_coords.y,
        };

        let h = height_map.get(fixed_coords) as f32 / HEIGHT_MAP_SCALE_FACTOR_F32;
        let x1 = height_map.get(coords_x1) as f32 / HEIGHT_MAP_SCALE_FACTOR_F32;
        let v1 = Vec3 {
            x: 128f32 / HEIGHT_MAP_SCALE_FACTOR_F32,
            y: 0f32,
            z: (x1 - h) as f32,
        };

        let coords_y1 = GridPosition2D {
            x: fixed_coords.x,
            y: fixed_coords.y + 1,
        };
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

        *terrain.get_mut(coords) = Vec3 {
            x: normal.x as i8,
            y: normal.y as i8,
            z: normal.z as i8,
        };
    }

    terrain
}

fn recompute_normals(land: &LandscapeDiff) -> TerrainMap<Vec3<i8>, 65> {
    let height_map = land.height_map.as_ref().unwrap();
    let vertex_normals = land.vertex_normals.as_ref().unwrap();

    let height_map_abs = calculate_terrain_map(height_map);

    let mut recomputed_vertex_normals = calculate_vertex_normals_map(&height_map_abs);
    for coords in vertex_normals.reference.iter_grid() {
        if !vertex_normals.has_difference.get(coords) {
            assert_eq!(vertex_normals.relative.get(coords), default());
            *recomputed_vertex_normals.get_mut(coords) = vertex_normals.reference.get(coords);
        }
    }

    recomputed_vertex_normals
}

impl LandscapeDiff {
    fn from_reference(land: &Landscape, allowed_data: LandData) -> Self {
        let included_data = landscape_flags(land);

        let height_map = calculate_reference(
            included_data.contains(LandscapeFlags::USES_VERTEX_HEIGHTS_AND_NORMALS)
                && allowed_data.contains(LandData::VHGT),
            try_calculate_height_map(land),
        );

        let vertex_normals = calculate_reference(
            included_data.contains(LandscapeFlags::USES_VERTEX_HEIGHTS_AND_NORMALS)
                && allowed_data.contains(LandData::VNML),
            vertex_normals(land),
        );

        let world_map_data = calculate_reference(
            included_data.intersects(LandscapeFlags::USES_WORLD_MAP_DATA)
                && allowed_data.contains(LandData::WNAM),
            world_map_data(land),
        );

        let vertex_colors = calculate_reference(
            included_data.contains(LandscapeFlags::USES_VERTEX_COLORS)
                && allowed_data.contains(LandData::VCLR),
            vertex_colors(land),
        );

        let texture_indices = calculate_reference(
            included_data.contains(LandscapeFlags::USES_TEXTURES)
                && allowed_data.contains(LandData::VTEX),
            texture_indices(land),
        );

        Self {
            coordinates: coordinates(land),
            flags: land.flags,
            height_map,
            vertex_normals,
            world_map_data,
            vertex_colors,
            texture_indices,
        }
    }

    fn from_difference(
        land: &Landscape,
        reference: Option<&Landscape>,
        allowed_data: LandData,
    ) -> Self {
        let included_data = landscape_flags(land);

        let height_map = calculate_differences(
            "height_map",
            included_data.contains(LandscapeFlags::USES_VERTEX_HEIGHTS_AND_NORMALS)
                && allowed_data.contains(LandData::VHGT),
            reference.and_then(try_calculate_height_map),
            try_calculate_height_map(land),
        );

        let vertex_normals = calculate_differences_with_mask(
            "vertex_normals",
            included_data.contains(LandscapeFlags::USES_VERTEX_HEIGHTS_AND_NORMALS)
                && allowed_data.contains(LandData::VNML),
            reference.and_then(vertex_normals),
            vertex_normals(land),
            true,
            height_map.map(|h| h.has_difference).as_ref(),
        );

        let world_map_data = calculate_differences(
            "world_map_data",
            included_data.intersects(LandscapeFlags::USES_WORLD_MAP_DATA)
                && allowed_data.contains(LandData::WNAM),
            reference.and_then(world_map_data),
            world_map_data(land),
        );

        let vertex_colors = calculate_differences(
            "vertex_colors",
            included_data.contains(LandscapeFlags::USES_VERTEX_COLORS)
                && allowed_data.contains(LandData::VCLR),
            reference.and_then(vertex_colors),
            vertex_colors(land),
        );

        let texture_indices = calculate_differences(
            "texture_indices",
            included_data.contains(LandscapeFlags::USES_TEXTURES)
                && allowed_data.contains(LandData::VTEX),
            reference.and_then(texture_indices),
            texture_indices(land),
        );

        Self {
            coordinates: coordinates(land),
            flags: land.flags,
            height_map,
            vertex_normals,
            world_map_data,
            vertex_colors,
            texture_indices,
        }
    }
}

struct Landmass {
    plugin: String,
    data: Arc<Plugin>,
    land: HashMap<Vec2<i32>, Landscape>,
}

impl Landmass {
    fn new(plugin: String, data: Arc<Plugin>) -> Self {
        Self {
            plugin,
            data,
            land: HashMap::new(),
        }
    }
}

struct LandmassDiff {
    plugin: String,
    data: Arc<Plugin>,
    land: HashMap<Vec2<i32>, LandscapeDiff>,
}

impl LandmassDiff {
    fn new(plugin: String, data: Arc<Plugin>) -> Self {
        Self {
            plugin,
            data,
            land: HashMap::new(),
        }
    }
}

// TODO(dvd): These textures should track which plugin they came from.
type KnownTextures = HashMap<String, LandscapeTexture>;
type RemappedTextures = HashMap<u16, u16>;

fn try_copy_landscape_and_remap_textures(
    plugin: &str,
    data: Arc<Plugin>,
    remapped_textures: &RemappedTextures,
) -> Option<Landmass> {
    let mut landmass = Landmass::new(plugin.to_string(), data);

    for land in landmass.data.objects_of_type::<Landscape>() {
        let mut updated_land = land.clone();
        if let Some(texture_indices) = &mut updated_land.texture_indices {
            for idx in texture_indices.data.flatten_mut() {
                let key = *idx;
                if key != 0 {
                    let old_id = key - 1;
                    let new_id = *remapped_textures.get(&old_id).expect("missing remapped ID");
                    *idx = new_id + 1;
                }
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

    println!("{:?}", data.header().unwrap());

    let mut remapped_texture_ids = RemappedTextures::new();
    for texture in data.objects_of_type::<LandscapeTexture>() {
        let old_id = texture
            .index
            .unwrap()
            .try_into()
            .expect("invalid texture ID");

        let new_id = if known_textures.contains_key(&texture.id) {
            let prev_idx: u16 = known_textures
                .get(&texture.id)
                .unwrap()
                .index
                .unwrap()
                .try_into()
                .unwrap();
            println!("Using LTEX {}={}  in {}", texture.id, old_id, plugin);
            prev_idx
        } else {
            let new_idx: u16 = known_textures
                .len()
                .try_into()
                .expect("exceeded 65535 textures");
            let mut new_texture = texture.clone();
            new_texture.index = Some(new_idx.into());
            println!("Adding LTEX {}={} in {}", new_texture.id, new_idx, plugin);
            known_textures.insert(texture.id.clone(), new_texture);
            new_idx
        };

        remapped_texture_ids.insert(old_id, new_id);
    }

    try_copy_landscape_and_remap_textures(plugin, data, &remapped_texture_ids)
}

fn merge_tes3_landscape(lhs: &Landscape, rhs: &Landscape) -> Landscape {
    let mut land = lhs.clone();

    let mut old_data = landscape_flags(lhs);
    let new_data = landscape_flags(rhs);

    if new_data.contains(LandscapeFlags::USES_VERTEX_HEIGHTS_AND_NORMALS) {
        old_data |= LandscapeFlags::USES_VERTEX_HEIGHTS_AND_NORMALS;
        land.vertex_normals = rhs.vertex_normals.clone();
        land.vertex_heights = rhs.vertex_heights.clone();
    }

    if new_data.contains(LandscapeFlags::USES_VERTEX_COLORS) {
        old_data |= LandscapeFlags::USES_VERTEX_COLORS;
        land.vertex_colors = rhs.vertex_colors.clone();
    }

    if new_data.contains(LandscapeFlags::USES_TEXTURES) {
        old_data |= LandscapeFlags::USES_TEXTURES;
        land.texture_indices = rhs.texture_indices.clone();
    }

    if new_data.intersects(LandscapeFlags::USES_WORLD_MAP_DATA) {
        land.world_map_data = rhs.world_map_data.clone();
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
                    merge_tes3_landscape(merged_landmass.land.get(coords).unwrap(), land);
                merged_landmass.land.insert(*coords, merged_land);
            } else {
                merged_landmass.land.insert(*coords, land.clone());
            }
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
            LandData::WNAM
        } else {
            landscape_flags(land).into()
        };

        if reference_land.is_some() {
            println!(
                "Calculating difference at {:?} between {} and {} ({:?})",
                coords, landmass.plugin, reference.plugin, allowed_data
            );
        } else {
            println!(
                "Inserting entry at {:?} from {} ({:?})",
                coords, landmass.plugin, allowed_data
            );
        }

        landmass_diff.land.insert(
            *coords,
            LandscapeDiff::from_difference(land, reference_land, allowed_data),
        );
    }

    landmass_diff
}

fn merge_landscape_diff(plugin: &str, old: &LandscapeDiff, new: &LandscapeDiff) -> LandscapeDiff {
    let mut merged = *old;

    let merge_strategy: ResolveConflictStrategy = default();
    let overwrite_strategy: OverwriteStrategy = default();

    let coords = merged.coordinates;

    merged.height_map = apply_merge_strategy(
        coords,
        plugin,
        "height_map",
        old.height_map,
        new.height_map,
        &merge_strategy,
    );

    merged.vertex_normals = apply_merge_strategy(
        coords,
        plugin,
        "vertex_normals",
        old.vertex_normals,
        new.vertex_normals,
        &merge_strategy,
    );

    if let Some(vertex_normals) = merged.vertex_normals.as_ref() {
        merged.vertex_normals = Some(apply_mask(
            vertex_normals,
            merged.height_map.map(|h| h.has_difference).as_ref(),
        ));
    }

    if merged.vertex_normals.is_modified() {
        assert!(merged.height_map.is_modified());
    }

    merged.world_map_data = apply_merge_strategy(
        coords,
        plugin,
        "world_map_data",
        old.world_map_data,
        new.world_map_data,
        &merge_strategy,
    );
    merged.vertex_colors = apply_merge_strategy(
        coords,
        plugin,
        "vertex_colors",
        old.vertex_colors,
        new.vertex_colors,
        &merge_strategy,
    );

    merged.texture_indices = apply_merge_strategy(
        coords,
        plugin,
        "texture_indices",
        old.texture_indices,
        new.texture_indices,
        &overwrite_strategy,
    );

    merged
}

fn merge_landmass_into(merged: &mut LandmassDiff, plugin: &LandmassDiff) {
    // TODO(dvd): Track plugin names on individual LAND records.
    println!("Merging {} into {}", plugin.plugin, merged.plugin);
    for (coords, land) in plugin.land.iter() {
        if merged.land.contains_key(coords) {
            println!("Merging {} into {:?}", plugin.plugin, coords);
            let merged_land = merged.land.get(coords).unwrap();
            merged.land.insert(
                *coords,
                merge_landscape_diff(&plugin.plugin, merged_land, land),
            );
        } else {
            println!("Adding {} at {:?}", plugin.plugin, coords);
            merged.land.insert(*coords, *land);
        }
    }
}

fn create_tes3_landmass(
    name: &str,
    active_plugin_paths: impl Iterator<Item = &PathBuf>,
    known_textures: &mut KnownTextures,
) -> Landmass {
    let master_landmasses = active_plugin_paths
        .flat_map(|path| path.to_str())
        .flat_map(|esm| try_create_landmass(esm, known_textures));

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
        let landscape_diff = LandscapeDiff::from_reference(land, allowed_data);
        assert!(!landscape_diff.is_modified());
        landmass_diff.land.insert(*coords, landscape_diff);
    }

    landmass_diff
}

fn update_known_textures(path: &str, known_textures: &mut KnownTextures) {
    if let Ok(records) = parse_records(path) {
        for texture in records
            .objects_of_type::<LandscapeTexture>()
            .filter(|texture| texture.texture.is_some())
        {
            let known_texture = known_textures.get_mut(&texture.id).unwrap();
            known_texture.texture = texture.texture.clone();
        }
    }
}

fn clean_landmass_diff(landmass: &mut LandmassDiff) {
    let mut unmodified = Vec::new();
    for (coords, land) in landmass.land.iter_mut() {
        if !land.is_modified() {
            unmodified.push(*coords);
        }
    }

    for coords in unmodified {
        landmass.land.remove(&coords);
    }
}

fn clean_known_textures(
    active_plugin_paths: &ActivePluginPaths,
    landmass: &LandmassDiff,
    known_textures: &mut KnownTextures,
    remapped_textures: &mut RemappedTextures,
) {
    // Make sure all LTEX records have the correct filenames.

    for master in active_plugin_paths.masters.iter() {
        update_known_textures(master.to_str().unwrap(), known_textures);
    }

    for plugin in active_plugin_paths.masters.iter() {
        update_known_textures(plugin.to_str().unwrap(), known_textures);
    }

    // Determine all LTEX records in use in the final MergedLands.esp.

    let mut used_ids = vec![false; known_textures.len()];
    for (_, land) in landmass.land.iter() {
        let Some(texture_indices) = land.texture_indices else {
            continue;
        };

        for coords in texture_indices.reference.iter_grid() {
            let key = <u16 as RelativeTo>::add(
                texture_indices.reference.get(coords),
                texture_indices.relative.get(coords),
            );
            if key != 0 {
                used_ids[(key - 1) as usize] = true;
            }
        }
    }

    // Determine the remapping needed for LTEX records.

    for (new_id, (idx, _)) in used_ids
        .iter()
        .enumerate()
        .filter(|(_, is_used)| **is_used)
        .enumerate()
    {
        remapped_textures.insert(idx.try_into().unwrap(), new_id.try_into().unwrap());
    }

    let mut unused_ids = Vec::new();
    for (id, texture) in known_textures.iter_mut() {
        if let Some(new_idx) = remapped_textures.get(&texture.index.unwrap().try_into().unwrap()) {
            texture.index = Some((*new_idx).into());
        } else {
            unused_ids.push(id.clone());
        }
    }

    for id in unused_ids {
        known_textures.remove(&id);
    }
}

fn convert_landscape_diff_to_landscape(
    landscape: &LandscapeDiff,
    remapped_textures: &RemappedTextures,
) -> Landscape {
    let mut new_landscape: Landscape = default();

    new_landscape.flags = landscape.flags;
    new_landscape.grid = (landscape.coordinates.x, landscape.coordinates.y);
    new_landscape.landscape_flags = LandscapeFlags::USES_VERTEX_HEIGHTS_AND_NORMALS
        | LandscapeFlags::USES_VERTEX_COLORS
        | LandscapeFlags::USES_TEXTURES
        | LandscapeFlags::UNKNOWN;

    let height_map = calculate_terrain_map(landscape.height_map.as_ref().unwrap());
    new_landscape.vertex_heights = Some(calculate_vertex_heights_tes3(&height_map));
    new_landscape.vertex_normals = Some(VertexNormals {
        data: Box::new(convert_terrain_map(
            &recompute_normals(landscape),
            |vertex| [vertex.x, vertex.y, vertex.z],
        )),
    });

    new_landscape.vertex_colors = Some(VertexColors {
        data: Box::new(convert_terrain_map(
            &calculate_terrain_map(landscape.vertex_colors.as_ref().unwrap()),
            |vertex| [vertex.x, vertex.y, vertex.z],
        )),
    });

    let mut texture_indices = calculate_terrain_map(landscape.texture_indices.as_ref().unwrap());

    for idx in texture_indices.flatten_mut() {
        let key = *idx;
        if key != 0 {
            let old_id = key - 1;
            let new_id = *remapped_textures.get(&old_id).expect("missing remapped ID");
            *idx = new_id + 1;
        }
    }

    new_landscape.texture_indices = Some(TextureIndices {
        data: Box::new(texture_indices),
    });

    new_landscape.world_map_data = Some(WorldMapData {
        data: Box::new(calculate_terrain_map(
            landscape.world_map_data.as_ref().unwrap(),
        )),
    });

    new_landscape
}

fn convert_landmass_diff_to_landmass(
    landmass: &LandmassDiff,
    remapped_textures: &RemappedTextures,
) -> Landmass {
    let mut new_landmass = Landmass::new(landmass.plugin.clone(), landmass.data.clone());

    for (coords, land) in landmass.land.iter() {
        new_landmass.land.insert(
            *coords,
            convert_landscape_diff_to_landscape(land, remapped_textures),
        );
    }

    new_landmass
}

fn save_plugin(name: &str, landmass: &Landmass, known_textures: &KnownTextures) {
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

    for texture in known_textures
        .values()
        .sorted_by(|a, b| a.index.unwrap().cmp(&b.index.unwrap()))
    {
        plugin
            .objects
            .push(TES3Object::LandscapeTexture(texture.clone()));
    }

    for land in landmass.land.values() {
        plugin.objects.push(TES3Object::Landscape(land.clone()));
    }

    // Save the file & set the last modified time.
    plugin.save_path(merged_filepath.clone()).unwrap();
    filetime::set_file_mtime(merged_filepath, last_modified_time).unwrap();
}

fn merge_all() {
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

    let active_plugin_paths = ActivePluginPaths::new("./Morrowind.ini");

    // TODO(dvd): Handle deletions correctly.
    // TODO(dvd): Merge object flags correctly.

    // TODO(dvd): Parse all masters + plugins first, so that they don't need to be reparsed later.

    let reference_landmass = Arc::new(create_tes3_landmass(
        "ReferenceLandmass.esp",
        active_plugin_paths.masters.iter(),
        &mut known_textures,
    ));

    let modded_landmasses = active_plugin_paths
        .plugins
        .iter()
        .flat_map(|path| path.to_str())
        .flat_map(|path| {
            if path == "Merged Lands.esp" {
                return None;
            }

            // TODO(dvd): Support additional masters, e.g. for patches.
            try_create_landmass(path, &mut known_textures)
                .map(|landmass| find_landmass_diff(&landmass, reference_landmass.clone()))
        })
        .collect::<Vec<_>>();

    // STEP 2:
    // Create the MergedLands.esp:
    //  - Calculate the "naive" TES3 merge of the ordered ESMs.

    let mut merged_lands = create_merged_lands_from_reference(reference_landmass);

    // STEP 3:
    // For each LandmassDiff, [IMPLEMENTATION NOTE] same order as Plugin:
    //  - Merge into `MergedLands.esp`.
    //     - If LAND does not exist in MergedLands.esp, insert.
    //     - Else, apply merge strategies.
    //        - Each merge is applied to the result of any previous merge.
    //        - Each merge is tracked so it can be referenced in the future.
    //        - Merge strategies may use the optional `.mergedlands.toml` for conflict resolution.
    //  - TODO(dvd): Iterate through updated landmass and check for seams on any modified cell.

    for modded_landmass in modded_landmasses.iter() {
        merge_landmass_into(&mut merged_lands, modded_landmass);
    }

    // STEP 4:
    // - Iterate through cells in MergedLands.esp and drop anything that is unchanged from the
    //   reference landmass created for MergedLands.esp.
    // - Update all LandData flags to match TES3 expectations.
    // - If LandData only includes WNAM, use VCLR for subrecord.
    // [IMPLEMENTATION NOTE] This is an optimization to make MergedLands.esp friendlier.

    clean_landmass_diff(&mut merged_lands);

    // STEP 5:
    //  - Produce images of the final merge results.
    // TODO(dvd): Implement.

    // ---------------------------------------------------------------------------------------------
    // [IMPLEMENTATION NOTE] Below this line, the merged landmass cannot be diff'd against plugins.
    // ---------------------------------------------------------------------------------------------

    // STEP 6:
    // Update LTEX records to only include textures in use in modified cells.

    let mut remapped_textures = RemappedTextures::new();
    clean_known_textures(
        &active_plugin_paths,
        &merged_lands,
        &mut known_textures,
        &mut remapped_textures,
    );

    // STEP 7:
    // Convert "height map" representation of LAND records to "xy delta + offset" representation.
    // Remap texture indices.

    let landmass = convert_landmass_diff_to_landmass(&merged_lands, &remapped_textures);

    // STEP 8:
    // Save to an ESP.
    //  - [IMPLEMENTATION NOTE] Reuse last modified date if the ESP already exists.

    save_plugin("Merged Lands", &landmass, &known_textures);
}

fn main() {
    const STACK_SIZE: usize = 8 * 1024 * 1024;

    let work_thread = std::thread::Builder::new()
        .stack_size(STACK_SIZE)
        .spawn(merge_all)
        .unwrap();

    work_thread.join().unwrap();
}
