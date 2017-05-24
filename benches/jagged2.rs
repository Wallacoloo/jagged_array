#![feature(test)]

extern crate jagged_array;
extern crate rand;
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
fn bench_collect(b: &mut Bencher) {
    b.iter(build_array)
}

#[bench]
fn bench_collect_vec(b: &mut Bencher) {
    b.iter(build_vec)
}

#[bench]
fn bench_access(b: &mut Bencher) {
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
fn bench_flat_len(b: &mut Bencher) {
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
