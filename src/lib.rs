//! This crate provides a jagged array, i.e. a type that is semantically equivalent to
//! `Box<[Box<[T]>]>`, but implemented with better memory locality and fewer heap allocations.

use std::ops::Index;

pub struct Jagged2<T> {
    /// Holds all the elements of the 2d array, but contiguously
    storage: Box<[T]>,
    /// Indicates where each row begins inside the storage
    onsets: Box<[usize]>,
}

impl<T> Index<(usize, usize)> for Jagged2<T> {
    type Output = T;
    /// Index into the jagged array. The index is given in (Major, Minor) form,
    /// i.e. (row, column) or (outer, inner).
    /// `array[(0, 0)]` is adjacent to `array[(0, 1)]` in memory but not
    /// necessarily to `array[(1, 0)]`.
    fn index(&self, index: (usize, usize)) -> &T {
        self.get(index).unwrap()
    }
}

impl<T> Jagged2<T> {
    /// Index into the jagged array. The index is given in (Major, Minor) form,
    /// i.e. (row, column) or (outer, inner).
    /// `array[(0, 0)]` is adjacent to `array[(0, 1)]` in memory but not
    /// necessarily to `array[(1, 0)]`.
    pub fn get(&self, index: (usize, usize)) -> Option<&T> {
        // Figure out which indices in the storage correspond to the
        // start and end of the selected row.
        let row_onset = match self.onsets.get(index.0) {
            Some(idx) => idx,
            None => { return None; }
        };
        let row_end = self.onsets.get(index.0+1).cloned().unwrap_or_else(|| self.storage.len());

        // Physical index into the storage
        let raw_index = row_onset + index.1;
        if raw_index < row_end {
            Some(&self.storage[raw_index])
        } else {
            None
        }
    }
    pub fn from_iter<IRow, ICol>(row_iter: IRow) -> Self
        where IRow: Iterator<Item=ICol>, ICol: Iterator<Item=T>
    {
        let mut storage = Vec::new();
        let mut onsets = Vec::with_capacity(row_iter.size_hint().0);
        for col_iter in row_iter {
            onsets.push(storage.len());
            storage.extend(col_iter);
        }
        // By turning this into a slice, we save `sizeof(usize)` bytes on struct
        // by not having to store the Vec's capacity.
        // We also potentially release some heap memory, though this might
        // reallocate.
        let storage = storage.into_boxed_slice();
        let onsets = onsets.into_boxed_slice();
        Self{ storage, onsets }
    }
}
