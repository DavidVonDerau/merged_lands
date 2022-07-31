use bitflags::bitflags;
use tes3::esp::LandscapeFlags;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
#[repr(C)]
pub struct Vec2<T> {
    pub x: T,
    pub y: T,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Default, Hash)]
#[repr(C)]
pub struct Vec3<T> {
    pub x: T,
    pub y: T,
    pub z: T,
}

pub type TerrainMap<U, const T: usize> = [[U; T]; T];

bitflags! {
    #[derive(Default)]
    pub struct LandData: u32 {
        const VCLR = 0b10;
        const VTEX = 0b100;
        const VNML = 0b1000;
        const VHGT = 0b10000;
        const WNAM = 0b100000;
    }
}

impl From<LandscapeFlags> for LandData {
    fn from(old: LandscapeFlags) -> Self {
        let mut new = LandData::default();

        if old.contains(LandscapeFlags::USES_VERTEX_HEIGHTS_AND_NORMALS) {
            new |= LandData::VNML;
            new |= LandData::VHGT;
        }

        if old.contains(LandscapeFlags::USES_VERTEX_COLORS) {
            new |= LandData::VCLR;
        }

        if old.contains(LandscapeFlags::USES_TEXTURES) {
            new |= LandData::VTEX;
        }

        if old.intersects(LandscapeFlags::USES_WORLD_MAP_DATA) {
            new |= LandData::WNAM;
        }

        new
    }
}
