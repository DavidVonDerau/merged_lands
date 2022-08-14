use crate::io::parsed_plugins::{ParsedPlugin, ParsedPlugins};
use crate::land::grid_access::SquareGridIterator;
use crate::land::textures::{KnownTextures, RemappedTextures};
use crate::repair::seam_detection::repair_landmass_seams;
use crate::LandmassDiff;
use log::trace;
use std::sync::Arc;
use tes3::esp::LandscapeTexture;

/// Remove any unmodified [crate::LandscapeDiff] from the [LandmassDiff].
pub fn clean_landmass_diff(landmass: &mut LandmassDiff) {
    let mut unmodified = Vec::new();

    assert_eq!(repair_landmass_seams(landmass), 0);

    for (coords, land) in landmass.land.iter_mut() {
        if !land.is_modified() {
            unmodified.push(*coords);
        }
    }

    trace!("Removing {} unmodified LAND records", unmodified.len());

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

    trace!("Removing {} unused LTEX records", num_removed_ids);
    trace!("Remapping {} LTEX records", known_textures.len());

    remapped_textures
}
