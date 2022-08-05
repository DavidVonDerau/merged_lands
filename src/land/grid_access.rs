#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct Index2D {
    pub x: usize,
    pub y: usize,
}

impl Index2D {
    pub fn new(x: usize, y: usize) -> Self {
        Self { x, y }
    }
}

#[derive(Default)]
pub struct GridIterator2D<const X: usize, const Y: usize> {
    coords: Index2D,
}

impl<const X: usize, const Y: usize> Iterator for GridIterator2D<X, Y> {
    type Item = Index2D;

    fn next(&mut self) -> Option<Index2D> {
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

pub trait GridAccessor2D<U> {
    fn get(&self, coords: Index2D) -> U;

    fn get_mut(&mut self, coords: Index2D) -> &mut U;
}
