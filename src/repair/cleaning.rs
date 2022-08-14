use crate::io::parsed_plugins::{is_esp, ParsedPlugin, ParsedPlugins};
use crate::land::grid_access::SquareGridIterator;
use crate::land::landscape_diff::LandscapeDiff;
use crate::land::textures::{KnownTextures, RemappedTextures};
use crate::merge::conflict::{ConflictResolver, ConflictType};
use crate::merge::relative_terrain_map::RelativeTerrainMap;
use crate::merge::relative_to::RelativeTo;
use crate::repair::seam_detection::repair_landmass_seams;
use crate::LandmassDiff;
use log::debug;
use std::collections::HashMap;
use std::default::default;
use std::sync::Arc;
use tes3::esp::LandscapeTexture;

pub fn has_difference<U: RelativeTo + ConflictResolver, const T: usize>(
    lhs: Option<&RelativeTerrainMap<U, T>>,
    rhs: Option<&RelativeTerrainMap<U, T>>,
) -> bool {
    let Some(lhs) = lhs else {
        return false;
    };

    let Some(rhs) = rhs else {
        return false;
    };

    let params = default();

    for coords in lhs.iter_grid() {
        let actual = lhs.get_value(coords);
        let expected = rhs.get_value(coords);
        match actual.average(expected, &params) {
            None => {}
            Some(ConflictType::Minor(_)) => {
                return true;
            }
            Some(ConflictType::Major(_)) => {
                return true;
            }
        }
    }

    false
}

fn has_any_difference(reference: &LandscapeDiff, plugin: &LandscapeDiff) -> bool {
    has_difference(reference.height_map.as_ref(), plugin.height_map.as_ref())
        || has_difference(
            reference.vertex_normals.as_ref(),
            plugin.vertex_normals.as_ref(),
        )
        || has_difference(
            reference.world_map_data.as_ref(),
            plugin.world_map_data.as_ref(),
        )
        || has_difference(
            reference.vertex_colors.as_ref(),
            plugin.vertex_colors.as_ref(),
        )
        || has_difference(
            reference.texture_indices.as_ref(),
            plugin.texture_indices.as_ref(),
        )
}

/// Remove any unmodified [crate::LandscapeDiff] from the [LandmassDiff].
pub fn clean_landmass_diff(landmass: &mut LandmassDiff, modded_landmasses: &[LandmassDiff]) {
    assert_eq!(repair_landmass_seams(landmass), 0);

    let mut modded_landmasses_map = HashMap::with_capacity(modded_landmasses.len());
    for modded_landmass in modded_landmasses.iter() {
        modded_landmasses_map.insert(modded_landmass.plugin.name.clone(), modded_landmass);
    }

    let mut unmodified = Vec::new();
    let mut num_unmodified_from_reference = 0;
    let mut num_unmodified_from_plugin = 0;

    for (coords, land) in landmass.land.iter_mut() {
        if !land.is_modified() {
            unmodified.push(*coords);
            num_unmodified_from_reference += 1;
            continue;
        };

        let num_esps = land
            .plugins
            .iter()
            .filter(|plugin| is_esp(&plugin.0.name))
            .count();

        if num_esps != 1 {
            continue;
        }

        let plugin = land.plugins.last().expect("safe").0.clone();
        assert!(is_esp(&plugin.name));

        let modded_landmass = modded_landmasses_map.get(&plugin.name).expect("safe");
        let modded_landmass_land = modded_landmass.land.get(coords).expect("safe");
        if !has_any_difference(land, modded_landmass_land) {
            unmodified.push(*coords);
            num_unmodified_from_plugin += 1;
        }
    }

    debug!(
        "Removing {} LAND records unmodified from reference",
        num_unmodified_from_reference
    );

    debug!(
        "Removing {} LAND records unmodified from plugins",
        num_unmodified_from_plugin
    );

    for coords in unmodified.drain(..) {
        landmass.land.remove(&coords);
    }
}

/// Remove any unused [crate::land::textures::KnownTexture] from the [KnownTextures].
/// Returns [RemappedTextures] for anything that was not removed.
pub fn clean_known_textures(
    parsed_plugins: &ParsedPlugins,
    landmass: &LandmassDiff,
    known_textures: &mut KnownTextures,
) -> RemappedTextures {
    assert!(
        known_textures.len() < u16::MAX as usize,
        "exceeded maximum number of textures"
    );

    fn update_known_textures(plugin: &Arc<ParsedPlugin>, known_textures: &mut KnownTextures) {
        for texture in plugin.records.objects_of_type::<LandscapeTexture>() {
            known_textures.update_texture(plugin, texture);
        }
    }

    // Make sure all LTEX records have the correct filenames.

    for master in parsed_plugins.masters.iter() {
        update_known_textures(master, known_textures);
    }

    for plugin in parsed_plugins.masters.iter() {
        update_known_textures(plugin, known_textures);
    }

    // Determine all LTEX records in use in the final MergedLands.esp.
    // Reserve extra texture index for the default 0th texture.

    let mut used_ids = vec![false; known_textures.len() + 1];
    used_ids[0] = true; // Assume the default texture is in use.
    for (_, land) in landmass.sorted() {
        let Some(texture_indices) = land.texture_indices.as_ref() else {
            continue;
        };

        for coords in texture_indices.iter_grid() {
            let key = texture_indices.get_value(coords);
            used_ids[key.as_u16() as usize] = true;
        }
    }

    // Determine the remapping needed for LTEX records.

    let remapped_textures = RemappedTextures::from(&used_ids);
    let num_removed_ids = known_textures.remove_unused(&remapped_textures);

    debug!("Removing {} unused LTEX records", num_removed_ids);
    debug!("Remapping {} LTEX records", known_textures.len());

    remapped_textures
}
