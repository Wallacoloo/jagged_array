extern crate jagged_array;

use std::iter::FromIterator;
use jagged_array::Jagged2;

#[test]
fn test_sized() {
    let a = Jagged2::from_iter(vec![
        vec![1, 2, 3],
        vec![4],
        vec![5, 6, 7, 8],
        vec![],
    ]);
    assert_eq!(a.len(), 4);
    assert_eq!(a.flat_len(), 8);
    // Test row 0
    assert_eq!(a[(0, 0)], 1);
    assert_eq!(a[(0, 1)], 2);
    assert_eq!(a.get((0, 2)), Some(&3));
    assert_eq!(a.get((0, 3)), None);
    // Test row 1
    assert_eq!(a[(1, 0)], 4);
    assert_eq!(a.get((1, 1)), None);
    // Test row 2
    assert_eq!(a[(2, 3)], 8);
    assert_eq!(a.get((2, 4)), None);
    assert_eq!(a.get((2, 11)), None);
    // Test invalid rows
    assert_eq!(a.get((3, 0)), None);
    assert_eq!(a.get((3, 3)), None);
    assert_eq!(a.get((1234, 44)), None);
    assert_eq!(a.get((-1isize as usize, 0)), None);
    assert_eq!(a.get((0, -1isize as usize)), None);
}

#[test]
fn test_zero_sized() {
    let a = Jagged2::from_iter(vec![
        vec![(), ()],
        vec![()],
        vec![],
        vec![(), (), ()],
    ]);
    assert_eq!(a.len(), 4);
    assert_eq!(a.flat_len(), 6);
    // Test row 0
    assert_eq!(a[(0, 0)], ());
    assert_eq!(a[(0, 1)], ());
    assert_eq!(a.get((0, 2)), None);
    // Test row 2
    assert_eq!(a.get((2, 0)), None);
    // Test invalid rows
    assert_eq!(a.get((3, 3)), None);
    assert_eq!(a.get((1234, 44)), None);
    assert_eq!(a.get((-1isize as usize, 0)), None);
    assert_eq!(a.get((0, -1isize as usize)), None);
}

#[test]
fn test_empty() {
    fn test_empty_for(a: Jagged2<u16>) {
        assert_eq!(a.len(), 0);
        assert_eq!(a.flat_len(), 0);
        assert_eq!(a.get((0, 0)), None);
        assert_eq!(a.get_row(0), None);
        assert_eq!(a.as_flat_slice(), &[]);
    }
    test_empty_for(Default::default());
    test_empty_for(Jagged2::from_iter(
        (0..0).map(|_| (0..0))
    ));
}
