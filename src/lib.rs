//! This crate provides a jagged array, i.e. a type that is semantically equivalent to
//! `Box<[Box<[T]>]>`, but implemented with better memory locality and fewer heap allocations.
//!
//! # Example
//!
//! ```
//! extern crate jagged_array;
//! extern crate streaming_iterator;
//! use std::iter::FromIterator;
//! use jagged_array::Jagged2;
//! use streaming_iterator::StreamingIterator;
//!
//! # fn main() {
//! // Create a jagged array from a vector of vectors
//! let mut a = Jagged2::from_iter(vec![
//!     vec![1, 2, 3],
//!     vec![4],
//!     vec![],
//!     vec![5, 6],
//! ]);
//!
//! // indexing is done in (row, column) form and supports `get` and `get_mut` variants.
//! assert_eq!(a[(1, 0)], 4);
//! *a.get_mut((1, 0)).unwrap() = 11;
//! assert_eq!(a.get((1, 0)), Some(&11));
//!
//! // Whole rows can also be accessed and modified
//! assert_eq!(a.get_row(3), Some(&[5, 6][..]));
//! a.get_row_mut(3).unwrap()[1] = 11;
//! // Note that although elements are modifiable, the structure is not;
//! // items cannot be inserted into rows, nor can new rows be added.
//!
//! // Iteration via `StreamingIterator`s. See the docs for more detail.
//! let mut iter = a.stream();
//! while let Some(row) = iter.next() {
//!     println!("row: {:?}", row);
//! }
//! # }
//! ```

#[macro_use] extern crate try_opt;
extern crate streaming_iterator;

use std::iter::{FromIterator, IntoIterator};
use std::mem;
use std::ops::Index;
use std::slice;

use streaming_iterator as stream;
use streaming_iterator::StreamingIterator;

/// 2-dimensional jagged array type. It's equivalent to a
/// `Box<Box<[mut T]>>`, but where all array data is stored contiguously
/// and fewer allocations are performed.
/// 
/// Note that no dimension of the array can be modified after creation.
#[derive(Debug)]
pub struct Jagged2<T> {
    /// Slices into the underlying storage, indexed by row.
    /// Minus bounds checking, data can be accessed essentially via
    /// `onsets[row].0[column]`.
    /// Row length is accessed by `onsets[row].1`.
    onsets: Box<[(*mut T, usize)]>,
}

/// [`StreamingIterator`] implementation for [`Jagged2`].
/// This allows for iteration with some lifetime restrictions on the value
/// returned by `next`. See [`Jagged2::stream`] for more info.
///
/// [`StreamingIterator`]: ../streaming_iterator/trait.StreamingIterator.html
/// [`Jagged2`]: struct.Jagged2.html
/// [`next`]: ../streaming_iterator/trait.StreamingIterator.html#method.next
/// [`Jagged2::stream`]: struct.Jagged2.html#method.stream
#[derive(Debug)]
pub struct Stream<'a, T: 'a> {
    onset_iter: stream::Convert<slice::Iter<'a, (*mut T, usize)>>,
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
    /// assert_eq!(a.len(), 4); // 4 rows
    /// assert_eq!(a.as_flat_slice(), &[1, 2, 3, 4, 5, 6][..]); // contiguous view
    /// assert_eq!(a.get_row(3), Some(&[5, 6][..])); // third row
    /// assert_eq!(a[(0, 1)], 2); // first row, second column
    /// ```
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

impl<'a, T> StreamingIterator for Stream<'a, T> {
    type Item = [T];
    fn advance(&mut self) {
        self.onset_iter.advance();
    }
    fn get(&self) -> Option<&Self::Item> {
        let &(row_addr, row_len) = *try_opt!(self.onset_iter.get());
        Some(unsafe {
            // The slice will have a lifetime limited to 'self,
            // and self borrows the backing storage (through Jagged2),
            // therefore the slice cannot outlive its storage;
            // this is safe.
            slice::from_raw_parts(row_addr, row_len)
        })
    }
    // optional override done for performance gains.
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.onset_iter.size_hint()
    }
    // optional override done for performance gains.
    fn count(self) -> usize {
        self.onset_iter.count()
    }
    // optional override done for performance gains.
    fn nth(&mut self, n: usize) -> Option<&Self::Item> {
        let &(row_addr, row_len) = *try_opt!(self.onset_iter.nth(n));
        Some(unsafe {
            // The slice will have a lifetime limited to 'self,
            // and self borrows the backing storage (through Jagged2),
            // therefore the slice cannot outlive its storage;
            // this is safe.
            slice::from_raw_parts(row_addr, row_len)
        })
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
    /// assert_eq!(a.get((1, 0)), Some(&4));
    /// assert_eq!(a.get((2, 0)), None);
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
    /// assert_eq!(a.get((1, 0)), Some(&4));
    /// *a.get_mut((1, 0)).unwrap() = 11;
    /// assert_eq!(a.get((1, 0)), Some(&11));
    /// ```
    pub fn get_mut(&mut self, index: (usize, usize)) -> Option<&mut T> {
        let view = try_opt!(self.get_row_mut(index.0));
        view.get_mut(index.1)
    }

    /// Retrieve the given row as a contiguous slice of memory.
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
    /// assert_eq!(a.get_row(3), Some(&[5, 6][..]));
    /// ```
    pub fn get_row(&self, row: usize) -> Option<&[T]> {
        let &(row_onset, row_len) = try_opt!(self.onsets.get(row));
        unsafe {
            Some(slice::from_raw_parts(row_onset, row_len))
        }
    }
    /// Retrieve the given row as a contiguous slice of mutable memory.
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
    /// assert_eq!(a.get_row_mut(3), Some(&mut[5, 6][..]));
    /// a.get_row_mut(3).unwrap()[1] = 11;
    /// assert_eq!(a[(3, 1)], 11);
    /// ```
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
    /// assert_eq!(a.as_flat_slice(), &[1, 2, 3, 4, 5, 6][..]);
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
    /// assert_eq!(a.as_flat_slice()[3], 4);
    /// a.as_flat_slice_mut()[3] = 33;
    /// assert_eq!(a[(1, 0)], 33);
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
    /// assert_eq!(a.flat_len(), 6);
    /// ```
    pub fn flat_len(&self) -> usize {
        if mem::size_of::<T>() == 0 {
            // For zero-sized types, we need to explicitly sum the length of each row slice;
            // we cannot use addressing tricks because each element shares the same address.
            self.onsets.iter().map(|row| row.1).sum()
        } else {
            // rows are stored sequentially and contiguously, with no extra padding,
            // so the number of elements is (&rows.first() - &rows.last())/sizeof(T) +
            // rows.last().len()
            let (last_addr, last_len) = match self.onsets.last() {
                None => return 0,
                Some(&(addr, len)) => (addr, len),
            };
            // if the array is empty, we would have returned already; safe to index row 0 now.
            let first_addr = self.onsets[0].0;
            (last_addr as usize - first_addr as usize) / mem::size_of::<T>() + last_len
        }
    }
    /// Return the number of rows held in the array.
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
    /// assert_eq!(a.len(), 4);
    /// ```
    pub fn len(&self) -> usize {
        self.onsets.len()
    }

    /// Create a [streaming iterator] over the rows of this array.
    /// Lifetime restrictions prevent implementing `std::iter::Iterator` for this
    /// type, however a streaming iterator provides similar features except that
    /// the lifetime of the items it yields is tied to the lifetime of the iterator
    /// itself.
    ///
    /// # Example
    /// ```
    /// # extern crate jagged_array;
    /// # extern crate streaming_iterator;
    /// use std::iter::FromIterator;
    /// use jagged_array::Jagged2;
    /// use streaming_iterator::StreamingIterator;
    ///
    /// # fn main() {
    /// let a = Jagged2::from_iter(vec![
    ///     vec![1, 2, 3],
    ///     vec![4],
    /// ]);
    /// let mut iter = a.stream();
    /// while let Some(row) = iter.next() {
    ///     println!("row: {:?}", row);
    /// }
    /// # }
    /// ```
    ///
    /// [streaming iterator]: ../streaming_iterator/index.html
    pub fn stream<'a>(&'a self) -> Stream<'a, T> {
        Stream{ onset_iter: stream::convert(self.onsets.iter()) }
    }

    /// Consumes self and returns the underlying storage
    /// (identical to `as_flat_slice_mut`, but owned).
    ///
    /// The slice can optionally be turned into a vector by calling
    /// `slice::into_vec()` on the result.
    pub fn into_boxed_slice(self) -> Box<[T]> {
        self.into_components().0
    }
    /// Consumes self and returns a tuple whose first element is a boxed slice
    /// of the underlying storage (identical to `as_flat_slice_mut`, but owned)
    /// and whose second element indicates the start address and length of each row.
    fn into_components(mut self) -> (Box<[T]>, Box<[(*mut T, usize)]>) {
        unsafe {
            let slice = Box::from_raw(self.as_flat_slice_mut() as *mut [T]);
            let onsets = mem::replace(&mut self.onsets, Vec::new().into_boxed_slice());
            // The box now owns all our memory; don't drop self in order to avoid
            // double-freeing.
            mem::forget(self);
            (slice, onsets)
        }
    }
}

