#![feature(slice_flatten)]
#![feature(let_else)]
#![feature(default_free_fn)]
#![feature(try_blocks)]

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

use crate::conflict::{ConflictResolver, ConflictType};
use crate::grid_access::{GridAccessor2D, SquareGridIterator};
use crate::height_map::calculate_height_map;
use crate::merge_strategy::{apply_merge_strategy, MergeStrategy};
use crate::overwrite_strategy::OverwriteStrategy;
use crate::parser::*;
use crate::relative_to::RelativeTo;
use crate::resolve_conflict_strategy::ResolveConflictStrategy;
use crate::round_to::RoundTo;
use crate::save_to_image::SaveToImage;
use std::collections::HashMap;
use std::default::default;
use std::str;

#[derive(Copy, Clone)]
pub struct RelativeTerrainMap<U: RelativeTo, const T: usize> {
    reference: TerrainMap<U, T>,
    relative: TerrainMap<<U as RelativeTo>::Delta, T>,
    has_difference: TerrainMap<bool, T>,
}

pub type OptionalTerrain<U, const T: usize> = Option<RelativeTerrainMap<U, T>>;

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

#[derive(Copy, Clone)]
struct PluginLAND {
    coordinates: Vec2<i32>,
    height_map: OptionalTerrain<i32, 65>,
    vertex_normals: OptionalTerrain<Vec3<i8>, 65>,
    world_map_height: OptionalTerrain<u8, 9>,
    vertex_colors: OptionalTerrain<Vec3<u8>, 65>,
    vertex_textures: OptionalTerrain<u16, 16>,
}

fn apply_mask<U: RelativeTo, const T: usize>(
    old: RelativeTerrainMap<U, T>,
    allow: Option<&TerrainMap<bool, T>>,
) -> OptionalTerrain<U, T> {
    let allowed = allow?;

    let mut new = old;

    for coords in old
        .relative
        .iter_grid()
        .filter(|coords| !allowed.get(*coords))
    {
        *new.has_difference.get_mut(coords) = false;
        *new.relative.get_mut(coords) = default();
    }

    new.has_difference.flatten().contains(&true).then_some(new)
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
    if !relative.has_difference.flatten().contains(&true) {
        println!("{}: same", value);
        return None;
    }

    let updated = if use_mask {
        apply_mask(relative, allow)
    } else {
        Some(relative)
    };

    let num_differences = updated
        .map(|t| t.has_difference.flatten().iter().filter(|v| **v).count())
        .unwrap_or(0);

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

fn remap_vtex(
    vtex: TerrainMap<u16, 16>,
    landmass: &Landmass,
    reference_landmass: &Landmass,
) -> TerrainMap<u16, 16> {
    let mut updated = vtex;

    for row in updated.iter_mut() {
        for id in row.iter_mut() {
            if *id == 0 {
                // Default texture.
            } else {
                let ltex = landmass
                    .textures
                    .iter()
                    .find(|ltex| ltex.landscape_id == (*id - 1) as u32)
                    .unwrap();

                if let Some(ref_ltex) = reference_landmass
                    .textures
                    .iter()
                    .find(|ref_ltex| ref_ltex.id == ltex.id)
                {
                    // Find the equivalent ID in the reference.
                    *id = (ref_ltex.landscape_id + 1) as u16;
                } else {
                    *id = (reference_landmass.textures.len() + (*id as usize) + 1) as u16;
                }
            }
        }
    }

    updated
}

impl PluginLAND {
    fn from_difference(
        land: &LAND,
        landmass: &Landmass,
        reference: Option<&LAND>,
        reference_landmass: &Landmass,
        allowed_data: DataFlags,
    ) -> Self {
        let height_map = calculate_differences(
            "height_map",
            land.included_data.contains(DataFlags::VNML_VHGT_WNAM)
                && allowed_data.contains(DataFlags::VHGT),
            reference.and_then(calculate_height_map),
            calculate_height_map(land),
        );

        let vertex_normals = calculate_differences_with_mask(
            "vertex_normals",
            land.included_data.contains(DataFlags::VNML_VHGT_WNAM)
                && allowed_data.contains(DataFlags::VNML),
            reference.and_then(|r| r.vertex_normals),
            land.vertex_normals,
            true,
            height_map.map(|h| h.has_difference).as_ref(),
        );

        let world_map_height = calculate_differences(
            "world_map_height",
            land.included_data.contains(DataFlags::VNML_VHGT_WNAM)
                && allowed_data.contains(DataFlags::WNAM),
            reference.and_then(|r| r.world_map_height),
            land.world_map_height,
        );

        let vertex_colors = calculate_differences(
            "vertex_colors",
            land.included_data.contains(DataFlags::VCLR) && allowed_data.contains(DataFlags::VCLR),
            reference.and_then(|r| r.vertex_colors),
            land.vertex_colors,
        );

        let remapped_vertex_textures = land
            .vertex_textures
            .map(|vtex| remap_vtex(vtex, landmass, reference_landmass));

        let vertex_textures = calculate_differences(
            "vertex_textures",
            land.included_data.contains(DataFlags::VTEX) && allowed_data.contains(DataFlags::VTEX),
            reference.and_then(|r| r.vertex_textures),
            remapped_vertex_textures,
        );

        Self {
            coordinates: land.coordinates,
            height_map,
            vertex_normals,
            world_map_height,
            vertex_colors,
            vertex_textures,
        }
    }
}

struct Landmass {
    plugin: String,
    land: HashMap<Vec2<i32>, LAND>,
    textures: Vec<LTEX>,
}

impl Landmass {
    fn new(plugin: String) -> Self {
        Self {
            plugin,
            land: HashMap::new(),
            textures: Vec::new(),
        }
    }
}

struct ModdedLandmass {
    plugin: String,
    land: HashMap<Vec2<i32>, PluginLAND>,
    textures: Vec<LTEX>,
}

impl ModdedLandmass {
    fn new(plugin: String) -> Self {
        Self {
            plugin,
            land: HashMap::new(),
            textures: Vec::new(),
        }
    }
}

fn create_landmass(data: &ESData) -> Option<Landmass> {
    let mut landmass = Landmass::new(data.plugin.clone());

    if let Some(land_records) = data.records.get::<Vec<LAND>>() {
        for land in land_records.iter() {
            let coords = land.coordinates;
            landmass.land.insert(coords, *land);
        }
    }

    if let Some(textures) = data.records.get::<Vec<LTEX>>() {
        for texture in textures.iter() {
            landmass.textures.push(texture.clone());
        }
    }

    if !landmass.land.is_empty() {
        Some(landmass)
    } else {
        None
    }
}

fn parse_landmass(plugin: &str) -> Option<Landmass> {
    let data = parse_records(plugin);
    create_landmass(&data)
}

fn tes3_merge_land(lhs: &LAND, rhs: &LAND) -> LAND {
    let mut land = *lhs;

    if rhs.included_data.contains(DataFlags::VNML_VHGT_WNAM) {
        land.included_data |= DataFlags::VNML_VHGT_WNAM;
        land.vertex_normals = rhs.vertex_normals;
        land.height_data = rhs.height_data;
        land.world_map_height = rhs.world_map_height;
    }

    if rhs.included_data.contains(DataFlags::VTEX) {
        land.included_data |= DataFlags::VTEX;
        land.vertex_textures = rhs.vertex_textures;
    }

    if rhs.included_data.contains(DataFlags::VCLR) {
        land.included_data |= DataFlags::VCLR;
        land.vertex_colors = rhs.vertex_colors;
    }

    land
}

fn tes3_merge_landmasses<'a, I>(landmasses: I) -> Landmass
where
    I: Iterator<Item = &'a Option<Landmass>>,
{
    let mut merged_landmass = Landmass::new("ReferenceLandmass.esp".to_string());

    for landmass in landmasses.filter_map(|x| x.as_ref()) {
        for (coords, land) in landmass.land.iter() {
            if merged_landmass.land.contains_key(coords) {
                println!("Merging {:?} from {}", coords, landmass.plugin);
                let merged_land = tes3_merge_land(merged_landmass.land.get(coords).unwrap(), land);
                merged_landmass.land.insert(*coords, merged_land);
            } else {
                println!("Inserting {:?} from {}", coords, landmass.plugin);
                merged_landmass.land.insert(*coords, *land);
            }
        }

        for texture in landmass.textures.iter() {
            merged_landmass.textures.push(texture.clone())
        }
    }

    merged_landmass
}

fn calculate_modded_landmass(landmass: &Landmass, reference: &Landmass) -> ModdedLandmass {
    let mut modded_landmass = ModdedLandmass::new(landmass.plugin.clone());
    let is_cantons = landmass.plugin == "Cantons_on_the_Global_Map_v1.1.esp";

    for (coords, land) in landmass.land.iter() {
        let reference_land = reference.land.get(coords);

        let mut allowed_data = land.included_data;
        if land.included_data.contains(DataFlags::VNML_VHGT_WNAM) {
            allowed_data |= DataFlags::VNML;
            allowed_data |= DataFlags::VHGT;
            allowed_data |= DataFlags::WNAM;
        }

        if is_cantons {
            // HACK.
            allowed_data = DataFlags::WNAM;
        }

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

        modded_landmass.land.insert(
            *coords,
            PluginLAND::from_difference(land, landmass, reference_land, reference, allowed_data),
        );
    }

    modded_landmass
}

fn merge_land(plugin: &str, old: &PluginLAND, new: &PluginLAND) -> PluginLAND {
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
    merged.world_map_height = apply_merge_strategy(
        coords,
        plugin,
        "world_map_height",
        old.world_map_height,
        new.world_map_height,
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

    merged.vertex_textures = apply_merge_strategy(
        coords,
        plugin,
        "vertex_textures",
        old.vertex_textures,
        new.vertex_textures,
        &overwrite_strategy,
    );

    merged
}

fn merge_landmasses(merged: &mut ModdedLandmass, plugin: &ModdedLandmass) {
    // TODO(dvd): Track plugin names on individual LAND records.
    println!("Merging {} into {}", plugin.plugin, merged.plugin);
    for (coords, land) in plugin.land.iter() {
        if merged.land.contains_key(coords) {
            println!("Merging {} into {:?}", plugin.plugin, coords);
            let merged_land = merged.land.get(coords).unwrap();
            merged
                .land
                .insert(*coords, merge_land(&plugin.plugin, merged_land, land));
        } else {
            println!("Adding {} at {:?}", plugin.plugin, coords);
            merged.land.insert(*coords, *land);
        }
    }
}

fn merge_all() {
    let masters = ["Morrowind.esm", "Tribunal.esm", "Bloodmoon.esm"];

    let master_landmasses = masters.map(parse_landmass);
    let reference_landmass = tes3_merge_landmasses(master_landmasses.iter());

    let mut merged_lands = ModdedLandmass::new("MergedLands.esp".to_string());

    let plugins = [
        "OAAB - The Ashen Divide.ESP",
        "Imperial Cart Travel.esp",
        "DD_Caldera_Expansion.esp",
        "The Haunted Tavern of the West Gash.esp",
        "Better Landscapes Stonewood Pass (RP Edit).esp",
        "Astrologians Guild.esp",
        "EastsideTravel.esp",
        "Welcome Home - No Solstheim.esp",
        "PackGuarTravel.esp",
        "Guar stables of Vivec.esp",
        "KogoruhnExpanded.esp",
        "Beautiful cities of morrowind.esp",
        "RedMountainReborn.esp",
        "OAAB - Foyada Mamaea.esp",
        "OAAB_Tel Mora.ESP",
        "BalmoraDocks.esp",
        "Cantons_on_the_Global_Map_v1.1.esp",
    ];

    let modded_landmasses = plugins
        .iter()
        .filter_map(|s| parse_landmass(s))
        .map(|landmass| calculate_modded_landmass(&landmass, &reference_landmass))
        .collect::<Vec<_>>();

    for modded_landmass in modded_landmasses.iter() {
        merge_landmasses(&mut merged_lands, modded_landmass);
    }
}

fn main() {
    const STACK_SIZE: usize = 8 * 1024 * 1024;

    let work_thread = std::thread::Builder::new()
        .stack_size(STACK_SIZE)
        .spawn(merge_all)
        .unwrap();

    work_thread.join().unwrap();
}
