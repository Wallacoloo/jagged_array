extern crate jagged_array;
extern crate serde_json;

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
    assert_eq!(a[[0, 0]], 1);
    assert_eq!(a[[0, 1]], 2);
    assert_eq!(a.get([0, 2]), Some(&3));
    assert_eq!(a.get([0, 3]), None);
    // Test row 1
    assert_eq!(a[[1, 0]], 4);
    assert_eq!(a.get([1, 1]), None);
    // Test row 2
    assert_eq!(a[[2, 3]], 8);
    assert_eq!(a.get([2, 4]), None);
    assert_eq!(a.get([2, 11]), None);
    // Test invalid rows
    assert_eq!(a.get([3, 0]), None);
    assert_eq!(a.get([3, 3]), None);
    assert_eq!(a.get([1234, 44]), None);
    assert_eq!(a.get([-1isize as usize, 0]), None);
    assert_eq!(a.get([0, -1isize as usize]), None);
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
    assert_eq!(a[[0, 0]], ());
    assert_eq!(a[[0, 1]], ());
    assert_eq!(a.get([0, 2]), None);
    // Test row 2
    assert_eq!(a.get([2, 0]), None);
    // Test invalid rows
    assert_eq!(a.get([3, 3]), None);
    assert_eq!(a.get([1234, 44]), None);
    assert_eq!(a.get([-1isize as usize, 0]), None);
    assert_eq!(a.get([0, -1isize as usize]), None);
}

#[test]
fn test_empty() {
    fn test_empty_for(a: Jagged2<u16>) {
        assert_eq!(a.len(), 0);
        assert_eq!(a.flat_len(), 0);
        assert_eq!(a.get([0, 0]), None);
        assert_eq!(a.get_row(0), None);
        assert_eq!(a.as_flat_slice(), &[] as &[u16]);
    }
    test_empty_for(Default::default());
    test_empty_for(Jagged2::from_iter(
        (0..0).map(|_| (0..0))
    ));
}

#[test]
fn test_clone_eq() {
    // Test that arrays impl Clone & Eq as expected.
    let a = Jagged2::from_iter(vec![
        vec![1, 2],
        vec![],
        vec![3],
    ]);
    let b = Jagged2::from_iter(vec![
        vec![1, 2],
        vec![6],
        vec![3],
    ]);
    let c = Jagged2::from_iter(vec![
        vec![0, 2],
        vec![],
        vec![3],
    ]);
    let d = Jagged2::from_iter(vec![
        vec![1, 2],
        vec![],
        vec![3],
        vec![]
    ]);
    let e = a.clone();
    assert_ne!(a, b);
    assert_ne!(a, c);
    assert_ne!(a, d);
    assert_ne!(b, c);
    assert_ne!(b, d);
    assert_ne!(c, d);
    assert_eq!(a, e);
}

#[test]
fn test_serde() {
    let a = Jagged2::from_iter(vec![
        vec![1, 2],
        vec![],
        vec![3],
    ]);
    let s = serde_json::to_string(&a).unwrap();
    assert_eq!(s, "[[1,2],[],[3]]");
    let b: Jagged2<u32> = serde_json::from_str(&s).unwrap();
    // Currently lacking Eq operator.
    assert_eq!(a.len(), b.len());
    assert_eq!(a.get_row(0), b.get_row(0));
    assert_eq!(a.get_row(1), b.get_row(1));
    assert_eq!(a.get_row(2), b.get_row(2));
}
