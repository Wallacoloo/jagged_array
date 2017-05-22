//! This crate provides a jagged array, i.e. a type that is semantically equivalent to
//! `Box<[Box<[T]>]>`, but implemented with better memory locality and fewer heap allocations.

#[macro_use] extern crate try_opt;

use std::iter::FromIterator;
use std::mem;
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
    /// Indicates where each row begins in memory.
    /// Note that onsets[0] points to the beginning of the underlying storage,
    /// which needs to be manually freed on drop,
    /// and onsets[len-1] points to the end of storage.
    /// Because of this, onsets.len() == num_rows + 1.
    /// 
    /// Minus bounds checking, data can be accessed essentially via
    /// `onsets[row][column]`
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
        assert!(mem::size_of::<T>() != 0, "Zero-Sized Types are not currently supported");
        let row_iter = row_iter.into_iter();
        // Collect the iterator into a flat vector,
        // and for each row, write the index into the flat vector at which it starts.
        let mut storage = Vec::new();
        // TODO: if we make onsets mutable and of type *const T, we can first base off of null
        // and then offset by the finalized address.
        let mut onsets = Vec::with_capacity(1 + row_iter.size_hint().0);
        for col_iter in row_iter {
            // store the index of the row, transmuted to *mut T.
            // This transmutation is done in order to reuse this vector for
            // holding absolute row addresses, once the base address is finalized.
            onsets.push(storage.len() as *mut T);
            storage.extend(col_iter);
        }
        onsets.push(storage.len() as *mut T);

        let storage = Box::into_raw(storage.into_boxed_slice()) as *mut T;
        // Transform the onsets from relative indices to absolute addresses.
        for onset in onsets.iter_mut() {
            unsafe {
                *onset = storage.offset(*onset as isize);
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
        let (row_onset, row_len) = try_opt!(self.get_row_components(row));
        unsafe {
            Some(slice::from_raw_parts(row_onset, row_len))
        }
    }
    /// Retrieve the given row as a contiguous slice of mutable memory.
    pub fn get_row_mut(&mut self, row: usize) -> Option<&mut [T]> {
        let (row_onset, row_len) = try_opt!(self.get_row_components(row));
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
        let (addr_start, len) = self.slice_components();
        unsafe {
            slice::from_raw_parts(addr_start, len)
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
        let (addr_start, len) = self.slice_components();
        unsafe {
            slice::from_raw_parts_mut(addr_start, len)
        }
    }

    /// Return the base address of the row and the number of items in the row,
    /// or None if the row doesn't exist.
    ///
    /// This method is useful in creating views of individual rows.
    fn get_row_components(&self, row: usize) -> Option<(*mut T, usize)> {
        // Figure out which indices in the storage correspond to the
        // start and end of the selected row.
        //
        // Note: if index.0 == usize::MAX, indexing is impossible;
        // we cannot have that many rows because of the extra overhead for tracking the row's END.
        // But usize::MAX = (-1isize as usize), so we definitely want to make
        // sure we correctly error for that query.
        let idx_of_row_end = try_opt!(row.checked_add(1));
        let row_end = *try_opt!(self.onsets.get(idx_of_row_end));
        // Because onsets[1+index.0] exists, it's safe to directly access
        // onsets[index.0]
        let row_onset = self.onsets[row];
        // TODO: Can T be size 0? If so this errors.
        let row_len = (row_end as usize - row_onset as usize)/mem::size_of::<T>();

        Some((row_onset, row_len))
    }

    /// Return the base pointer to the storage area and the number of items stored.
    /// This is used to construct slice views of the storage.
    fn slice_components(&self) -> (*mut T, usize) {
        let addr_start = self.onsets[0];
        let addr_end = *self.onsets.last().unwrap();
        let len = (addr_end as usize - addr_start as usize)/mem::size_of::<T>();
        (addr_start, len)
    }
}

