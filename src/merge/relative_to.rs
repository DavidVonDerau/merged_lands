use crate::land::terrain_map::Vec3;
use const_default::ConstDefault;
use std::fmt::Debug;

/// Types implementing [RelativeTo] can be subtracted with [RelativeTo::subtract] to compute
/// some delta of type [RelativeTo::Delta]. The delta can be passed to [RelativeTo::add] to
/// recompute the original value.
pub trait RelativeTo: Copy + Default + ConstDefault + Eq + Debug + Sized + 'static {
    /// A [RelativeTo::Delta] is a signed version of the type implementing [RelativeTo].
    type Delta: Copy + Default + ConstDefault + Eq + Debug + Sized + 'static;

    /// Subtract `rhs` from `lhs` and return the [RelativeTo::Delta].
    fn subtract(lhs: Self, rhs: Self) -> Self::Delta;

    /// Add the [RelativeTo::Delta] `rhs` to `lhs`.
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
