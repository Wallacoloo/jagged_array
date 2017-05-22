//! This crate provides a jagged array, i.e. a type that is semantically equivalent to
//! `Box<[Box<[T]>]>`, but implemented with better memory locality and fewer heap allocations.

#[macro_use] extern crate try_opt;

use std::iter::FromIterator;
use std::ops::Index;
use std::slice;

/// 2-dimensional jagged array type. It's equivalent to a
/// `Box<Box<[mut T]>>`, but where all array data is stored contiguously
/// and fewer allocations are performed.
/// 
/// Note that no dimension of the array can be modified after creation.
/// **Jagged2 only supports positively-sized types** (i.e. empty structs or `()`
/// will cause a runtime error).
pub struct Jagged2<T> {
    /// Slices into the underlying storage, indexed by row.
    /// Minus bounds checking, data can be accessed essentially via
    /// `onsets[row].0[column]`.
    /// Row length is accessed by `onsets[row].1`.
    onsets: Box<[(*mut T, usize)]>,
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
            // dropping a Box created from a slice of all the storage will do that.
            Box::from_raw(self.as_flat_slice_mut() as *mut [T]);
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
        // Tranform all inputs into their iterators.
        // We need to collect into a vector so that we can get an accurate size
        // estimate of the overall storage BEFORE any more allocation.
        // Having an accurate size to use with Vec::with_capacity makes a substantial different
        // (can halve the time it takes to construct the jagged array).
        let row_iters: Vec<_> = row_iter.into_iter().map(|i| i.into_iter()).collect();
        let mut storage = Vec::with_capacity(row_iters.iter().map(|i| i.size_hint().0).sum());
        let mut onsets = Vec::with_capacity(row_iters.len());
        for col_iter in row_iters {
            // store the index of the row, transmuted to *mut T.
            // This transmutation is done in order to reuse this vector for
            // holding absolute row addresses, once the base address is finalized.
            let row_start = storage.len();
            storage.extend(col_iter);
            let row_end = storage.len();
            onsets.push((row_start as *mut T, row_end-row_start));
        }

        let storage = Box::into_raw(storage.into_boxed_slice()) as *mut T;
        // Transform the onsets from relative indices to absolute addresses.
        for onset in onsets.iter_mut() {
            unsafe {
                onset.0 = storage.offset(onset.0 as isize);
            }
        }
        let onsets = onsets.into_boxed_slice();
        // Now data can be accessed via `onsets[row][column]`
        Self{ onsets }
    }
}

impl<T> Jagged2<T> {
    /// Index into the jagged array. The index is given in (Major, Minor) form,
    /// i.e. (row, column) or (outer, inner).
    ///
    /// # Example
    /// ```
    /// use std::iter::FromIterator;
    /// use jagged_array::Jagged2;
    /// let a = Jagged2::from_iter(vec![
    ///     vec![1, 2, 3],
    ///     vec![4],
    ///     vec![],
    ///     vec![5, 6],
    /// ]);
    /// assert!(a.get((1, 0)) == Some(&4));
    /// assert!(a.get((2, 0)) == None);
    /// ```
    pub fn get(&self, index: (usize, usize)) -> Option<&T> {
        let view = try_opt!(self.get_row(index.0));
        view.get(index.1)
    }

    /// Index into the jagged array. The index is given in (Major, Minor) form,
    /// i.e. (row, column) or (outer, inner).
    ///
    /// # Example
    /// ```
    /// use std::iter::FromIterator;
    /// use jagged_array::Jagged2;
    /// let mut a = Jagged2::from_iter(vec![
    ///     vec![1, 2, 3],
    ///     vec![4],
    ///     vec![],
    ///     vec![5, 6],
    /// ]);
    /// assert!(a.get((1, 0)) == Some(&4));
    /// *a.get_mut((1, 0)).unwrap() = 11;
    /// assert!(a.get((1, 0)) == Some(&11));
    /// ```
    pub fn get_mut(&mut self, index: (usize, usize)) -> Option<&mut T> {
        let view = try_opt!(self.get_row_mut(index.0));
        view.get_mut(index.1)
    }

    /// Retrieve the given row as a contiguous slice of memory.
    pub fn get_row(&self, row: usize) -> Option<&[T]> {
        let &(row_onset, row_len) = try_opt!(self.onsets.get(row));
        unsafe {
            Some(slice::from_raw_parts(row_onset, row_len))
        }
    }
    /// Retrieve the given row as a contiguous slice of mutable memory.
    pub fn get_row_mut(&mut self, row: usize) -> Option<&mut [T]> {
        let &(row_onset, row_len) = try_opt!(self.onsets.get(row));
        unsafe {
            Some(slice::from_raw_parts_mut(row_onset, row_len))
        }
    }

    /// Return a slice over the entire storage area.
    ///
    /// # Example
    /// ```
    /// use std::iter::FromIterator;
    /// use jagged_array::Jagged2;
    /// let a = Jagged2::from_iter(vec![
    ///     vec![1, 2, 3],
    ///     vec![4],
    ///     vec![],
    ///     vec![5, 6],
    /// ]);
    /// assert!(a.as_flat_slice() == &vec![1, 2, 3, 4, 5, 6][..]);
    /// ```
    pub fn as_flat_slice(&self) -> &[T] {
        match self.onsets.get(0) {
            None => &[],
            Some(&(addr_start, _)) => unsafe {
                slice::from_raw_parts(addr_start, self.flat_len())
            }
        }
    }
    /// Return a mutable slice over the entire storage area.
    ///
    /// # Example
    /// ```
    /// use std::iter::FromIterator;
    /// use jagged_array::Jagged2;
    /// let mut a = Jagged2::from_iter(vec![
    ///     vec![1, 2, 3],
    ///     vec![4],
    ///     vec![],
    ///     vec![5, 6],
    /// ]);
    /// assert!(a.as_flat_slice()[3] == 4);
    /// a.as_flat_slice_mut()[3] = 33;
    /// assert!(a[(1, 0)] == 33);
    /// ```
    pub fn as_flat_slice_mut(&mut self) -> &mut [T] {
        match self.onsets.get(0) {
            None => &mut [],
            Some(&(addr_start, _)) => unsafe {
                slice::from_raw_parts_mut(addr_start, self.flat_len())
            }
        }
    }

    /// Return the total number of `T` held in the array.
    pub fn flat_len(&self) -> usize {
        // TODO: non-zero sized types can use a fast-path
        self.onsets.iter().map(|row| row.1).sum()
    }
    /// Return the number of rows held in the array.
    pub fn len(&self) -> usize {
        self.onsets.len()
    }
}

