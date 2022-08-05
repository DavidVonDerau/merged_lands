use crate::land::grid_access::{GridAccessor2D, GridIterator2D, Index2D, SquareGridIterator};
use crate::land::height_map::calculate_vertex_normals_map;
use crate::land::terrain_map::{TerrainMap, Vec3};
use crate::merge::relative_to::RelativeTo;
use std::default::default;

#[derive(Clone)]
pub struct RelativeTerrainMap<U: RelativeTo, const T: usize> {
    reference: TerrainMap<U, T>,
    relative: TerrainMap<<U as RelativeTo>::Delta, T>,
    has_difference: TerrainMap<bool, T>,
}

pub struct DefaultRelativeTerrainMap {}

impl DefaultRelativeTerrainMap {
    pub const HEIGHT_MAP: RelativeTerrainMap<i32, 65> =
        RelativeTerrainMap::new([[0; 65]; 65], [[0; 65]; 65], [[false; 65]; 65]);

    pub const VERTEX_NORMALS: RelativeTerrainMap<Vec3<i8>, 65> = RelativeTerrainMap::new(
        [[Vec3::new(0, 0, 0); 65]; 65],
        [[Vec3::new(0, 0, 0); 65]; 65],
        [[false; 65]; 65],
    );
}

impl<U: RelativeTo, const T: usize> SquareGridIterator<T> for RelativeTerrainMap<U, T> {
    fn iter_grid(&self) -> GridIterator2D<T, T> {
        default()
    }
}

impl<U: RelativeTo, const T: usize> RelativeTerrainMap<U, T> {
    pub const fn new(
        reference: TerrainMap<U, T>,
        relative: TerrainMap<<U as RelativeTo>::Delta, T>,
        has_difference: TerrainMap<bool, T>,
    ) -> Self {
        Self {
            reference,
            relative,
            has_difference,
        }
    }

    pub fn from_difference(
        reference: &TerrainMap<U, T>,
        plugin: &TerrainMap<U, T>,
    ) -> RelativeTerrainMap<U, T> {
        let mut output = RelativeTerrainMap::new(*reference, [[default(); T]; T], [[false; T]; T]);

        for coords in reference.iter_grid() {
            output.set_value(coords, plugin.get(coords));
        }

        output
    }

    pub fn differences(&self) -> &TerrainMap<bool, T> {
        &self.has_difference
    }

    pub fn get_value(&self, coords: Index2D) -> U {
        <U as RelativeTo>::add(self.reference.get(coords), self.relative.get(coords))
    }

    pub fn set_value(&mut self, coords: Index2D, value: U) {
        let difference = U::subtract(value, self.reference.get(coords));
        *self.relative.get_mut(coords) = difference;
        *self.has_difference.get_mut(coords) = difference != default();
    }

    pub fn get_difference(&self, coords: Index2D) -> <U as RelativeTo>::Delta {
        let delta = self.relative.get(coords);
        if delta == default() {
            assert!(!self.has_difference.get(coords));
        } else {
            assert!(self.has_difference.get(coords));
        }
        delta
    }

    pub fn set_difference(&mut self, coords: Index2D, difference: <U as RelativeTo>::Delta) {
        *self.relative.get_mut(coords) = difference;
        *self.has_difference.get_mut(coords) = difference != default();
    }

    pub fn has_difference(&self, coords: Index2D) -> bool {
        if self.has_difference.get(coords) {
            assert_ne!(self.relative.get(coords), default());
            true
        } else {
            assert_eq!(self.relative.get(coords), default());
            false
        }
    }

    pub fn clean_all(&mut self) {
        for v in self.has_difference.flatten_mut() {
            *v = false;
        }

        for v in self.relative.flatten_mut() {
            *v = default();
        }
    }

    pub fn clean_some(&mut self, iter: impl Iterator<Item = Index2D>) {
        for coords in iter {
            *self.has_difference.get_mut(coords) = false;
            *self.relative.get_mut(coords) = default();
        }
    }

    pub fn to_terrain(&self) -> TerrainMap<U, T> {
        let mut terrain = [[default(); T]; T];
        for coords in self.iter_grid() {
            *terrain.get_mut(coords) = self.get_value(coords);
        }
        terrain
    }
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

pub type OptionalTerrainMap<U, const T: usize> = Option<RelativeTerrainMap<U, T>>;

impl<U: RelativeTo, const T: usize> IsModified for OptionalTerrainMap<U, T> {
    fn is_modified(&self) -> bool {
        self.as_ref().map(|map| map.is_modified()).unwrap_or(false)
    }

    fn num_differences(&self) -> usize {
        self.as_ref().map(|map| map.num_differences()).unwrap_or(0)
    }
}

pub fn recompute_vertex_normals(
    height_map: &RelativeTerrainMap<i32, 65>,
    vertex_normals: &RelativeTerrainMap<Vec3<i8>, 65>,
) -> TerrainMap<Vec3<i8>, 65> {
    let height_map_abs = height_map.to_terrain();

    let mut recomputed_vertex_normals = calculate_vertex_normals_map(&height_map_abs);
    for coords in vertex_normals.iter_grid() {
        if !height_map.has_difference(coords) {
            assert_eq!(vertex_normals.get_difference(coords), default());
            *recomputed_vertex_normals.get_mut(coords) = vertex_normals.get_value(coords);
        }
    }

    recomputed_vertex_normals
}
