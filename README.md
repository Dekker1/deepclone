# deepclone

Deep clone that copies shared data once.

`#[derive(Clone)]` on a type holding an `Rc` bumps the reference count, so the "copy" writes
through to the original. A hand-written deep clone goes wrong the other way, duplicating the
pointee at every reference, so what was shared once becomes two separate copies. This crate is
the third behaviour: each object is copied once, every reference to it in the copy points at
that one new object, and the copy shares nothing with the original. It is `copy.deepcopy` with
Python's `memo` argument.

```rust
use std::{cell::RefCell, rc::Rc};
use deepclone::DeepClone;

#[derive(DeepClone)]
struct Diamond {
    left: Rc<RefCell<u32>>,
    right: Rc<RefCell<u32>>,
}

let shared = Rc::new(RefCell::new(1));
let original = Diamond { left: Rc::clone(&shared), right: Rc::clone(&shared) };

let copy = original.deep_clone();

// One new object, reachable from both fields of the copy.
assert!(Rc::ptr_eq(&copy.left, &copy.right));
assert!(!Rc::ptr_eq(&copy.left, &original.left));

// Writing through the copy leaves the original alone.
*copy.left.borrow_mut() = 2;
assert_eq!(*copy.right.borrow(), 2);
assert_eq!(*original.right.borrow(), 1);
```

`Box<dyn Trait>` works too, and so do `Weak` back-edges and the cycles they form. See the
[crate docs](https://docs.rs/deepclone) for those, for the
[limits](https://docs.rs/deepclone#scope-and-what-is-not-supported), and for why there is
deliberately no blanket `impl<T: Clone> DeepClone for T`.

Requires Rust 1.85. The `derive` feature is on by default; the library builds without it.

## Not to be confused with

Two crates share this one's trait name, and one of them has the opposite semantics.

- [`deep-clone`](https://crates.io/crates/deep-clone) (unmaintained since 2022) declares the
  same `DeepClone::deep_clone`, but its `Rc` impl is `Rc::new(self.deref().deep_clone())`. With
  no memo, a diamond becomes two allocations. That is the failure this crate exists to avoid,
  shipped under this crate's trait name.
- [`asajeffrey/deep-clone`](https://github.com/asajeffrey/deep-clone) also spells it
  `DeepClone::deep_clone`, but solves lifetime erasure: an associated `type DeepCloned: 'static`
  turns a `Cow<'a, T>` into a `Cow<'static, T>`.
- [`dyn-clone`](https://crates.io/crates/dyn-clone) makes `Clone` dyn-compatible without
  changing its semantics. This crate borrows its `*mut ()` technique.
- `ImplicitClone` and `dupe` are about cloning *cheaply*, not deeply.
- [`fory`](https://crates.io/crates/fory) tracks `Rc`/`Arc` identity and cycles across a
  serialize and deserialize round-trip, and handles the `Rc<dyn Trait>` this crate does not.
  Worth considering if you already have `Serialize`/`Deserialize` bounds.

## What it costs

Medians from `cargo bench -- --sample-count 1000` on an Apple M4, against `Clone` and against
a deep clone written the obvious way, without a memo.

| | `Clone` | `deep_clone` | naive deep clone |
|---|---|---|---|
| A struct with no shared pointers | 106 ns | 110 ns | n/a |
| 16 nodes sharing a 64-node subtree | 11 ns | **7.6 µs** | 22.2 µs |
| A chain of 256 unique nodes | 6 ns | **26.7 µs** | 5.7 µs |

`Clone` is not a competitor on the last two rows. It bumps a refcount and copies nothing, which
is the bug. Against a real deep clone, memoising wins outright when there is sharing to exploit
and costs about 80 ns per `Rc` when there is none, that being one hash and one insert.

The first row is a wash: data with no shared pointers never touches the memo, and the
element-wise `collect` that rebuilds a `Vec` hits the same `TrustedLen` specialisation `Clone`
does. It is also allocation-dominated, so run that group on its own; benchmark groups
contaminate each other through allocator state enough to flip the sign of a difference that
small.

## Licence

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion
in this crate by you, as defined in the Apache-2.0 licence, shall be dual licensed as above,
without any additional terms or conditions.
