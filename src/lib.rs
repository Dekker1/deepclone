//! Deep clone that copies shared data once.
//!
//! Rust gives you two behaviours for cloning an object graph that contains `Rc` or `Arc`,
//! and neither one is "an independent copy of the whole graph":
//!
//! - [`#[derive(Clone)]`][Clone] bumps the reference count. The "copy" shares mutable state
//!   with the original, which is usually a silent bug.
//! - A hand-written deep clone duplicates the pointee at every reference. Internal sharing is
//!   destroyed: two fields that pointed at one object now point at two.
//!
//! This crate is the third behaviour: each object is copied once, and every reference to it
//! in the copy points at that one new object. It is `copy.deepcopy` from Python, and
//! algorithmically a copying garbage collector's forwarding table.
//!
//! Serde documents the same gap: its `rc` feature warns that these types "do not preserve
//! identity and may result in multiple copies of the same data".
//!
//! # The footgun
//!
//! ```
//! # use std::{cell::RefCell, rc::Rc};
//! #[derive(Clone)] // Looks harmless. It is not.
//! struct Solver {
//!     shared: Rc<RefCell<u32>>,
//! }
//!
//! let original = Solver {
//!     shared: Rc::new(RefCell::new(0)),
//! };
//! let copy = original.clone();
//! *copy.shared.borrow_mut() = 42;
//!
//! // The "independent" copy wrote through to the original.
//! assert_eq!(*original.shared.borrow(), 42);
//! ```
//!
//! `Rc<T>` implements [`Clone`], so `derive(Clone)` produces a copy that goes on sharing with
//! the original, and nothing warns you. Deriving [`DeepClone`] instead routes every `Rc`
//! through the cloner:
//!
//! ```
//! # use std::{cell::RefCell, rc::Rc};
//! use deepclone::DeepClone;
//!
//! #[derive(DeepClone)]
//! struct Solver {
//!     shared: Rc<RefCell<u32>>,
//! }
//!
//! let original = Solver {
//!     shared: Rc::new(RefCell::new(0)),
//! };
//! let copy = original.deep_clone();
//! *copy.shared.borrow_mut() = 42;
//!
//! assert_eq!(*original.shared.borrow(), 0);
//! ```
//!
//! # Sharing is preserved
//!
//! A field-by-field deep clone gets this part wrong. Two fields on one `Rc` must still be on
//! one `Rc` after the copy — a *new* one:
//!
//! ```
//! # use std::{cell::RefCell, rc::Rc};
//! use deepclone::DeepClone;
//!
//! #[derive(DeepClone)]
//! struct Diamond {
//!     left: Rc<RefCell<u32>>,
//!     right: Rc<RefCell<u32>>,
//! }
//!
//! let shared = Rc::new(RefCell::new(1));
//! let original = Diamond {
//!     left: Rc::clone(&shared),
//!     right: Rc::clone(&shared),
//! };
//! let copy = original.deep_clone();
//!
//! // One new object, reachable from both fields of the copy.
//! assert!(Rc::ptr_eq(&copy.left, &copy.right));
//! assert!(!Rc::ptr_eq(&copy.left, &original.left));
//!
//! *copy.left.borrow_mut() = 2;
//! assert_eq!(*copy.right.borrow(), 2);
//! assert_eq!(*original.right.borrow(), 1);
//! ```
//!
//! # There is deliberately no blanket impl
//!
//! `impl<T: Clone> DeepClone for T` would reintroduce the footgun, since `Rc<T>: Clone` and
//! without specialization such an impl could not be overridden for `Rc`.
//!
//! So a field whose type has no [`DeepClone`] impl is a compile error. Opt out per field with
//! `#[deepclone(clone)]`.
//!
//! # Trait objects
//!
//! [`DeepClone::deep_clone_in`] returns `Self`, so it is not dyn-compatible and cloning
//! through a `Box<dyn Trait>` needs a boxed variant. Add [`DynDeepClone`] as a supertrait and
//! invoke [`deep_clone_trait_object!`]:
//!
//! ```
//! # use std::{cell::RefCell, rc::Rc};
//! use deepclone::{DeepClone, DynDeepClone, deep_clone_trait_object};
//!
//! trait Propagator: DynDeepClone {
//!     fn state(&self) -> Rc<RefCell<u32>>;
//! }
//! deep_clone_trait_object!(Propagator);
//!
//! #[derive(DeepClone)]
//! struct Counter(Rc<RefCell<u32>>);
//! impl Propagator for Counter {
//!     fn state(&self) -> Rc<RefCell<u32>> {
//!         Rc::clone(&self.0)
//!     }
//! }
//!
//! let shared = Rc::new(RefCell::new(0));
//! let original: Vec<Box<dyn Propagator>> = vec![
//!     Box::new(Counter(Rc::clone(&shared))),
//!     Box::new(Counter(Rc::clone(&shared))),
//! ];
//! let copy = original.deep_clone();
//!
//! // Two new propagators, sharing one new state object.
//! assert!(Rc::ptr_eq(&copy[0].state(), &copy[1].state()));
//! assert!(!Rc::ptr_eq(&copy[0].state(), &shared));
//! ```
//!
//! # Cycles
//!
//! Strong `Rc` edges downwards with [`Weak`](std::rc::Weak) back-edges upwards clones
//! correctly, cycles included: a `Weak` needs only its target's *identity*, and
//! [`Rc::new_cyclic`] reserves the copy's allocation before the pointee is built.
//!
//! A cycle of *strong* edges panics instead of overflowing the stack. It leaks in the original
//! too, so it is a bug in the source.
//!
//! # Scope, and what is not supported
//!
//! One [`Cloner`] is one clone operation, and [`DeepClone::deep_clone`] makes a fresh one per
//! call. Reusing one for an unrelated clone is the one way to make two copies share again.
//!
//! - [`Cloner::rc`] and friends need `T: Sized + 'static`, so `Rc<dyn Trait>`, `Rc<[T]>`, and
//!   `Rc<str>` are not tracked. Use `#[deepclone(clone)]` for those.
//! - That `'static` propagates from [`TypeId`]: a generic type with an `Rc<..T..>` field needs
//!   `T: 'static` on its own declaration, which the derive does not add for you.
//! - [`Cloner`] is neither `Send` nor `Sync`, so [`Cloner::arc`] is single-threaded.
//! - Peak memory holds both copies, since the [`Cloner`] keeps every copy alive.
//! - A [`RefCell`](std::cell::RefCell) already mutably borrowed panics on `borrow()`, as does
//!   a poisoned `Mutex` or `RwLock`.
//! - Do not mutate the source from inside a [`DeepClone`] impl. Objects are keyed by address,
//!   which is unambiguous only because every source stays alive throughout.
//!
//! The derive is behind the default `derive` feature; the library builds without it.
//!
//! # Prior art
//!
//! The `*mut ()` round-trip in [`deep_clone_box`] is David Tolnay's, from `dyn-clone`, and
//! `oxc_allocator::CloneIn` is the precedent for a context-carrying clone trait. The README
//! covers how this differs from the similarly named crates.

/// Generate the cloner's methods for one flavour of shared pointer.
///
/// `Rc` and `Arc` differ only in their type names here, but they are distinct types with
/// distinct `Weak` companions, so the pair cannot be written generically.
macro_rules! shared_ptr_methods {
	($strong:ident, $weak:ident, $strong_fn:ident, $weak_fn:ident) => {
		impl Cloner {
			#[doc = concat!("Deep clone `", stringify!($strong), "<T>` through the cloner.")]
			///
			/// The first call for a given source allocates one copy and records it; later
			/// calls return another handle to that same copy.
			///
			/// # Panics
			///
			/// If the source contains a cycle of strong edges, which would leak in the source
			/// too. `Weak` back-edges are fine.
			pub fn $strong_fn<T: DeepClone + 'static>(&mut self, src: &$strong<T>) -> $strong<T> {
				let key = (
					TypeId::of::<T>(),
					$strong::as_ptr(src).cast::<()>() as usize,
				);
				match self.memo.get(&key) {
					Some(Entry::Done(copy)) => return $strong::clone(stored(&**copy)),
					Some(Entry::InProgress(_)) => strong_cycle::<T>(),
					None => {}
				}
				let copy = $strong::new_cyclic(|shell: &$weak<T>| {
					// Recorded before recursing, so a `Weak` back-edge reached while building
					// the pointee resolves to this very allocation.
					let _ = self
						.memo
						.insert(key, Entry::InProgress(Box::new(shell.clone())));
					// Not `src.deep_clone_in(..)`, which would resolve to the impl on the
					// pointer type itself and recurse forever.
					(**src).deep_clone_in(self)
				});
				let _ = self
					.memo
					.insert(key, Entry::Done(Box::new($strong::clone(&copy))));
				copy
			}

			#[doc = concat!("Deep clone `", stringify!($weak), "<T>` through the cloner.")]
			///
			/// A live source upgrades, clones through
			#[doc = concat!("[`", stringify!($strong_fn), "`](Self::", stringify!($strong_fn), ")")]
			/// like any other shared pointer, and downgrades again, so the answer does not
			/// depend on whether the target was reached strongly first. An already-dangling
			/// source clones to a dangling `Weak`.
			///
			/// The `Cloner` holds the target strongly until it drops, so a weak-only target
			/// survives long enough to be pointed at, then deallocates unless the copy holds
			/// it. A source whose only strong owner sits outside what was cloned therefore
			/// copies to a dangling `Weak`.
			pub fn $weak_fn<T: DeepClone + 'static>(&mut self, src: &$weak<T>) -> $weak<T> {
				let Some(strong) = src.upgrade() else {
					return $weak::new();
				};
				let key = (
					TypeId::of::<T>(),
					$strong::as_ptr(&strong).cast::<()>() as usize,
				);
				if let Some(entry) = self.memo.get(&key) {
					return match entry {
						Entry::Done(copy) => $strong::downgrade(stored(&**copy)),
						Entry::InProgress(shell) => stored::<$weak<T>>(&**shell).clone(),
					};
				}
				$strong::downgrade(&self.$strong_fn(&strong))
			}
		}
	};
}

mod dyn_clone;
mod impls;

use std::{
	any::{Any, TypeId, type_name},
	fmt,
	rc::{Rc, Weak as RcWeak},
	sync::{Arc, Weak as ArcWeak},
};

#[cfg(feature = "derive")]
pub use deepclone_derive::DeepClone;
use rustc_hash::FxHashMap;

#[doc(hidden)]
pub use crate::dyn_clone::__private;
pub use crate::dyn_clone::{DynDeepClone, deep_clone_box};

/// The forwarding table for one deep clone operation, mapping each source object's identity
/// to its copy.
///
/// [`DeepClone::deep_clone`] makes one per call, which is what you want. Construct one
/// yourself only to clone several values against *one* `Cloner`:
///
/// ```
/// # use std::{cell::RefCell, rc::Rc};
/// use deepclone::{Cloner, DeepClone};
///
/// let shared = Rc::new(RefCell::new(1));
/// let mut cloner = Cloner::default();
/// let (left, right) = (
///     shared.deep_clone_in(&mut cloner),
///     shared.deep_clone_in(&mut cloner),
/// );
///
/// assert!(Rc::ptr_eq(&left, &right));
/// ```
///
/// Reusing one for an unrelated clone makes those two share copies, which is the bug this
/// crate exists to prevent.
#[derive(Default)]
pub struct Cloner {
	/// The address alone would very likely do, since only whole heap allocations are keyed and
	/// two live ones are disjoint, but the `TypeId` makes the downcasts below correct by
	/// construction rather than by an argument about `Rc`'s private layout.
	///
	/// FxHash because the keys are addresses, not attacker-chosen, and `SipHash` costs about
	/// 30% of the per-`Rc` price.
	memo: FxHashMap<(TypeId, usize), Entry>,
}

/// A clone that copies everything reachable, preserving the sharing among the copies: two
/// references to one object become two references to one *new* object, and the copy shares
/// nothing with the original.
///
/// Derive it. A hand-written impl must thread `cloner` into every field, since that is the
/// only thing keeping shared objects shared.
#[diagnostic::on_unimplemented(
	message = "`{Self}` cannot be deep cloned",
	label = "no `DeepClone` impl",
	note = "derive `DeepClone` on `{Self}` if you own it, or mark the field \
	        `#[deepclone(clone)]` if a shallow clone is correct for it",
	note = "`Rc<[T]>`, `Rc<str>`, and `Rc<dyn Trait>` land here by design: `Cloner::rc` keys the \
	        cloner on `TypeId`, so it needs `T: Sized + 'static`. Use `#[deepclone(clone)]` for \
	        them"
)]
pub trait DeepClone {
	/// Deep clone `self`. This is the method to call.
	///
	/// Each call gets its own [`Cloner`], so two calls never share copies with each other.
	fn deep_clone(&self) -> Self
	where
		Self: Sized,
	{
		// The `Cloner` outlives the result's construction, so a copy that nothing else holds
		// strongly deallocates only once this returns.
		self.deep_clone_in(&mut Cloner::default())
	}

	/// Deep clone `self` through `cloner`. This is the method to implement, and the one to
	/// call from inside another impl so that the whole copy shares one `Cloner`.
	fn deep_clone_in(&self, cloner: &mut Cloner) -> Self;
}

/// A memo table entry, which needs two states so that back-edges can be resolved.
enum Entry {
	/// A copy whose allocation `new_cyclic` has reserved but not yet initialised. Holds a
	/// `Weak` to it, which is all a back-edge needs.
	InProgress(Box<dyn Any>),
	/// A finished copy, held strongly so that it stays alive for the rest of the operation
	/// even if nothing in the copy points at it yet.
	Done(Box<dyn Any>),
}

/// Read a memo entry back out at the type its key promises.
fn stored<T: 'static>(entry: &dyn Any) -> &T {
	entry
		.downcast_ref::<T>()
		.expect("memo entries are stored under a key carrying their own TypeId")
}

/// Report a cycle of strong edges, which cannot be copied and would have leaked anyway.
#[cold]
fn strong_cycle<T>() -> ! {
	panic!(
		"deep clone reached a cycle of strong `Rc`/`Arc` edges through `{}`; the copy of that \
		 object does not exist yet, so there is nothing to point at. Such a cycle also leaks \
		 in the original — use `Weak` for back-edges, which this crate does support.",
		type_name::<T>(),
	)
}

impl fmt::Debug for Cloner {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		// The entries are `Box<dyn Any>`, so their count is all there is to report.
		f.debug_struct("Cloner")
			.field("copies", &self.memo.len())
			.finish()
	}
}

shared_ptr_methods!(Rc, RcWeak, rc, rc_weak);
shared_ptr_methods!(Arc, ArcWeak, arc, arc_weak);
