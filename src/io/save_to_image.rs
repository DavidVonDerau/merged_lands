use crate::land::grid_access::{GridAccessor2D, Index2D, SquareGridIterator};
use crate::land::landscape_diff::LandscapeDiff;
use crate::land::terrain_map::{Vec2, Vec3};
use crate::merge::conflict::{ConflictResolver, ConflictType};
use crate::merge::relative_terrain_map::RelativeTerrainMap;
use crate::merge::relative_to::RelativeTo;
use crate::LandmassDiff;
use image::imageops::FilterType;
use image::{DynamicImage, ImageBuffer, Luma, Pixel, Rgb};
use log::{error, trace};
use std::default::default;
use std::ops::{Deref, DerefMut};

const DEFAULT_SCALE_FACTOR: usize = 4;

fn save_resized_image<const T: usize, I>(img: I, file_name: &str, scale_factor: usize)
where
    DynamicImage: From<I>,
{
    assert!(scale_factor > 0, "scale_factor must be > 0");
    match DynamicImage::from(img)
        .resize_exact(
            (T * scale_factor) as u32,
            (T * scale_factor) as u32,
            FilterType::Nearest,
        )
        .save(&file_name)
    {
        Ok(_) => {}
        Err(e) => {
            error!("Unable to save file {} due to: {}", file_name, e)
        }
    };
}

impl<P, Container> GridAccessor2D<P> for ImageBuffer<P, Container>
where
    P: Pixel,
    Container: Deref<Target = [P::Subpixel]> + DerefMut<Target = [P::Subpixel]>,
{
    fn get(&self, coords: Index2D) -> P {
        *self.get_pixel(coords.x as u32, coords.y as u32)
    }

    fn get_mut(&mut self, coords: Index2D) -> &mut P {
        self.get_pixel_mut(coords.x as u32, coords.y as u32)
    }
}

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

        for coords in self.iter_grid() {
            let new = self.get_value(coords);
            *img.get_mut(coords) = Rgb::from([new.x, new.y, new.z]);
        }

        save_resized_image::<T, _>(img, file_name, DEFAULT_SCALE_FACTOR);
    }
}

fn calculate_min_max<U: RelativeTo, const T: usize>(map: &RelativeTerrainMap<U, T>) -> (f32, f32)
where
    f64: From<U>,
{
    let mut min_value = f32::MAX;
    let mut max_value = f32::MIN;

    for coords in map.iter_grid() {
        let value = map.get_value(coords);
        min_value = min_value.min(f64::from(value) as f32);
        max_value = max_value.max(f64::from(value) as f32);
    }

    (min_value, max_value)
}

impl<const T: usize> SaveToImage for RelativeTerrainMap<u8, T> {
    fn save_to_image(&self, file_name: &str) {
        let mut img = ImageBuffer::new(T as u32, T as u32);

        let (min_value, max_value) = calculate_min_max(self);

        for coords in self.iter_grid() {
            let value = self.get_value(coords) as f32;
            let scaled = (value - min_value) as f32 / (max_value - min_value);
            *img.get_mut(coords) = Luma::from([(scaled * 255.) as u8]);
        }

        save_resized_image::<T, _>(img, file_name, DEFAULT_SCALE_FACTOR);
    }
}

impl<const T: usize> SaveToImage for RelativeTerrainMap<i32, T> {
    fn save_to_image(&self, file_name: &str) {
        let mut img = ImageBuffer::new(T as u32, T as u32);

        let (min_value, max_value) = calculate_min_max(self);

        for coords in self.iter_grid() {
            let value = self.get_value(coords) as f32;
            let scaled = (value - min_value) as f32 / (max_value - min_value);
            let as_u8 = (scaled * 255.) as u8;
            if self.has_difference(coords) {
                *img.get_mut(coords) = Rgb::from([
                    (as_u8 as f32 * 0.98) as u8,
                    (as_u8 as f32 * 1.04) as u8,
                    (as_u8 as f32 * 0.98) as u8,
                ]);
            } else {
                *img.get_mut(coords) = Rgb::from([as_u8, as_u8, as_u8]);
            }
        }

        save_resized_image::<T, _>(img, file_name, DEFAULT_SCALE_FACTOR);
    }
}

pub fn save_image<U: RelativeTo + ConflictResolver, const T: usize>(
    coords: Vec2<i32>,
    plugin: &str,
    value: &str,
    lhs: Option<&RelativeTerrainMap<U, T>>,
    rhs: Option<&RelativeTerrainMap<U, T>>,
) where
    RelativeTerrainMap<U, T>: SaveToImage,
{
    let Some(lhs) = lhs else {
        return;
    };

    let Some(rhs) = rhs else {
        return;
    };

    let mut diff_img = ImageBuffer::new(T as u32, T as u32);

    let mut num_major_conflicts = 0;
    let mut num_minor_conflicts = 0;

    let params = default();

    for coords in lhs.iter_grid() {
        let actual = lhs.get_value(coords);
        let expected = rhs.get_value(coords);
        let has_difference = rhs.has_difference(coords);

        match actual.average(expected, &params) {
            None => {
                let color = if has_difference {
                    Rgb::from([0, 255u8, 0])
                } else {
                    Rgb::from([0, 0, 0])
                };

                *diff_img.get_mut(coords) = color;
            }
            Some(ConflictType::Minor(_)) => {
                let color = if has_difference {
                    num_minor_conflicts += 1;
                    Rgb::from([255u8, 255u8, 0])
                } else {
                    Rgb::from([0, 0, 0])
                };

                *diff_img.get_mut(coords) = color;
            }
            Some(ConflictType::Major(_)) => {
                let color = if has_difference {
                    num_major_conflicts += 1;
                    Rgb::from([255u8, 0, 0])
                } else {
                    Rgb::from([0, 0, 0])
                };

                *diff_img.get_mut(coords) = color;
            }
        }
    }

    if num_minor_conflicts == 0 && num_major_conflicts == 0 {
        return;
    }

    // TODO(dvd): Read thresholds from config.
    let minor_conflict_threshold = (T * T) as f32 * 0.01;
    let major_conflict_threshold = (T * T) as f32 * 0.001;

    let mut should_skip = num_minor_conflicts < minor_conflict_threshold as usize
        && num_major_conflicts < major_conflict_threshold as usize;

    // TODO(dvd): Configure this too.
    if value == "vertex_colors" || value == "vertex_normals" {
        should_skip = true;
    }

    trace!(
        "({:>4}, {:>4}) {:<15} | {:<50} | {:>4} Major | {:>4} Minor{}",
        coords.x,
        coords.y,
        value,
        plugin,
        num_major_conflicts,
        num_minor_conflicts,
        if should_skip { "" } else { " *" }
    );

    if should_skip {
        return;
    }

    {
        let file_name = format!(
            "Maps/{}_{}_{}_DIFF_{}.png",
            value, coords.x, coords.y, plugin,
        );

        save_resized_image::<T, _>(diff_img, &file_name, DEFAULT_SCALE_FACTOR);
    }

    {
        let img_name = format!("Maps/{}_{}_{}_MERGED.png", value, coords.x, coords.y);
        lhs.save_to_image(&img_name);
    }
}

fn save_landscape_images(plugin_name: &str, reference: &LandscapeDiff, plugin: &LandscapeDiff) {
    save_image(
        reference.coords,
        plugin_name,
        "height_map",
        reference.height_map.as_ref(),
        plugin.height_map.as_ref(),
    );
    save_image(
        reference.coords,
        plugin_name,
        "vertex_normals",
        reference.vertex_normals.as_ref(),
        plugin.vertex_normals.as_ref(),
    );
    save_image(
        reference.coords,
        plugin_name,
        "world_map_data",
        reference.world_map_data.as_ref(),
        plugin.world_map_data.as_ref(),
    );
    save_image(
        reference.coords,
        plugin_name,
        "vertex_colors",
        reference.vertex_colors.as_ref(),
        plugin.vertex_colors.as_ref(),
    );
}

pub fn save_landmass_images(merged: &mut LandmassDiff, plugin: &LandmassDiff) {
    for (coords, land) in plugin.iter_land() {
        let merged_land = merged.land.get(coords).expect("safe");
        save_landscape_images(&plugin.plugin, merged_land, land);
    }
}
