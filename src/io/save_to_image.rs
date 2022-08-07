use crate::io::parsed_plugins::ParsedPlugin;
use crate::land::grid_access::{GridAccessor2D, Index2D, SquareGridIterator};
use crate::land::landscape_diff::LandscapeDiff;
use crate::land::terrain_map::{Vec2, Vec3};
use crate::merge::conflict::{ConflictResolver, ConflictType};
use crate::merge::relative_terrain_map::RelativeTerrainMap;
use crate::merge::relative_to::RelativeTo;
use crate::LandmassDiff;
use anyhow::{anyhow, Context, Result};
use image::imageops::FilterType;
use image::{DynamicImage, ImageBuffer, Luma, Pixel, Rgb};
use log::{error, trace, warn};
use owo_colors::OwoColorize;
use std::default::default;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};

const DEFAULT_SCALE_FACTOR: usize = 4;

/// Saves `img` to `file_name` after resizing by `scale_factor`.
fn save_resized_image<const T: usize, I>(
    img: I,
    file_path: &Path,
    scale_factor: usize,
) -> Result<()>
where
    DynamicImage: From<I>,
{
    let directory = file_path.parent().expect("safe");

    let exists = directory
        .try_exists()
        .with_context(|| anyhow!("Unable to find `{}` directory", directory.to_string_lossy()))?;

    if !exists {
        warn!(
            "{} {}",
            format!(
                "Unable to save image file {}",
                file_path.to_string_lossy().bold()
            )
            .yellow(),
            format!(
                "because the `{}` directory does not exist",
                directory.to_string_lossy()
            )
            .yellow()
        );

        return Ok(());
    }

    assert!(scale_factor > 0, "scale_factor must be > 0");
    DynamicImage::from(img)
        .resize_exact(
            (T * scale_factor) as u32,
            (T * scale_factor) as u32,
            FilterType::Nearest,
        )
        .save(file_path)
        .with_context(|| anyhow!("Unable to save image file {}", file_path.to_string_lossy()))?;

    Ok(())
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

/// Types implementing [SaveToImage] support a method [SaveToImage::save_to_image].
pub trait SaveToImage {
    /// Save an image to `file_name`.
    fn save_to_image(&self, file_path: &Path);
}

impl<const T: usize> SaveToImage for RelativeTerrainMap<Vec3<i8>, T> {
    fn save_to_image(&self, _file_path: &Path) {
        // Ignore
    }
}

impl<const T: usize> SaveToImage for RelativeTerrainMap<u16, T> {
    fn save_to_image(&self, _file_path: &Path) {
        // Ignore
    }
}

impl<const T: usize> SaveToImage for RelativeTerrainMap<Vec3<u8>, T> {
    fn save_to_image(&self, file_path: &Path) {
        let mut img = ImageBuffer::new(T as u32, T as u32);

        for coords in self.iter_grid() {
            let new = self.get_value(coords);
            *img.get_mut(coords) = Rgb::from([new.x, new.y, new.z]);
        }

        save_resized_image::<T, _>(img, file_path, DEFAULT_SCALE_FACTOR)
            .map_err(|e| error!("{}", e.bold().bright_red()))
            .ok();
    }
}

/// Calculates the min and max values of the [RelativeTerrainMap].
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
    fn save_to_image(&self, file_path: &Path) {
        let mut img = ImageBuffer::new(T as u32, T as u32);

        let (min_value, max_value) = calculate_min_max(self);

        for coords in self.iter_grid() {
            let value = self.get_value(coords) as f32;
            let scaled = (value - min_value) as f32 / (max_value - min_value);
            *img.get_mut(coords) = Luma::from([(scaled * 255.) as u8]);
        }

        save_resized_image::<T, _>(img, file_path, DEFAULT_SCALE_FACTOR)
            .map_err(|e| error!("{}", e.bold().bright_red()))
            .ok();
    }
}

impl<const T: usize> SaveToImage for RelativeTerrainMap<i32, T> {
    fn save_to_image(&self, file_path: &Path) {
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

        save_resized_image::<T, _>(img, file_path, DEFAULT_SCALE_FACTOR)
            .map_err(|e| error!("{}", e.bold().bright_red()))
            .ok();
    }
}

/// Saves an image of the conflicts between the `lhs` [RelativeTerrainMap] and
/// the `rhs` [RelativeTerrainMap] if any exist.
pub fn save_image<U: RelativeTo + ConflictResolver, const T: usize>(
    merged_lands_dir: &Path,
    coords: Vec2<i32>,
    plugin: &ParsedPlugin,
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

        // TODO(dvd): #feature Use a gradient so that smaller conflicts can be seen.
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

    // TODO(dvd): #mvp Read thresholds from config.
    let minor_conflict_threshold = (T * T) as f32 * 0.02;
    let major_conflict_threshold = (T * T) as f32 * 0.001;

    let mut should_skip = num_minor_conflicts < minor_conflict_threshold as usize
        && num_major_conflicts < major_conflict_threshold as usize;

    // TODO(dvd): #mvp Configure this too.
    if value == "vertex_colors" || value == "vertex_normals" {
        should_skip = true;
    }

    trace!(
        "({:>4}, {:>4}) {:<15} | {:<50} | {:>4} Major | {:>4} Minor{}",
        coords.x,
        coords.y,
        value,
        plugin.name,
        num_major_conflicts,
        num_minor_conflicts,
        if should_skip {
            "".to_string()
        } else {
            " *".bold().bright_red().to_string()
        }
    );

    if should_skip {
        return;
    }

    {
        let file_name = format!(
            "{}_{}_{}_DIFF_{}.png",
            value, coords.x, coords.y, plugin.name,
        );

        let file_path: PathBuf = [
            merged_lands_dir,
            Path::new("Conflicts"),
            &PathBuf::from(file_name),
        ]
        .iter()
        .collect();

        save_resized_image::<T, _>(diff_img, &file_path, DEFAULT_SCALE_FACTOR)
            .map_err(|e| error!("{}", e.bold().bright_red()))
            .ok();
    }

    {
        let file_name = format!("{}_{}_{}_MERGED.png", value, coords.x, coords.y);
        let file_path: PathBuf = [
            merged_lands_dir,
            Path::new("Conflicts"),
            &PathBuf::from(file_name),
        ]
        .iter()
        .collect();
        lhs.save_to_image(&file_path);
    }
}

/// Saves images of conflicts between [LandscapeDiff] `reference` and `plugin`.
fn save_landscape_images(
    merged_lands_dir: &Path,
    parsed_plugin: &ParsedPlugin,
    reference: &LandscapeDiff,
    plugin: &LandscapeDiff,
) {
    save_image(
        merged_lands_dir,
        reference.coords,
        parsed_plugin,
        "height_map",
        reference.height_map.as_ref(),
        plugin.height_map.as_ref(),
    );
    save_image(
        merged_lands_dir,
        reference.coords,
        parsed_plugin,
        "vertex_normals",
        reference.vertex_normals.as_ref(),
        plugin.vertex_normals.as_ref(),
    );
    save_image(
        merged_lands_dir,
        reference.coords,
        parsed_plugin,
        "world_map_data",
        reference.world_map_data.as_ref(),
        plugin.world_map_data.as_ref(),
    );
    save_image(
        merged_lands_dir,
        reference.coords,
        parsed_plugin,
        "vertex_colors",
        reference.vertex_colors.as_ref(),
        plugin.vertex_colors.as_ref(),
    );
}

/// Saves images of conflicts between [LandmassDiff] `reference` and `plugin`.
pub fn save_landmass_images(
    merged_lands_dir: &Path,
    reference: &LandmassDiff,
    plugin: &LandmassDiff,
) {
    for (coords, land) in plugin.sorted() {
        let merged_land = reference.land.get(coords).expect("safe");
        save_landscape_images(merged_lands_dir, &plugin.plugin, merged_land, land);
    }
}
