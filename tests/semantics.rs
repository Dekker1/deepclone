//! Tests for what a deep clone must actually do, rather than for what compiles.

// The derive is what makes these shapes writable at all, so there is nothing left to test
// without it.
#![cfg(feature = "derive")]

use std::{
	cell::RefCell,
	rc::{Rc, Weak},
};

use deepclone::{Cloner, DeepClone, DynDeepClone, deep_clone_trait_object};

#[derive(DeepClone)]
struct Diamond {
	left: Rc<RefCell<u32>>,
	right: Rc<RefCell<u32>>,
}

#[test]
fn sharing_within_the_graph_is_preserved() {
	let shared = Rc::new(RefCell::new(1));
	let original = Diamond {
		left: Rc::clone(&shared),
		right: Rc::clone(&shared),
	};

	let copy = original.deep_clone();

	assert!(
		Rc::ptr_eq(&copy.left, &copy.right),
		"the two fields must point at one new object"
	);
	assert!(
		!Rc::ptr_eq(&copy.left, &original.left),
		"the new object must not be the old one"
	);
}

#[test]
fn the_copy_is_independent_of_the_original() {
	let shared = Rc::new(RefCell::new(1));
	let original = Diamond {
		left: Rc::clone(&shared),
		right: Rc::clone(&shared),
	};

	let copy = original.deep_clone();
	*copy.left.borrow_mut() = 2;

	assert_eq!(*copy.right.borrow(), 2, "sharing survives inside the copy");
	assert_eq!(*original.left.borrow(), 1, "the original is untouched");
	assert_eq!(*shared.borrow(), 1);
}

#[test]
fn each_operation_gets_its_own_memo() {
	let shared = Rc::new(RefCell::new(1));
	let original = Diamond {
		left: Rc::clone(&shared),
		right: Rc::clone(&shared),
	};

	let first = original.deep_clone();
	let second = original.deep_clone();

	assert!(
		!Rc::ptr_eq(&first.left, &second.left),
		"a memo must not leak from one clone operation into the next"
	);
}

#[test]
fn one_cloner_can_span_several_values() {
	let shared = Rc::new(RefCell::new(1));
	let mut cloner = Cloner::default();
	let (left, right) = (cloner.rc(&shared), cloner.rc(&shared));

	assert!(Rc::ptr_eq(&left, &right));
	assert!(!Rc::ptr_eq(&left, &shared));
}

#[test]
fn refcount_of_the_source_is_unchanged() {
	let shared = Rc::new(RefCell::new(1));
	let original = Diamond {
		left: Rc::clone(&shared),
		right: Rc::clone(&shared),
	};

	let copy = original.deep_clone();

	assert_eq!(Rc::strong_count(&shared), 3, "one local plus two fields");
	assert_eq!(Rc::strong_count(&copy.left), 2, "the copy's two fields");
}

// A propagator-shaped extension point, which is the motivating case: the values live behind
// trait objects, so the clone has to be dyn-compatible.
trait Propagator: DynDeepClone {
	fn state(&self) -> Rc<RefCell<u32>>;
	fn bump(&self);
}
deep_clone_trait_object!(Propagator);

#[derive(DeepClone)]
struct Counter {
	state: Rc<RefCell<u32>>,
	step: u32,
}

impl Propagator for Counter {
	fn state(&self) -> Rc<RefCell<u32>> {
		Rc::clone(&self.state)
	}

	fn bump(&self) {
		*self.state.borrow_mut() += self.step;
	}
}

#[test]
fn trait_objects_share_one_new_state() {
	let shared = Rc::new(RefCell::new(0));
	let original: Vec<Box<dyn Propagator>> = vec![
		Box::new(Counter {
			state: Rc::clone(&shared),
			step: 1,
		}),
		Box::new(Counter {
			state: Rc::clone(&shared),
			step: 10,
		}),
	];

	let copy = original.deep_clone();
	copy[0].bump();
	copy[1].bump();

	assert!(
		Rc::ptr_eq(&copy[0].state(), &copy[1].state()),
		"two new propagators, one new shared state"
	);
	assert_eq!(
		*copy[0].state().borrow(),
		11,
		"both wrote to that one state"
	);
	assert_eq!(*shared.borrow(), 0, "the original solver is untouched");
}

#[test]
fn auto_trait_variants_of_a_trait_object_are_covered() {
	let original: Vec<Box<dyn Propagator + Send + Sync>> = vec![];
	let copy = original.deep_clone();
	assert!(copy.is_empty());
}

// The idiomatic Rust graph: strong edges down, weak edges back up.
#[derive(DeepClone)]
struct Node {
	value: u32,
	children: Vec<Rc<RefCell<Node>>>,
	parent: Weak<RefCell<Node>>,
}

fn tree() -> Rc<RefCell<Node>> {
	let root = Rc::new(RefCell::new(Node {
		value: 1,
		children: Vec::new(),
		parent: Weak::new(),
	}));
	let child = Rc::new(RefCell::new(Node {
		value: 2,
		children: Vec::new(),
		parent: Rc::downgrade(&root),
	}));
	root.borrow_mut().children.push(child);
	root
}

#[test]
fn weak_back_edges_point_into_the_copy() {
	let original = tree();
	let copy = original.deep_clone();

	let child = Rc::clone(&copy.borrow().children[0]);
	let parent = child
		.borrow()
		.parent
		.upgrade()
		.expect("the copy's back-edge must resolve");

	assert!(
		Rc::ptr_eq(&parent, &copy),
		"back-edge points at the new root"
	);
	assert!(!Rc::ptr_eq(&parent, &original));

	copy.borrow_mut().value = 99;
	assert_eq!(original.borrow().value, 1);
}

#[test]
fn a_weak_visited_before_its_target_still_resolves() {
	// `parent` is declared after `children` in `Node`, so reverse the graph to make the weak
	// edge the first thing the clone encounters.
	#[derive(DeepClone)]
	struct Reversed {
		first: Weak<RefCell<u32>>,
		then: Rc<RefCell<u32>>,
	}

	let target = Rc::new(RefCell::new(7));
	let original = Reversed {
		first: Rc::downgrade(&target),
		then: Rc::clone(&target),
	};

	let copy = original.deep_clone();
	let upgraded = copy.first.upgrade().expect("the weak must resolve");

	assert!(Rc::ptr_eq(&upgraded, &copy.then), "one new target, not two");
	assert!(!Rc::ptr_eq(&upgraded, &target));
}

#[test]
fn a_dangling_weak_clones_to_a_dangling_weak() {
	let source: Weak<RefCell<u32>> = Rc::downgrade(&Rc::new(RefCell::new(1)));
	assert!(source.upgrade().is_none(), "the target dropped immediately");

	let copy = source.deep_clone();
	assert!(copy.upgrade().is_none());
}

#[test]
fn a_weak_only_target_dies_with_the_memo() {
	// Nothing in the copy holds the target strongly, so it must deallocate when the operation
	// ends — exactly as it would have in the source graph.
	let target = Rc::new(RefCell::new(1));
	let source = Rc::downgrade(&target);

	let copy = source.deep_clone();
	assert!(
		copy.upgrade().is_none(),
		"the memo's strong reference must not outlive the operation"
	);
	assert_eq!(Rc::strong_count(&target), 1, "the source is unaffected");
}

#[test]
#[should_panic(expected = "cycle of strong `Rc`/`Arc` edges")]
fn a_strong_cycle_panics_rather_than_overflowing_the_stack() {
	#[derive(DeepClone)]
	struct Looped {
		next: RefCell<Option<Rc<Looped>>>,
	}

	let node = Rc::new(Looped {
		next: RefCell::new(None),
	});
	*node.next.borrow_mut() = Some(Rc::clone(&node));

	let _ = node.deep_clone();

	// Break the cycle so the leak does not outlive the test, though the panic above means this
	// is never reached.
	*node.next.borrow_mut() = None;
}

/// The `unsafe` in `deep_clone_box` reuses the source's vtable, so a heterogeneous collection
/// is the case that would expose a mistake there.
#[test]
fn trait_objects_of_different_concrete_types_keep_their_own_behaviour() {
	#[derive(DeepClone)]
	struct Doubler(Rc<RefCell<u32>>);

	impl Propagator for Doubler {
		fn state(&self) -> Rc<RefCell<u32>> {
			Rc::clone(&self.0)
		}

		fn bump(&self) {
			let doubled = *self.0.borrow() * 2;
			*self.0.borrow_mut() = doubled;
		}
	}

	let shared = Rc::new(RefCell::new(3));
	let original: Vec<Box<dyn Propagator>> = vec![
		Box::new(Counter {
			state: Rc::clone(&shared),
			step: 4,
		}),
		Box::new(Doubler(Rc::clone(&shared))),
	];

	let copy = original.deep_clone();
	copy[0].bump();
	copy[1].bump();

	assert_eq!(*copy[0].state().borrow(), 14, "3 + 4, then doubled");
	assert_eq!(*shared.borrow(), 3);
}
