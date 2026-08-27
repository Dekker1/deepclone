# deepclone

Deep clone that copies shared data once. If two places in the source pointed at one `Rc` or
`Arc`, the two corresponding places in the copy point at one *new* one — and the copy shares
nothing with the original. This is `copy.deepcopy(x, memo)` from Python.

`#[derive(Clone)]` gets this wrong by bumping the reference count, so the "copy" writes
through to the original. A hand-written deep clone gets it wrong the other way, duplicating
the pointee at every reference, so what was shared once is now two separate copies.

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
[crate docs](https://docs.rs/deepclone) for those, for the limits, and for why there is
deliberately no blanket `impl<T: Clone> DeepClone for T`.

## Licence

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion
in this crate by you, as defined in the Apache-2.0 licence, shall be dual licensed as above,
without any additional terms or conditions.
