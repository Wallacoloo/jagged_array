//! This crate provides a jagged array, i.e. a type that is semantically equivalent to
//! `Box<[Box<[T]>]>`, but implemented with better memory locality and fewer heap allocations.
//!
//! # Example
//!
//! ```rust
//! extern crate jagged_array;
//! extern crate streaming_iterator;
//! use std::iter::FromIterator;
//! use jagged_array::{Jagged2, Jagged2Builder};
//! use streaming_iterator::StreamingIterator;
//!
//! # fn main() {
//! // Create a builder object for the array, and append some data.
//! // Each `extend` call builds another row.
//! let mut builder = Jagged2Builder::new();
//! builder.extend(&[1, 2, 3]); // row 0 = [1, 2, 3]
//! builder.extend(vec![4]);    // row 1 = [4]
//! builder.extend(&[]);        // row 2 = []
//! builder.extend(5..7);       // row 3 = [5, 6]
//!
//! // Finalize the builder into a non-resizable jagged array.
//! let mut a: Jagged2<u32> = builder.into();
//! // Alternatively, we could have created the same array from a Vec<Vec<T>> type:
//! let alt_form = Jagged2::from_iter(vec![
//!     vec![1, 2, 3],
//!     vec![4],
//!     vec![],
//!     vec![5, 6],
//! ]);
//! assert_eq!(a, alt_form);
//!
//! // Indexing is done in [row, column] form and supports `get` and `get_mut` variants.
//! assert_eq!(a[[1, 0]], 4);
//! *a.get_mut([1, 0]).unwrap() = 11;
//! assert_eq!(a.get([1, 0]), Some(&11));
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
extern crate serde;
extern crate streaming_iterator;

use std::fmt;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};
use std::iter::{FromIterator, IntoIterator};
use std::marker::PhantomData;
use std::mem;
use std::ops::Index;
use std::slice;

use serde::de::{Deserialize, Deserializer, DeserializeSeed, SeqAccess, Visitor};
use serde::ser::{Serialize, Serializer, SerializeSeq};

use streaming_iterator as stream;
use streaming_iterator::StreamingIterator;

/// 2-dimensional jagged array type. It's equivalent to a
/// `Box<Box<[mut T]>>`, but where all array data is stored contiguously
/// and fewer allocations are performed.
/// 
/// Note that no dimension of the array can be modified after creation.
///
/// Jagged arrays can be created via the [`Jagged2Builder`] type
/// or the [`from_iter`] method.
///
/// [`Jagged2Builder`]: struct.Jagged2Builder.html
/// [`from_iter`]: struct.Jagged2.html#method.from_iter
pub struct Jagged2<T> {
    /// Slices into the underlying storage, indexed by row.
    /// Minus bounds checking, data can be accessed essentially via
    /// `onsets[row].0[column]`.
    /// Row length is accessed by `onsets[row].1`.
    onsets: Box<[(*mut T, usize)]>,
}

/// Struct to facilitate building a jagged array row by row.
///
/// # Example
/// ```
/// use jagged_array::{Jagged2, Jagged2Builder};
///
/// let mut builder = Jagged2Builder::new();
/// builder.extend(&[1, 2]); // push an array/slice
/// builder.extend((0..2)); // push an iterator (range)
/// builder.extend(vec![3]); // push and consume a vector
///
/// // Finalize the builder into an array.
/// let array: Jagged2<u32> = builder.into();
///
/// assert_eq!(array.len(), 3);
/// assert_eq!(array.get_row(0), Some(&[1, 2][..]));
/// assert_eq!(array.get_row(1), Some(&[0, 1][..]));
/// assert_eq!(array.get_row(2), Some(&[3][..]));
/// ```
// The builder is 100% safe, before finalization.
// Because the `onsets` field is using indexes prior to finalization,
// the struct is safe to Clone, Eq or Hash predictably.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Jagged2Builder<T> {
    /// Holds all the array data, contiguously.
    storage: Vec<T>,
    /// Holds (base address, row length) for each row.
    /// Note that the base address isn't actually an address,
    /// but the offset into the storage in units of T, cast to *mut T.
    onsets: Vec<(*mut T, usize)>,
}

/// [`StreamingIterator`] implementation for [`Jagged2`].
/// This allows for iteration with some lifetime restrictions on the value
/// returned by `next`.
/// 
/// See [`Jagged2::stream`] for more info.
///
/// [`StreamingIterator`]: ../streaming_iterator/trait.StreamingIterator.html
/// [`Jagged2`]: struct.Jagged2.html
/// [`next`]: ../streaming_iterator/trait.StreamingIterator.html#method.next
/// [`Jagged2::stream`]: struct.Jagged2.html#method.stream
//#[derive(Debug)] // TODO: uncomment this when streaming_iterator adds Debug support.
pub struct Stream<'a, T: 'a> {
    onset_iter: stream::Convert<slice::Iter<'a, (*mut T, usize)>>,
}

impl<T> Index<[usize; 2]> for Jagged2<T> {
    type Output = T;
    /// Index into the jagged array. The index is given in (Major, Minor) form,
    /// i.e. (row, column) or (outer, inner).
    /// `array[[0, 0]]` is adjacent to `array[[0, 1]]` in memory but not
    /// necessarily to `array[[1, 0]]`.
    fn index(&self, index: [usize; 2]) -> &T {
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

impl<T> Default for Jagged2<T> {
    fn default() -> Self {
        // zero-sized array with no elements
        let onsets = Vec::new().into_boxed_slice();
        Self{ onsets }
    }
}

impl<T> Debug for Jagged2<T>
    where T: Debug
{
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        // empty array should be compactly displayed.
        if self.is_empty() {
            return f.write_str("[]");
        }

        f.write_str("[\n")?;

        let mut stream = self.stream();
        while let Some(row) = stream.next() {
            f.write_str("  ")?;
            row.fmt(f)?;
            f.write_str("\n")?;
        }

        f.write_str("]")?;
        Ok(())
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
    /// assert_eq!(a[[0, 1]], 2); // first row, second column
    /// ```
    fn from_iter<IRow>(row_iter: IRow) -> Self
        where IRow: IntoIterator<Item=ICol>
    {
        // Tranform all inputs into their iterators.
        // We need to collect *just the iterators* into a vector so that we can get an accurate size
        // estimate of the overall storage BEFORE any more allocation.
        // Having an accurate size to use with Vec::with_capacity makes a substantial different
        // (can halve the time it takes to construct the jagged array).
        let row_iters: Vec<_> = row_iter.into_iter().map(|i| i.into_iter()).collect();
        let row_estimate = row_iters.len();
        let item_estimate = row_iters.iter().map(|i| i.size_hint().0).sum();
        let mut builder = Jagged2Builder::with_capacity(row_estimate, item_estimate);

        for row in row_iters {
            builder.extend(row);
        }
        builder.into()
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

impl<T> Clone for Jagged2<T>
    where T: Clone
{
    fn clone(&self) -> Self {
        let mut builder = Jagged2Builder::with_capacity(self.len(), self.flat_len());
        let mut rows = self.stream();
        while let Some(row) = rows.next() {
            builder.extend(row.iter().cloned());
        }
        builder.into()
    }
}

impl<T> PartialEq for Jagged2<T>
    where T: PartialEq
{
    fn eq(&self, other: &Jagged2<T>) -> bool {
        if self.len() != other.len() {
            return false;
        }
        // TODO: implement using .zip() and .all()
        // Currently StreamingIterator doesn't support .zip
        let (mut stream_me, mut stream_other) = (self.stream(), other.stream());
        while let (Some(row_me), Some(row_other)) = (stream_me.next(), stream_other.next()) {
            if row_me != row_other {
                return false;
            }
        }
        true
    }
}

impl<T> Eq for Jagged2<T> where T: Eq {}

impl<T> Hash for Jagged2<T>
    where T: Hash
{
    fn hash<H>(&self, state: &mut H)
        where H: Hasher
    {
        let mut stream = self.stream();
        while let Some(row) = stream.next() {
            row.hash(state);
        }
    }
}

impl<T> Serialize for Jagged2<T>
    where T: Serialize
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where S: Serializer
    {
        // Serialize as a sequence of [T] sequences.
        let mut seq = serializer.serialize_seq(Some(self.len()))?;
        let mut stream = self.stream();
        while let Some(row) = stream.next() {
            seq.serialize_element(row)?;
        }
        seq.end()
    }
}

impl<'de, T> Deserialize<'de> for Jagged2<T>
    where T: Deserialize<'de>
{
    fn deserialize<D>(deserializer: D) -> Result<Jagged2<T>, D::Error>
        where D: Deserializer<'de>
    {
        // Visit the entire Jagged2 array; returns the array.
        struct JaggedVisitor<T>(PhantomData<T>);
        // Deserialize one row of a Jagged2 array into the builder.
        struct RowDeserializer<'a, T: 'a>(&'a mut Jagged2Builder<T>);
        // Take a Serde SeqAccess type and adapt it to an iterator.
        // This lets use use the array builder's `extend` method.
        struct SeqAccessToIter<'a, A, E: 'a, T> {
            seq: A,
            /// Where to write back the error if an error occurs in self.next()
            result: &'a mut Result<(), E>,
            phantom: PhantomData<T>,
        }

        impl<'de, T> Visitor<'de> for JaggedVisitor<T>
            where T: Deserialize<'de>
        {
            type Value = Jagged2<T>;
            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("an array of arrays of T (i.e. [[T]])")
            }
            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
                where A: SeqAccess<'de>
            {
                // Create a builder, then deserialize into it one row at a time.
                let mut builder = Jagged2Builder::with_capacity(seq.size_hint().unwrap_or(0), 0);
                while seq.next_element_seed(RowDeserializer(&mut builder))?.is_some() {}
                Ok(builder.into())
            }
        }
        impl<'de, 'a, T> DeserializeSeed<'de> for RowDeserializer<'a, T>
            where T: Deserialize<'de>
        {
            type Value = (); // deserialize into a builder; nothing is returned
            fn deserialize<D>(self, deserializer: D) -> Result<(), D::Error>
                where D: Deserializer<'de>
            {
                deserializer.deserialize_seq(self)
            }
        }
        impl<'de, 'a, T: 'a> Visitor<'de> for RowDeserializer<'a, T>
            where T: Deserialize<'de>
        {
            type Value = ();
            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("an sequence of T (i.e. [T])")
            }
            fn visit_seq<A>(self, seq: A) -> Result<(), A::Error>
                where A: SeqAccess<'de>
            {
                // Adapt the row SeqAccessor into an iterator,
                // and use the builder's `extend` method to capture the whole row.
                let mut result: Result<(), A::Error> = Ok(());
                self.0.extend(SeqAccessToIter{
                    seq,
                    result: &mut result,
                    phantom: PhantomData
                });
                result
            }
        }

        impl<'de, 'a, A, T> Iterator for SeqAccessToIter<'a, A, A::Error, T>
            where A: SeqAccess<'de>,
            T: Deserialize<'de>
        {
            type Item = T;
            fn next(&mut self) -> Option<T> {
                // Yield the next item from the accessor.
                // If it is an error, write this error to the result reference
                // and terminate the iterator.
                match self.seq.next_element() {
                    Ok(value) => value,
                    Err(error) => {
                        *self.result = Err(error);
                        None
                    }
                }
            }
            fn size_hint(&self) -> (usize, Option<usize>) {
                match self.seq.size_hint() {
                    None => (0, None),
                    // Serde's size_hint is *exact*. i.e. if the SeqAccess
                    // provides a size_hint, it is simultaneously a lower and upper bound.
                    Some(value) => (value, Some(value)),
                }
            }
        }

        deserializer.deserialize_seq(JaggedVisitor(PhantomData))
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
    /// assert_eq!(a.get([1, 0]), Some(&4));
    /// assert_eq!(a.get([2, 0]), None);
    /// ```
    pub fn get(&self, index: [usize; 2]) -> Option<&T> {
        let view = try_opt!(self.get_row(index[0]));
        view.get(index[1])
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
    /// assert_eq!(a.get([1, 0]), Some(&4));
    /// *a.get_mut([1, 0]).unwrap() = 11;
    /// assert_eq!(a.get([1, 0]), Some(&11));
    /// ```
    pub fn get_mut(&mut self, index: [usize; 2]) -> Option<&mut T> {
        let view = try_opt!(self.get_row_mut(index[0]));
        view.get_mut(index[1])
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
    /// assert_eq!(a[[3, 1]], 11);
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
    /// assert_eq!(a[[1, 0]], 33);
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
    /// This is generally a constant-time operation, but if `T` is a zero-sized type
    /// then the time complexity is proportional to the number of rows in the array.
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

    /// Return true if the array holds no items.
    /// Semantically equivalent to `jagged2.len() == 0`.
    pub fn is_empty(&self) -> bool {
        self.onsets.is_empty()
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


impl<T> Jagged2Builder<T> {
    /// Construct a new array builder, defaulted to not holding any rows.
    pub fn new() -> Self {
        Default::default()
    }
    /// Construct an empty array builder, but preallocate enough heap space
    /// for `row_cap` rows, and a total of `item_cap` items stored cumulatively
    /// in the array.
    ///
    /// Adding more rows/items than specified after using this constructor is
    /// not an error; more space will be allocated on demand.
    pub fn with_capacity(row_cap: usize, item_cap: usize) -> Self {
        Self {
            storage: Vec::with_capacity(item_cap),
            onsets: Vec::with_capacity(row_cap),
        }
    }
    /// Return the number of rows held in the array builder.
    pub fn len(&self) -> usize {
        self.onsets.len()
    }
    /// Return the total number of `T` held in the array.
    pub fn flat_len(&self) -> usize {
        self.storage.len()
    }
    /// If `new_len < self.len()`, the array will be extended
    /// with empty rows until it reaches the given length. Otherwise,
    /// rows will be truncated from the tail of the array until the length is reached.
    ///
    /// # Example
    /// ```
    /// use jagged_array::Jagged2Builder;
    /// let mut builder: Jagged2Builder<u32> = Jagged2Builder::new();
    /// builder.extend(&[1, 2]);
    /// builder.extend(&[3]);
    /// builder.resize(4);
    /// let mut expected: Jagged2Builder<u32> = Jagged2Builder::new();
    /// expected.extend(&[1, 2]);
    /// expected.extend(&[3]);
    /// expected.extend(&[]);
    /// expected.extend(&[]);
    /// assert_eq!(builder, expected);
    /// ```
    pub fn resize(&mut self, new_len: usize) {
        // new rows start at end of array w/ length=0
        let onset_padding = (self.flat_len() as *mut T, 0);
        self.onsets.resize(new_len, onset_padding);

        let new_size = match self.onsets.last() {
            None => 0, // no rows -> no data
            Some(&(row_start, row_length)) => (row_start as usize) + row_length,
        };
        self.storage.drain(new_size..);
    }
}


impl<T> Default for Jagged2Builder<T> {
    fn default() -> Self {
        Self {
            storage: Vec::new(),
            onsets: Vec::new(),
        }
    }
}

impl<T> Extend<T> for Jagged2Builder<T> {
    /// Push a new row into the builder
    fn extend<I>(&mut self, row: I)
        where I: IntoIterator<Item=T>
    {
        let row_data = row.into_iter();

        let row_start = self.storage.len();
        self.storage.extend(row_data);
        let row_end = self.storage.len();
        // store the index of the row, transmuted to *mut T.
        // This transmutation is done in order to reuse this vector for
        // holding absolute row addresses, once the base address is finalized.
        self.onsets.push((row_start as *mut T, row_end-row_start));
    }
}

impl<'a, T> Extend<&'a T> for Jagged2Builder<T>
    where T: 'a + Copy
{
    /// Push a new row into the builder;
    /// this also works for array/slice types passed by reference, if T is Copy.
    fn extend<I>(&mut self, row: I)
        where I: IntoIterator<Item=&'a T>
    {
        self.extend(row.into_iter().cloned())
    }
}

impl<T> Into<Jagged2<T>> for Jagged2Builder<T> {
    fn into(self) -> Jagged2<T> {
        // Make the storage immutable, and then also cast away its length.
        // Length data is redundant with the slice info we already have.
        let storage = Box::into_raw(self.storage.into_boxed_slice()) as *mut T;
        let mut onsets = self.onsets;

        // Convert the row base addresses from relative indices to absolute addresses.
        // This is safe to do now because `storage` is immutable and fixed.
        for onset in onsets.iter_mut() {
            unsafe {
                onset.0 = storage.offset(onset.0 as isize);
            }
        }
        // Also finalize the row metadata
        let onsets = onsets.into_boxed_slice();
        // Now data can be accessed via (psuedo): `onsets[row].0[column]`
        Jagged2{ onsets }
    }
}

