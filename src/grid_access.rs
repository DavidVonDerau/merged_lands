use crate::TerrainMap;
use image::{ImageBuffer, Pixel};
use std::default::default;
use std::ops::{Deref, DerefMut};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct GridPosition2D {
    pub x: usize,
    pub y: usize,
}

#[derive(Default)]
pub struct GridIterator2D<const X: usize, const Y: usize> {
    coords: GridPosition2D,
}

impl<const X: usize, const Y: usize> Iterator for GridIterator2D<X, Y> {
    type Item = GridPosition2D;

    fn next(&mut self) -> Option<GridPosition2D> {
        if self.coords.y == Y {
            None
        } else {
            let result = Some(self.coords);

            self.coords.x += 1;
            if self.coords.x == X && self.coords.y < Y {
                self.coords.x = 0;
                self.coords.y += 1;
            }

            result
        }
    }
}

pub trait SquareGridIterator<const T: usize> {
    fn iter_grid(&self) -> GridIterator2D<T, T>;
}

impl<U, const T: usize> SquareGridIterator<T> for TerrainMap<U, T> {
    fn iter_grid(&self) -> GridIterator2D<T, T> {
        default()
    }
}

pub trait GridAccessor2D<U> {
    fn get(&self, coords: GridPosition2D) -> U;

    fn get_mut(&mut self, coords: GridPosition2D) -> &mut U;
}

impl<U: Copy, const T: usize> GridAccessor2D<U> for TerrainMap<U, T> {
    fn get(&self, coords: GridPosition2D) -> U {
        self[coords.y][coords.x]
    }

    fn get_mut(&mut self, coords: GridPosition2D) -> &mut U {
        &mut self[coords.y][coords.x]
    }
}

impl<P, Container> GridAccessor2D<P> for ImageBuffer<P, Container>
where
    P: Pixel,
    Container: Deref<Target = [P::Subpixel]> + DerefMut<Target = [P::Subpixel]>,
{
    fn get(&self, coords: GridPosition2D) -> P {
        *self.get_pixel(coords.x as u32, coords.y as u32)
    }

    fn get_mut(&mut self, coords: GridPosition2D) -> &mut P {
        self.get_pixel_mut(coords.x as u32, coords.y as u32)
    }
}
