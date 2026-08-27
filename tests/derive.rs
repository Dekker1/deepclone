//! Tests for the shapes `#[derive(DeepClone)]` has to handle.

// The derive is what makes these shapes writable at all, so there is nothing left to test
// without it.
#![cfg(feature = "derive")]
#![expect(
	clippy::tests_outside_test_module,
	reason = "an integration test crate is entirely tests"
)]

use std::{
	cell::RefCell,
	collections::HashMap,
	marker::PhantomData,
	rc::Rc,
	sync::{Arc, Mutex},
};

use deepclone::{Cloner, DeepClone};

#[derive(DeepClone)]
struct Attributes {
	#[deepclone(clone)]
	foreign: Foreign,
	#[deepclone(with = double)]
	doubled: u32,
	#[deepclone(default)]
	scratch: Vec<Foreign>,
}

/// A where-clause on the type must survive, and the generated `DeepClone` bounds are added to
/// it rather than replacing it.
#[derive(DeepClone)]
struct Constrained<T>
where
	T: Copy + 'static,
{
	value: T,
	shared: Rc<RefCell<T>>,
}

/// A foreign type with no `DeepClone` impl, standing in for the ones a user cannot implement.
#[derive(Clone, Debug, PartialEq)]
struct Foreign(u32);

/// Plain data alongside shared data, which is the shape most real types have.
#[derive(DeepClone)]
struct Mixed {
	name: String,
	counts: Vec<u32>,
	lookup: HashMap<String, Rc<RefCell<u32>>>,
	shared: Rc<RefCell<u32>>,
	threaded: Arc<Mutex<u32>>,
	marker: PhantomData<fn()>,
}

/// `T` is only ever cloned by `Clone`, so the default `T: DeepClone` bound would be too
/// strong and has to be replaceable.
#[derive(DeepClone)]
#[deepclone(bound = "T: Clone")]
struct Opaque<T> {
	#[deepclone(clone)]
	value: T,
	shared: Rc<RefCell<u32>>,
}

#[derive(DeepClone)]
enum Tree<T: 'static> {
	Leaf,
	Value(T),
	Branch {
		label: String,
		children: Vec<Rc<RefCell<Tree<T>>>>,
	},
}

#[derive(DeepClone)]
struct Tuple(u32, Rc<RefCell<String>>, Rc<RefCell<String>>);

#[derive(DeepClone)]
struct Unit;

#[test]
fn a_mix_of_shared_and_plain_fields() {
	let shared = Rc::new(RefCell::new(1));
	let mut lookup = HashMap::new();
	let _ = lookup.insert("one".to_owned(), Rc::clone(&shared));

	let original = Mixed {
		name: "solver".to_owned(),
		counts: vec![1, 2, 3],
		lookup,
		shared: Rc::clone(&shared),
		threaded: Arc::new(Mutex::new(0)),
		marker: PhantomData,
	};
	let copy = original.deep_clone();

	assert_eq!(copy.name, "solver");
	assert_eq!(copy.counts, vec![1, 2, 3]);
	assert!(
		Rc::ptr_eq(&copy.lookup["one"], &copy.shared),
		"sharing through a `HashMap` value is preserved without the derive knowing about `Rc`"
	);
	assert!(!Rc::ptr_eq(&copy.shared, &shared));
	assert!(!Arc::ptr_eq(&copy.threaded, &original.threaded));
}

#[test]
fn container_bound_override() {
	let copy = Opaque {
		value: Foreign(5),
		shared: Rc::new(RefCell::new(1)),
	}
	.deep_clone();

	assert_eq!(copy.value, Foreign(5));
}

fn double(value: &u32, _cloner: &mut Cloner) -> u32 {
	value * 2
}

#[test]
fn enums_and_generics() {
	let shared = Rc::new(RefCell::new(Tree::Value(7_u32)));
	let original = Tree::Branch {
		label: "root".to_owned(),
		children: vec![Rc::clone(&shared), Rc::clone(&shared)],
	};

	let copy = original.deep_clone();
	let Tree::Branch { label, children } = &copy else {
		panic!("the variant must survive the round trip")
	};

	assert_eq!(label, "root");
	assert!(Rc::ptr_eq(&children[0], &children[1]));
	assert!(!Rc::ptr_eq(&children[0], &shared));

	let _ = Tree::<u32>::Leaf.deep_clone();
	let _ = Tree::Value(1_u32).deep_clone();
}

#[test]
fn field_attributes() {
	let copy = Attributes {
		foreign: Foreign(1),
		doubled: 21,
		scratch: vec![Foreign(9)],
	}
	.deep_clone();

	assert_eq!(copy.foreign, Foreign(1));
	assert_eq!(copy.doubled, 42);
	assert!(
		copy.scratch.is_empty(),
		"`default` ignores the source value"
	);
}

#[test]
fn tuple_structs() {
	let shared = Rc::new(RefCell::new("a".to_owned()));
	let copy = Tuple(1, Rc::clone(&shared), Rc::clone(&shared)).deep_clone();

	assert_eq!(copy.0, 1);
	assert!(Rc::ptr_eq(&copy.1, &copy.2));
	assert!(!Rc::ptr_eq(&copy.1, &shared));
	let _ = Unit.deep_clone();
}

#[test]
fn where_clauses() {
	let shared = Rc::new(RefCell::new(3_u32));
	let copy = Constrained {
		value: 3,
		shared: Rc::clone(&shared),
	}
	.deep_clone();

	assert_eq!(copy.value, 3);
	assert!(!Rc::ptr_eq(&copy.shared, &shared));
}

/// The whole point, end to end: a solver whose propagators share mutable state, cloned through
/// the `Clone` impl its users already call.
mod solver {
	use deepclone::DynDeepClone;

	use super::*;

	#[derive(DeepClone)]
	struct Bound {
		state: Rc<RefCell<u32>>,
		amount: u32,
	}

	trait Propagator: DynDeepClone {
		fn propagate(&self);
	}

	#[derive(DeepClone)]
	struct Solver {
		propagators: Vec<Box<dyn Propagator>>,
		shared: Rc<RefCell<u32>>,
	}

	#[test]
	fn two_propagators_share_state_across_a_solver_clone() {
		let shared = Rc::new(RefCell::new(0));
		let solver = Solver {
			propagators: vec![
				Box::new(Bound {
					state: Rc::clone(&shared),
					amount: 1,
				}),
				Box::new(Bound {
					state: Rc::clone(&shared),
					amount: 10,
				}),
			],
			shared: Rc::clone(&shared),
		};

		let copy = solver.clone();
		for propagator in &copy.propagators {
			propagator.propagate();
		}

		assert_eq!(
			*copy.shared.borrow(),
			11,
			"both propagators wrote to the solver's one new state object"
		);
		assert_eq!(*shared.borrow(), 0, "the original solver is untouched");
	}

	impl Propagator for Bound {
		fn propagate(&self) {
			*self.state.borrow_mut() += self.amount;
		}
	}

	// Coherence permits this alongside `DeepClone` precisely because there is no blanket impl.
	impl Clone for Solver {
		fn clone(&self) -> Self {
			self.deep_clone()
		}
	}
}
