#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
/// An index on some 2D grid.
pub struct Index2D {
    pub x: usize,
    pub y: usize,
}

impl Index2D {
    /// Returns a new [Index2D] with coordinates `x` and `y`.
    pub fn new(x: usize, y: usize) -> Self {
        Self { x, y }
    }
}

#[derive(Default)]
/// An [Iterator] over some 2D grid.
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

/// Types implementing [SquareGridIterator] support a method [SquareGridIterator::iter_grid].
pub trait SquareGridIterator<const T: usize> {
    /// Returns a [GridIterator2D] that will visit each coordinate in the grid.
    /// The order of iteration is x-axis first, then y-axis.
    fn iter_grid(&self) -> GridIterator2D<T, T>;
}

/// Types implementing [GridAccessor2D] can be indexed by [Index2D] `coords`.
pub trait GridAccessor2D<U> {
    /// Get the value at `coords`.
    fn get(&self, coords: Index2D) -> U;

    /// Get a mutable reference to the value at `coords`.
    fn get_mut(&mut self, coords: Index2D) -> &mut U;
}
