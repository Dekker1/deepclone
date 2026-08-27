//! What the memo costs, against the two things it replaces.
//!
//! `Clone` is the floor: it bumps refcounts and copies nothing. The naive deep clone is what
//! anyone writes by hand before discovering the aliasing problem. The interesting number is
//! the gap to `Clone` on data with no sharing, since that is the case where the memo is pure
//! overhead.

use std::{hint::black_box, rc::Rc};

use deepclone::DeepClone;

/// Plain data with no shared pointers, where the memo is never touched.
#[derive(Clone, DeepClone)]
struct Flat {
	/// Some owned heap data to make the copy non-trivial.
	name: String,
	/// Enough elements that the copy is dominated by the memcpy.
	values: Vec<u64>,
}

/// A node in a graph of shared pointers.
#[derive(Clone, DeepClone)]
struct Node {
	/// Payload, so the copy is not purely pointer work.
	value: u64,
	/// Children, shared or not depending on how the graph was built.
	children: Vec<Rc<Node>>,
}

/// A chain of unique nodes, so every `Rc` is a memo miss and the table is pure cost.
fn chain(depth: u64) -> Node {
	let mut node = Node {
		value: 0,
		children: Vec::new(),
	};
	for value in 1..depth {
		node = Node {
			value,
			children: vec![Rc::new(node)],
		};
	}
	node
}

/// `width` nodes all pointing at one shared subtree, which is the shape the memo exists for:
/// the naive clone copies that subtree `width` times where this copies it once. Sharing a
/// *leaf* would measure nothing, since copying a leaf is cheaper than hashing it.
fn dag(width: u64, shared_depth: u64) -> Node {
	let shared = Rc::new(chain(shared_depth));
	Node {
		value: 0,
		children: (0..width)
			.map(|value| {
				Rc::new(Node {
					value,
					children: vec![Rc::clone(&shared)],
				})
			})
			.collect(),
	}
}

/// Entry point for the divan harness.
fn main() {
	divan::main();
}

/// A deep clone written the obvious way, duplicating the pointee at every reference.
fn naive(node: &Node) -> Node {
	Node {
		value: node.value,
		children: node
			.children
			.iter()
			.map(|child| Rc::new(naive(child)))
			.collect(),
	}
}

/// Data with no shared pointers, where the memo is created but never touched.
#[divan::bench_group(name = "flat, no shared pointers")]
mod flat {
	use deepclone::DeepClone;

	use super::{Flat, black_box};

	/// The floor: bumps refcounts, copies nothing, and is wrong for shared data.
	#[divan::bench]
	fn clone(bencher: divan::Bencher) {
		let source = input();
		bencher.bench_local(|| black_box(black_box(&source).clone()));
	}

	/// This crate.
	#[divan::bench]
	fn deep_clone(bencher: divan::Bencher) {
		let source = input();
		bencher.bench_local(|| black_box(black_box(&source).deep_clone()));
	}

	/// The data under test, identical for both benchmarks.
	fn input() -> Flat {
		Flat {
			name: "solver".to_owned(),
			values: (0..1024).collect(),
		}
	}
}

/// The shape the memo exists for: one subtree reachable through many parents.
#[divan::bench_group(name = "dag, 16 nodes sharing a 64-node subtree")]
mod shared {
	use deepclone::DeepClone;

	use super::{black_box, dag, naive};

	/// The floor: bumps refcounts, copies nothing, and is wrong for shared data.
	#[divan::bench]
	fn clone(bencher: divan::Bencher) {
		let source = dag(16, 64);
		bencher.bench_local(|| black_box(black_box(&source).clone()));
	}

	/// This crate.
	#[divan::bench]
	fn deep_clone(bencher: divan::Bencher) {
		let source = dag(16, 64);
		bencher.bench_local(|| black_box(black_box(&source).deep_clone()));
	}

	/// A deep clone without a memo, which duplicates shared data.
	#[divan::bench]
	fn naive_deep_clone(bencher: divan::Bencher) {
		let source = dag(16, 64);
		bencher.bench_local(|| black_box(naive(black_box(&source))));
	}
}

/// Every `Rc` unique, so the memo is pure overhead with nothing to deduplicate.
#[divan::bench_group(name = "chain, 256 unique nodes")]
mod unshared {
	use deepclone::DeepClone;

	use super::{black_box, chain, naive};

	/// The floor: bumps refcounts, copies nothing, and is wrong for shared data.
	#[divan::bench]
	fn clone(bencher: divan::Bencher) {
		let source = chain(256);
		bencher.bench_local(|| black_box(black_box(&source).clone()));
	}

	/// This crate.
	#[divan::bench]
	fn deep_clone(bencher: divan::Bencher) {
		let source = chain(256);
		bencher.bench_local(|| black_box(black_box(&source).deep_clone()));
	}

	/// A deep clone without a memo, which duplicates shared data.
	#[divan::bench]
	fn naive_deep_clone(bencher: divan::Bencher) {
		let source = chain(256);
		bencher.bench_local(|| black_box(naive(black_box(&source))));
	}
}
