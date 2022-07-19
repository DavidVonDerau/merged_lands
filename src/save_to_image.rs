use crate::grid_access::{GridAccessor2D, SquareGridIterator};
use crate::{RelativeTerrainMap, RelativeTo, Vec3};
use image::imageops::FilterType;
use image::{DynamicImage, ImageBuffer, Luma, Rgb};

pub trait SaveToImage {
    fn save_to_image(&self, file_name: &str);
}

impl<const T: usize> SaveToImage for RelativeTerrainMap<Vec3<i8>, T> {
    fn save_to_image(&self, _file_name: &str) {
        // Ignore
    }
}

impl<const T: usize> SaveToImage for RelativeTerrainMap<u16, T> {
    fn save_to_image(&self, _file_name: &str) {
        // Ignore
    }
}

impl<const T: usize> SaveToImage for RelativeTerrainMap<Vec3<u8>, T> {
    fn save_to_image(&self, file_name: &str) {
        let mut img = ImageBuffer::new(T as u32, T as u32);

        for coords in self.reference.iter_grid() {
            let new = <Vec3<u8> as RelativeTo>::add(
                self.reference.get(coords),
                self.relative.get(coords),
            );
            *img.get_mut(coords) = Rgb::from([new.x, new.y, new.z]);
        }

        let img = DynamicImage::from(img);
        img.resize_exact((T * 4) as u32, (T * 4) as u32, FilterType::Nearest)
            .save(file_name)
            .unwrap();
    }
}

fn calculate_min_max<U: RelativeTo, const T: usize>(map: &RelativeTerrainMap<U, T>) -> (f32, f32)
where
    f64: From<U>,
{
    let mut min_value = f32::MAX;
    let mut max_value = f32::MIN;

    for coords in map.reference.iter_grid() {
        let value = <U as RelativeTo>::add(map.reference.get(coords), map.relative.get(coords));
        min_value = min_value.min(f64::from(value) as f32);
        max_value = max_value.max(f64::from(value) as f32);
    }

    (min_value, max_value)
}

impl<const T: usize> SaveToImage for RelativeTerrainMap<u8, T> {
    fn save_to_image(&self, file_name: &str) {
        let mut img = ImageBuffer::new(T as u32, T as u32);

        let (min_value, max_value) = calculate_min_max(self);

        for coords in self.reference.iter_grid() {
            let value =
                <u8 as RelativeTo>::add(self.reference.get(coords), self.relative.get(coords))
                    as f32;
            let scaled = (value - min_value) as f32 / (max_value - min_value);
            *img.get_mut(coords) = Luma::from([(scaled * 255.) as u8]);
        }

        let img = DynamicImage::from(img);
        img.resize_exact((T * 4) as u32, (T * 4) as u32, FilterType::Nearest)
            .save(file_name)
            .unwrap();
    }
}

impl<const T: usize> SaveToImage for RelativeTerrainMap<i32, T> {
    fn save_to_image(&self, file_name: &str) {
        let mut img = ImageBuffer::new(T as u32, T as u32);

        let (min_value, max_value) = calculate_min_max(self);

        for coords in self.reference.iter_grid() {
            let value =
                <i32 as RelativeTo>::add(self.reference.get(coords), self.relative.get(coords))
                    as f32;
            let scaled = (value - min_value) as f32 / (max_value - min_value);
            let as_u8 = (scaled * 255.) as u8;
            if self.has_difference.get(coords) {
                *img.get_mut(coords) = Rgb::from([
                    (as_u8 as f32 * 0.98) as u8,
                    (as_u8 as f32 * 1.04) as u8,
                    (as_u8 as f32 * 0.98) as u8,
                ]);
            } else {
                *img.get_mut(coords) = Rgb::from([as_u8, as_u8, as_u8]);
            }
        }

        let img = DynamicImage::from(img);
        img.resize_exact((T * 4) as u32, (T * 4) as u32, FilterType::Nearest)
            .save(file_name)
            .unwrap();
    }
}
