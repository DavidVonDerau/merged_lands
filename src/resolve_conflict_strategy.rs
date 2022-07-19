use crate::grid_access::{GridAccessor2D, SquareGridIterator};
use crate::{
    ConflictResolver, ConflictType, MergeStrategy, RelativeTerrainMap, RelativeTo, SaveToImage,
    Vec2,
};
use image::imageops::FilterType;
use image::{DynamicImage, ImageBuffer, Rgb};
use std::default::default;

#[derive(Default)]
pub struct ResolveConflictStrategy {}

impl MergeStrategy for ResolveConflictStrategy {
    fn apply<U: RelativeTo, const T: usize>(
        &self,
        coords: Vec2<i32>,
        plugin: &str,
        value: &str,
        lhs: &RelativeTerrainMap<U, T>,
        rhs: &RelativeTerrainMap<U, T>,
    ) -> RelativeTerrainMap<U, T>
    where
        RelativeTerrainMap<U, T>: SaveToImage,
    {
        let mut diff_img = ImageBuffer::new(T as u32, T as u32);

        let mut relative = [[default(); T]; T];
        let mut has_difference = [[false; T]; T];

        let mut num_major_conflicts = 0;
        let mut num_minor_conflicts = 0;

        let params = default();

        for coords in relative.iter_grid() {
            let lhs_diff = lhs.has_difference.get(coords);
            let rhs_diff = rhs.has_difference.get(coords);

            let mut diff = default();
            if lhs_diff && !rhs_diff {
                diff = lhs.relative.get(coords);
                assert_ne!(diff, default());
                *diff_img.get_mut(coords) = Rgb::from([0, 0, 255u8]);
            } else if !lhs_diff && rhs_diff {
                diff = rhs.relative.get(coords);
                assert_ne!(diff, default());
                *diff_img.get_mut(coords) = Rgb::from([0, 255u8, 0]);
            } else if !lhs_diff && !rhs_diff {
                // NOP.
                *diff_img.get_mut(coords) = Rgb::from([0, 0, 0]);
            } else {
                let lhs_diff = lhs.relative.get(coords);
                assert_ne!(lhs_diff, default());

                let rhs_diff = rhs.relative.get(coords);
                assert_ne!(rhs_diff, default());

                match lhs_diff.average(rhs_diff, &params) {
                    None => {
                        diff = lhs.relative.get(coords);
                        *diff_img.get_mut(coords) = Rgb::from([0, 0, 255u8]);
                    }
                    Some(ConflictType::Minor(value)) => {
                        diff = value;
                        num_minor_conflicts += 1;
                        *diff_img.get_mut(coords) = Rgb::from([255u8, 255u8, 0]);
                    }
                    Some(ConflictType::Major(value)) => {
                        diff = value;
                        num_major_conflicts += 1;
                        *diff_img.get_mut(coords) = Rgb::from([255u8, 0, 0]);
                    }
                }
            }

            *relative.get_mut(coords) = diff;
            *has_difference.get_mut(coords) = diff != default();
        }

        if num_minor_conflicts > 0 || num_major_conflicts > 0 {
            println!(
                "\t\t{} Major Conflicts ({} Minor Conflicts)",
                num_major_conflicts, num_minor_conflicts
            );

            {
                let img_name = format!(
                    "Maps/{}_{}_{}_DIFF_{}_{}_{}.png",
                    value, coords.x, coords.y, plugin, num_minor_conflicts, num_major_conflicts
                );

                let img = DynamicImage::from(diff_img);
                img.resize_exact((T * 4) as u32, (T * 4) as u32, FilterType::Nearest)
                    .save(img_name)
                    .unwrap();
            }
        }

        let merged = RelativeTerrainMap {
            reference: lhs.reference,
            relative,
            has_difference,
        };

        {
            let img_name = format!(
                "Maps/{}_{}_{}_MERGED_{}_{}_{}.png",
                value, coords.x, coords.y, plugin, num_minor_conflicts, num_major_conflicts
            );
            merged.save_to_image(&img_name);
        }

        merged
    }
}
