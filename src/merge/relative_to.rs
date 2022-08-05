use crate::land::terrain_map::Vec3;
use std::fmt::Debug;

pub trait RelativeTo: Copy + Default + Eq + Debug + Sized + 'static {
    type Delta: Copy + Default + Eq + Debug + Sized + 'static;

    fn subtract(lhs: Self, rhs: Self) -> Self::Delta;

    fn add(lhs: Self, rhs: Self::Delta) -> Self;
}

impl RelativeTo for i32 {
    type Delta = i32;

    fn subtract(lhs: Self, rhs: Self) -> Self::Delta {
        (lhs as Self::Delta) - (rhs as Self::Delta)
    }

    fn add(lhs: Self, rhs: Self::Delta) -> Self {
        ((lhs as Self::Delta) + rhs) as Self
    }
}

impl RelativeTo for u8 {
    type Delta = i32;

    fn subtract(lhs: Self, rhs: Self) -> Self::Delta {
        (lhs as Self::Delta) - (rhs as Self::Delta)
    }

    fn add(lhs: Self, rhs: Self::Delta) -> Self {
        ((lhs as Self::Delta) + rhs) as Self
    }
}

impl RelativeTo for i8 {
    type Delta = i32;

    fn subtract(lhs: Self, rhs: Self) -> Self::Delta {
        (lhs as Self::Delta) - (rhs as Self::Delta)
    }

    fn add(lhs: Self, rhs: Self::Delta) -> Self {
        ((lhs as Self::Delta) + rhs) as Self
    }
}

impl RelativeTo for u16 {
    type Delta = i32;

    fn subtract(lhs: Self, rhs: Self) -> Self::Delta {
        (lhs as Self::Delta) - (rhs as Self::Delta)
    }

    fn add(lhs: Self, rhs: Self::Delta) -> Self {
        ((lhs as Self::Delta) + rhs) as Self
    }
}

impl<T: RelativeTo> RelativeTo for Vec3<T> {
    type Delta = Vec3<<T as RelativeTo>::Delta>;

    fn subtract(lhs: Self, rhs: Self) -> Self::Delta {
        Self::Delta {
            x: <T as RelativeTo>::subtract(lhs.x, rhs.x),
            y: <T as RelativeTo>::subtract(lhs.y, rhs.y),
            z: <T as RelativeTo>::subtract(lhs.z, rhs.z),
        }
    }

    fn add(lhs: Self, rhs: Self::Delta) -> Self {
        Self {
            x: <T as RelativeTo>::add(lhs.x, rhs.x),
            y: <T as RelativeTo>::add(lhs.y, rhs.y),
            z: <T as RelativeTo>::add(lhs.z, rhs.z),
        }
    }
}
