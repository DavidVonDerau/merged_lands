use crate::land::grid_access::Index2D;
use crate::land::terrain_map::Vec2;
use crate::merge::relative_terrain_map::RelativeTerrainMap;
use crate::LandmassDiff;
use hashbrown::HashSet;
use itertools::Itertools;
use log::{debug, trace};
use std::cmp::Ordering;
use std::collections::VecDeque;

/// Calculates new coordinates by adding the `offset` to the `coords`.
fn coords_with_offset(coords: Vec2<i32>, offset: [i32; 2]) -> Vec2<i32> {
    Vec2::new(coords.x + offset[0], coords.y + offset[1])
}

/// Given a `coords`, adds the four (N, W, S, E) adjacent sides to the
/// list of `possible_seams` if they are not already `visited`.
fn push_back_neighbors(
    possible_seams: &mut VecDeque<(Vec2<i32>, Vec2<i32>)>,
    visited: &mut HashSet<(Vec2<i32>, Vec2<i32>)>,
    coords: Vec2<i32>,
) {
    /// Sorts a pair of `Vec2` coordinates by `x` and then `y`.
    fn sort_pair(lhs: Vec2<i32>, rhs: Vec2<i32>) -> (Vec2<i32>, Vec2<i32>) {
        assert_ne!(lhs, rhs);
        match lhs.x.cmp(&rhs.x) {
            Ordering::Greater => (rhs, lhs),
            Ordering::Less => (lhs, rhs),
            Ordering::Equal => match lhs.y.cmp(&rhs.y) {
                Ordering::Greater => (rhs, lhs),
                Ordering::Less => (lhs, rhs),
                _ => unreachable!(),
            },
        }
    }

    for offset in [[-1, 0], [1, 0], [0, 1], [0, -1]] {
        let neighbor = coords_with_offset(coords, offset);
        let pair = sort_pair(coords, neighbor);
        if visited.insert(pair) {
            possible_seams.push_back(pair);
        }
    }
}

/// A corner of a landscape.
struct Corner {
    coords: Index2D,
    cell_offset: [i32; 2],
}

/// A set of 4 [Corner] relative to the current land.
/// Corner seams are repaired by inspecting all 4 vertices
/// meeting at the same corner.
struct CornerCase {
    corners: [Corner; 4],
}

/// Repairs corner seams by averaging their values together.
fn repair_corner_seams(
    merged: &mut LandmassDiff,
    coords: Vec2<i32>,
    num_seams_repaired: &mut usize,
) {
    let cases = [
        CornerCase {
            corners: [
                Corner {
                    coords: Index2D::new(0, 0),
                    cell_offset: [0, 0],
                },
                Corner {
                    coords: Index2D::new(0, 64),
                    cell_offset: [0, -1],
                },
                Corner {
                    coords: Index2D::new(64, 0),
                    cell_offset: [-1, 0],
                },
                Corner {
                    coords: Index2D::new(64, 64),
                    cell_offset: [-1, -1],
                },
            ],
        },
        CornerCase {
            corners: [
                Corner {
                    coords: Index2D::new(64, 0),
                    cell_offset: [0, 0],
                },
                Corner {
                    coords: Index2D::new(64, 64),
                    cell_offset: [0, -1],
                },
                Corner {
                    coords: Index2D::new(0, 0),
                    cell_offset: [1, 0],
                },
                Corner {
                    coords: Index2D::new(0, 64),
                    cell_offset: [1, -1],
                },
            ],
        },
        CornerCase {
            corners: [
                Corner {
                    coords: Index2D::new(64, 64),
                    cell_offset: [0, 0],
                },
                Corner {
                    coords: Index2D::new(64, 0),
                    cell_offset: [0, 1],
                },
                Corner {
                    coords: Index2D::new(0, 64),
                    cell_offset: [1, 0],
                },
                Corner {
                    coords: Index2D::new(0, 0),
                    cell_offset: [1, 1],
                },
            ],
        },
        CornerCase {
            corners: [
                Corner {
                    coords: Index2D::new(0, 64),
                    cell_offset: [0, 0],
                },
                Corner {
                    coords: Index2D::new(0, 0),
                    cell_offset: [0, 1],
                },
                Corner {
                    coords: Index2D::new(64, 64),
                    cell_offset: [-1, 0],
                },
                Corner {
                    coords: Index2D::new(64, 0),
                    cell_offset: [-1, 1],
                },
            ],
        },
    ];

    for case in cases.iter() {
        let average = {
            let adjacent_values = case.corners.iter().map(|corner| {
                merged
                    .land
                    .get(&coords_with_offset(coords, corner.cell_offset))
                    .and_then(|land| land.height_map.as_ref())
                    .map(|height_map| height_map.get_value(corner.coords))
            });

            let mut average = 0;
            let mut num_values = 0;
            for value in adjacent_values.flatten() {
                average += value;
                num_values += 1;
            }

            if num_values > 0 {
                average /= num_values;
                Some(average)
            } else {
                None
            }
        };

        let Some(average) = average else {
            continue;
        };

        for corner in case.corners.iter() {
            let Some(land) = merged
                .land
                .get_mut(&coords_with_offset(coords, corner.cell_offset)) else {
                continue;
            };

            let Some(height_map) = land.height_map.as_mut() else {
                continue;
            };

            if height_map.get_value(corner.coords) != average {
                height_map.set_value(corner.coords, average);
                *num_seams_repaired += 1;
            }
        }
    }
}

/// Repairs landmass seams by a two-step algorithm. First, the algorithm repairs any
/// corner seams by averaging the values of all vertices shared by 4 cells. Then, the
/// algorithm will repair seams on the sides between cells by picking the average value
/// of both sides. For performance, only seams adjacent to coordinates in the `possible_seams`
/// field of the [LandmassDiff] will be visited.
pub fn repair_landmass_seams(merged: &mut LandmassDiff) -> usize {
    let mut possible_seams = VecDeque::new();
    let mut visited = HashSet::new();
    let mut repaired = HashSet::new();

    let mut num_seams_repaired = 0;

    for coords in merged.sorted().map(|pair| *pair.0).collect_vec() {
        repair_corner_seams(merged, coords, &mut num_seams_repaired);
        push_back_neighbors(&mut possible_seams, &mut visited, coords);
    }

    /// Repairs a seam shared by two cells along a side.
    fn try_repair_seam<const T: usize>(
        lhs_coord: Index2D,
        rhs_coord: Index2D,
        lhs_map: &mut RelativeTerrainMap<i32, T>,
        rhs_map: &mut RelativeTerrainMap<i32, T>,
        index: usize,
    ) -> usize {
        let lhs_value = lhs_map.get_value(lhs_coord);
        let rhs_value = rhs_map.get_value(rhs_coord);
        if lhs_value != rhs_value {
            assert!(
                index != 0 && index != 64,
                "corners should have been fixed first"
            );

            // TODO(dvd): #feature Should this use the ConflictResolver instead?
            let average = (lhs_value + rhs_value) / 2;
            let lhs_diff = (average - lhs_value).abs();
            let rhs_diff = (average - rhs_value).abs();
            lhs_map.set_value(lhs_coord, average);
            rhs_map.set_value(rhs_coord, average);
            lhs_diff.max(rhs_diff) as usize
        } else {
            0
        }
    }

    while !possible_seams.is_empty() {
        let next = possible_seams.pop_front().expect("safe");

        let Some(mut lands) = merged.land.get_many_mut([&next.0, &next.1]) else {
            continue;
        };

        let (lhs, rhs) = lands.split_at_mut(1);
        let lhs = &mut lhs[0];
        let rhs = &mut rhs[0];

        let Some(lhs_height_map) = lhs.height_map.as_mut() else {
            continue;
        };

        let Some(rhs_height_map) = rhs.height_map.as_mut() else {
            continue;
        };

        let is_top_seam = if lhs.coords.x == rhs.coords.x {
            assert!(lhs.coords.y < rhs.coords.y);
            true
        } else {
            assert!(lhs.coords.x < rhs.coords.x);
            false
        };

        let mut seam_size = 0;
        let mut sum = 0;
        let mut max_delta = usize::MIN;
        let mut min_delta = usize::MAX;
        if is_top_seam {
            for x in 0..65 {
                let lhs_coord = Index2D::new(x, 64);
                let rhs_coord = Index2D::new(x, 0);
                let delta =
                    try_repair_seam(lhs_coord, rhs_coord, lhs_height_map, rhs_height_map, x);
                if delta > 0 {
                    num_seams_repaired += 1;
                    seam_size += 1;
                    sum += delta;
                    max_delta = max_delta.max(delta);
                    min_delta = min_delta.min(delta);
                }
            }
        } else {
            for y in 0..65 {
                let lhs_coord = Index2D::new(64, y);
                let rhs_coord = Index2D::new(0, y);
                let delta =
                    try_repair_seam(lhs_coord, rhs_coord, lhs_height_map, rhs_height_map, y);
                if delta > 0 {
                    num_seams_repaired += 1;
                    seam_size += 1;
                    sum += delta;
                    max_delta = max_delta.max(delta);
                    min_delta = min_delta.min(delta);
                }
            }
        }

        if seam_size > 0 {
            let average = sum / seam_size;
            repaired.insert((next, seam_size, max_delta, min_delta, average));
        }
    }

    if num_seams_repaired > 0 {
        debug!("Repaired {} seams", num_seams_repaired);
        for seam in repaired.iter().sorted_by_key(|a| std::cmp::Reverse(a.1)) {
            trace!(
                " - ({:>4}, {:>4}) | ({:>4}, {:>4}) | # of Seams = {:<3} | Max = {:<3} | Min = {:<3} | Avg = {}",
                seam.0 .0.x,
                seam.0 .0.y,
                seam.0 .1.x,
                seam.0 .1.y,
                seam.1,
                seam.2,
                seam.3,
                seam.4
            );
        }
    }

    num_seams_repaired
}
