use crate::land::grid_access::SquareGridIterator;
use crate::land::landscape_diff::LandscapeDiff;
use crate::land::terrain_map::Vec3;
use crate::merge::conflict::{ConflictResolver, ConflictType};
use crate::merge::relative_terrain_map::RelativeTerrainMap;
use crate::merge::relative_to::RelativeTo;
use crate::LandmassDiff;
use std::default::default;

/// Adds any conflicts between the `lhs` [RelativeTerrainMap] and
/// the `rhs` [RelativeTerrainMap] to the `vertex_colors`.
pub fn add_vertex_colors<U: RelativeTo + ConflictResolver, const T: usize>(
    lhs: Option<&RelativeTerrainMap<U, T>>,
    rhs: Option<&RelativeTerrainMap<U, T>>,
    vertex_colors: Option<&mut RelativeTerrainMap<Vec3<u8>, T>>,
) {
    let Some(lhs) = lhs else {
        return;
    };

    let Some(rhs) = rhs else {
        return;
    };

    let Some(vertex_colors) = vertex_colors else {
        return;
    };

    let params = default();

    const MAJOR_COLOR: Vec3<u8> = Vec3::new(255u8, 0, 0);
    const MINOR_COLOR: Vec3<u8> = Vec3::new(255u8, 255u8, 0);
    const MODIFIED_COLOR: Vec3<u8> = Vec3::new(0, 255u8, 0);
    const UNMODIFIED_COLOR: Vec3<u8> = Vec3::new(0, 0, 0);

    for coords in lhs.iter_grid() {
        let actual = lhs.get_value(coords);
        let expected = rhs.get_value(coords);
        let has_difference = rhs.has_difference(coords);

        let debug_color = if has_difference {
            match actual.average(expected, &params) {
                None => MODIFIED_COLOR,
                Some(ConflictType::Minor(_)) => MINOR_COLOR,
                Some(ConflictType::Major(_)) => MAJOR_COLOR,
            }
        } else {
            UNMODIFIED_COLOR
        };

        if debug_color == UNMODIFIED_COLOR {
            continue;
        }

        let current_color = vertex_colors.get_value(coords);
        let can_paint = (debug_color == MAJOR_COLOR)
            || (debug_color == MINOR_COLOR && current_color != MAJOR_COLOR);
        if can_paint {
            vertex_colors.set_value(coords, debug_color);
        }
    }
}

/// Add vertex colors to [LandscapeDiff] `reference` for any conflict found with `plugin`.
fn add_debug_vertex_colors_to_landscape(reference: &mut LandscapeDiff, plugin: &LandscapeDiff) {
    add_vertex_colors(
        reference.height_map.as_ref(),
        plugin.height_map.as_ref(),
        reference.vertex_colors.as_mut(),
    );
}

/// Add vertex colors to [LandmassDiff] `reference` for any conflict found with `plugin`.
pub fn add_debug_vertex_colors_to_landmass(reference: &mut LandmassDiff, plugin: &LandmassDiff) {
    for (coords, land) in plugin.sorted() {
        let merged_land = reference.land.get_mut(coords).expect("safe");
        add_debug_vertex_colors_to_landscape(merged_land, land);
    }
}
