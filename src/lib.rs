//! This crate provides a jagged array, i.e. a type that is semantically equivalent to
//! `Box<[Box<[T]>]>`, but implemented with better memory locality and fewer heap allocations.

use std::iter::FromIterator;
use std::mem;
use std::ops::Index;
use std::slice;

pub struct Jagged2<T> {
    /// Indicates where each row begins in memory.
    /// Note that onsets[0] points to the beginning of the underlying storage,
    /// which needs to be manually freed on drop,
    /// and onsets[len-1] points to the end of storage.
    /// Because of this, onsets.len() == num_rows + 1
    onsets: Box<[*mut T]>,
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

impl<T> Drop for Jagged2<T> {
    fn drop(&mut self) {
        unsafe {
            // Need to explicitly free memory on drop.
            // onsets[0] points to the base address of the storage;
            // dropping a Box created from that address will trigger deallocation.
            Box::from_raw(self.onsets[0]);
        }
    }
}

impl<T, ICol> FromIterator<ICol> for Jagged2<T>
    where ICol: IntoIterator<Item=T>
{
    /// Allow construction from any type that behaves like `[[T]]`.
    fn from_iter<IRow>(row_iter: IRow) -> Self
        where IRow: IntoIterator<Item=ICol>
    {
        let row_iter = row_iter.into_iter();
        // Collect the iterator into a flat vector,
        // and for each row, write the index into the flat vector at which it starts.
        let mut storage = Vec::new();
        let mut onsets = Vec::with_capacity(row_iter.size_hint().0);
        for col_iter in row_iter {
            onsets.push(storage.len());
            storage.extend(col_iter);
        }
        let len = storage.len();

        // Transform the onsets into an array that holds the *address* of each row.
        let storage = Box::into_raw(storage.into_boxed_slice()) as *mut T;
        let onsets = onsets.into_iter().chain(Some(len).into_iter())
            .map(|idx| unsafe {
                storage.offset(idx as isize)
            })
            .collect::<Vec<_>>().into_boxed_slice();
        // Now data can be accessed via `onsets[row][column]`
        Self{ onsets }
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
        let row_end = match self.onsets.get(1+index.0) {
            Some(addr) => *addr,
            None => { return None }
        };
        // Because onsets[1+index.0] exists, it's safe to directly access
        // onsets[index.0]
        let row_onset = *self.onsets.get(index.0).unwrap();
        // TODO: Can T be size 0? If so this errors.
        let row_len = (row_end as usize - row_onset as usize)/mem::size_of::<T>();

        unsafe {
            // Let the slice do the bounds checking on the column access for us.
            let row_slice = slice::from_raw_parts(row_onset, row_len);
            row_slice.get(index.1)
        }

    }
}
