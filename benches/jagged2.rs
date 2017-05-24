#![feature(test)]

extern crate jagged_array;
extern crate rand;
extern crate serde_json;
extern crate test;

use std::iter::FromIterator;

use rand::{Rng, XorShiftRng};
use test::Bencher;

use jagged_array::Jagged2;

fn build_vec() -> Vec<Vec<u32>> {
    (0..100).map(|i| {
        (0..(4*i % 300)).collect()
    }).collect()
}

fn build_array() -> Jagged2<u32> {
    (0..100).map(|i| {
        (0..(4*i % 300))
    }).collect()
}


#[bench]
fn bench_collect_jag(b: &mut Bencher) {
    b.iter(build_array)
}

#[bench]
fn bench_collect_vec(b: &mut Bencher) {
    b.iter(build_vec)
}

#[bench]
fn bench_access_jag(b: &mut Bencher) {
    let mut rng = rand::XorShiftRng::new_unseeded();
    let arr = build_array();
    b.iter(||
        (1..16).map(|_| arr.get([rng.gen::<usize>() % 128, rng.gen::<usize>() % 128])).last()
    );
}

#[bench]
fn bench_access_vec(b: &mut Bencher) {
    let mut rng = rand::XorShiftRng::new_unseeded();
    let arr = build_vec();
    b.iter(||
        (1..16).map(|_| arr.get(rng.gen::<usize>() % 128).map(|v| v.get(rng.gen::<usize>() % 128))).last()
    );
}

#[bench]
fn bench_flat_len_jag(b: &mut Bencher) {
    let arr = build_array();
    b.iter(||
        arr.flat_len()
    );
}

#[bench]
fn bench_flat_len_vec(b: &mut Bencher) {
    let arr = build_vec();
    b.iter::<usize, _>(|| {
        arr.iter().map(|row| row.len()).sum()
    });
}

#[bench]
fn bench_serde_jag(b: &mut Bencher) {
    let arr = build_array();
    b.iter(|| -> Jagged2<u32> {
        let s = serde_json::to_string(&arr).unwrap();
        serde_json::from_str(&s).unwrap()
    });
}

#[bench]
fn bench_serde_vec(b: &mut Bencher) {
    let vec = build_vec();
    b.iter(|| -> Vec<Vec<u32>> {
        let s = serde_json::to_string(&vec).unwrap();
        serde_json::from_str(&s).unwrap()
    });
}
