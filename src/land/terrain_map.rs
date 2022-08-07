use crate::land::grid_access::{GridAccessor2D, GridIterator2D, Index2D, SquareGridIterator};
use bitflags::bitflags;
use const_default::ConstDefault;
use std::default::default;
use tes3::esp::LandscapeFlags;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
#[repr(C)]
/// A [Vec2] is an `x` and `y` value. Can be converted to and from `[T; 2]`.
pub struct Vec2<T> {
    pub x: T,
    pub y: T,
}

impl<T: ConstDefault> ConstDefault for Vec2<T> {
    const DEFAULT: Self = Vec2::new(<T as ConstDefault>::DEFAULT, <T as ConstDefault>::DEFAULT);
}

impl<T> Vec2<T> {
    pub const fn new(x: T, y: T) -> Self {
        Self { x, y }
    }
}

impl<T: Copy> From<[T; 2]> for Vec2<T> {
    fn from(array: [T; 2]) -> Self {
        Self::new(array[0], array[1])
    }
}

impl<T> From<Vec2<T>> for [T; 2] {
    fn from(vec: Vec2<T>) -> Self {
        [vec.x, vec.y]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Default, Hash)]
#[repr(C)]
/// A [Vec3] is an `x`, `y`, and `z` value. Can be converted to and from `[T; 3]`.
pub struct Vec3<T> {
    pub x: T,
    pub y: T,
    pub z: T,
}

impl<T: ConstDefault> ConstDefault for Vec3<T> {
    const DEFAULT: Self = Vec3::new(
        <T as ConstDefault>::DEFAULT,
        <T as ConstDefault>::DEFAULT,
        <T as ConstDefault>::DEFAULT,
    );
}

impl<T> Vec3<T> {
    pub const fn new(x: T, y: T, z: T) -> Self {
        Self { x, y, z }
    }
}

impl<T: Copy> From<[T; 3]> for Vec3<T> {
    fn from(array: [T; 3]) -> Self {
        Self::new(array[0], array[1], array[2])
    }
}

impl<T> From<Vec3<T>> for [T; 3] {
    fn from(vec: Vec3<T>) -> Self {
        [vec.x, vec.y, vec.z]
    }
}

/// A wrapper type for `[[U; T]; T]`.
/// Implements [GridAccessor2D] and [SquareGridIterator].
pub type TerrainMap<U, const T: usize> = [[U; T]; T];

impl<U: Copy, const T: usize> GridAccessor2D<U> for TerrainMap<U, T> {
    fn get(&self, coords: Index2D) -> U {
        self[coords.y][coords.x]
    }

    fn get_mut(&mut self, coords: Index2D) -> &mut U {
        &mut self[coords.y][coords.x]
    }
}

impl<U, const T: usize> SquareGridIterator<T> for TerrainMap<U, T> {
    fn iter_grid(&self) -> GridIterator2D<T, T> {
        default()
    }
}

bitflags! {
    #[derive(Default)]
    /// The data included with some [Landscape] or [LandscapeDiff].
    pub struct LandData: u32 {
        const VERTEX_COLORS = 0b10;
        const TEXTURES = 0b100;
        const VERTEX_HEIGHTS = 0b1000;
        const VERTEX_NORMALS = 0b10000;
        const WORLD_MAP = 0b100000;
    }
}

impl From<LandscapeFlags> for LandData {
    fn from(old: LandscapeFlags) -> Self {
        let mut new = LandData::default();

        if old.contains(LandscapeFlags::USES_VERTEX_HEIGHTS_AND_NORMALS) {
            new |= LandData::VERTEX_HEIGHTS;
            new |= LandData::VERTEX_NORMALS;
        }

        if old.contains(LandscapeFlags::USES_VERTEX_COLORS) {
            new |= LandData::VERTEX_COLORS;
        }

        if old.contains(LandscapeFlags::USES_TEXTURES) {
            new |= LandData::TEXTURES;
        }

        if old.uses_world_map_data() {
            new |= LandData::WORLD_MAP;
        }

        new
    }
}
