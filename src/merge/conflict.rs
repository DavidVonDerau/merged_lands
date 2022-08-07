use crate::land::terrain_map::Vec3;
use crate::merge::round_to::RoundTo;

/// The [ConflictType] classifies the severity of a conflict.
/// This is determined by [ConflictParams] passed to the
/// [ConflictResolver::average] method.
pub enum ConflictType<T> {
    /// A minor [ConflictType].
    Minor(T),
    /// A major [ConflictType].
    Major(T),
}

/// A [Conflict] is an [Option] wrapper around [ConflictType].
pub type Conflict<T> = Option<ConflictType<T>>;

/// Types implementing [ConflictResolver] support the method [ConflictResolver::average].
pub trait ConflictResolver: Sized {
    /// Attempt to merge `self` with `rhs` per [ConflictParams] and return the [Conflict].
    /// [None] is returned when `self == rhs`.
    fn average(self, rhs: Self, params: &ConflictParams) -> Conflict<Self>;
}

/// Controls the classification of a [Conflict] into [ConflictType::Minor] or [ConflictType::Major].
pub struct ConflictParams {
    minor_threshold_pct: f32,
    minor_threshold_min: f32,
    minor_threshold_max: f32,
}

impl Default for ConflictParams {
    /// The default [ConflictParams] are chosen to minimize
    /// the likelihood that a [ConflictType::Minor] is noticeable.
    fn default() -> Self {
        Self {
            minor_threshold_pct: 0.3,
            minor_threshold_min: 10.0,
            minor_threshold_max: 64.0,
        }
    }
}

/// Returns [ConflictType] for `lhs` and `rhs` per [ConflictParams].
fn classify_conflict<U>(lhs: f32, rhs: f32, params: &ConflictParams) -> ConflictType<U>
where
    f32: RoundTo<U>,
{
    let lhs_weight = (lhs.abs() as f32) / ((lhs.abs() as f32) + (rhs.abs() as f32));
    let rhs_weight = 1. - lhs_weight;
    let lhs_weight_2 = lhs_weight.powf(1.5);
    let rhs_weight_2 = rhs_weight.powf(1.5);
    let lhs_weight = lhs_weight_2 / (lhs_weight_2 + rhs_weight_2);
    let rhs_weight = 1. - lhs_weight;
    let average = lhs_weight * (lhs as f32) + rhs_weight * (rhs as f32);
    let minimum = lhs.min(rhs) as f32;
    let proportional_threshold =
        (params.minor_threshold_pct * minimum as f32).max(params.minor_threshold_min);
    let difference = f32::abs(minimum - average);
    if difference >= proportional_threshold.min(params.minor_threshold_max) {
        ConflictType::Major(average.round_to())
    } else {
        ConflictType::Minor(average.round_to())
    }
}

impl<T: Eq + Into<f64>> ConflictResolver for T
where
    f32: RoundTo<T>,
{
    fn average(self, rhs: Self, params: &ConflictParams) -> Conflict<Self> {
        if self == rhs {
            None
        } else {
            Some(classify_conflict(
                self.into() as f32,
                rhs.into() as f32,
                params,
            ))
        }
    }
}

impl<T> ConflictResolver for Vec3<T>
where
    T: Eq + PartialEq + ConflictResolver + Copy,
{
    fn average(self, rhs: Self, params: &ConflictParams) -> Conflict<Self> {
        if self == rhs {
            None
        } else {
            let mut num_major_conflicts = 0;

            let x = match self.x.average(rhs.x, params) {
                None => self.x,
                Some(ConflictType::Minor(x)) => x,
                Some(ConflictType::Major(x)) => {
                    num_major_conflicts += 1;
                    x
                }
            };

            let y = match self.y.average(rhs.y, params) {
                None => self.y,
                Some(ConflictType::Minor(y)) => y,
                Some(ConflictType::Major(y)) => {
                    num_major_conflicts += 1;
                    y
                }
            };

            let z = match self.z.average(rhs.z, params) {
                None => self.z,
                Some(ConflictType::Minor(z)) => z,
                Some(ConflictType::Major(z)) => {
                    num_major_conflicts += 1;
                    z
                }
            };

            if num_major_conflicts > 0 {
                Some(ConflictType::Major(Self { x, y, z }))
            } else {
                Some(ConflictType::Minor(Self { x, y, z }))
            }
        }
    }
}
