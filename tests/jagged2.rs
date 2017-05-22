extern crate jagged_array;

use std::iter::FromIterator;
use jagged_array::Jagged2;

#[test]
fn test_create() {
    let a = Jagged2::from_iter(vec![
        vec![1, 2, 3],
        vec![4],
        vec![5, 6, 7, 8],
        vec![],
    ]);
    // Test row 0
    assert!(a[(0, 0)] == 1);
    assert!(a[(0, 1)] == 2);
    assert!(a.get((0, 2)) == Some(&3));
    assert!(a.get((0, 3)) == None);
    // Test row 1
    assert!(a[(1, 0)] == 4);
    assert!(a.get((1, 1)) == None);
    // Test row 2
    assert!(a[(2, 3)] == 8);
    assert!(a.get((2, 4)) == None);
    assert!(a.get((2, 11)) == None);
    // Test invalid rows
    assert!(a.get((3, 0)) == None);
    assert!(a.get((3, 3)) == None);
    assert!(a.get((1234, 44)) == None);
    assert!(a.get((-1isize as usize, 0)) == None);
}

