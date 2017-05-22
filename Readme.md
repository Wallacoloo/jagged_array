This crate provides a jagged array, i.e. a type that is semantically equivalent to
`Box<[Box<[T]>]>`, but implemented with better memory locality and fewer heap allocations.

## Example

```rust
extern crate jagged_array;
extern crate streaming_iterator;
use std::iter::FromIterator;
use jagged_array::Jagged2;
use streaming_iterator::StreamingIterator;

# fn main() {
// Create a jagged array from a vector of vectors
let mut a = Jagged2::from_iter(vec![
    vec![1, 2, 3],
    vec![4],
    vec![],
    vec![5, 6],
]);

// indexing is done in (row, column) form and supports `get` and `get_mut` variants.
assert_eq!(a[(1, 0)], 4);
*a.get_mut((1, 0)).unwrap() = 11;
assert_eq!(a.get((1, 0)), Some(&11));

// Whole rows can also be accessed and modified
assert_eq!(a.get_row(3), Some(&[5, 6][..]));
a.get_row_mut(3).unwrap()[1] = 11;
// Note that although elements are modifiable, the structure is not;
// items cannot be inserted into rows, nor can new rows be added.

// Iteration via `StreamingIterator`s. See the docs for more detail.
let mut iter = a.stream();
while let Some(row) = iter.next() {
    println!("row: {:?}", row);
}
# }
```


## Documentation

Documentation can be found on [docs.rs](https://docs.rs/jagged_array/)


## License

The code in this repository is licensed under either of

   * Apache License, Version 2.0 (http://www.apache.org/licenses/LICENSE-2.0)
   * MIT license (http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this repository by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
