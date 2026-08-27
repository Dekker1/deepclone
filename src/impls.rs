//! [`DeepClone`] impls for the standard library.
//!
//! Split three ways, following CPython's `copy` module: types with nothing to copy deeply,
//! containers that recurse into their contents, and the shared pointers that consult the
//! cloner.

/// Types that reach no shared pointer, so [`Clone::clone`] is already a deep copy.
///
/// A blanket `impl<T: Copy> DeepClone for T` would cover most of these and is sound — `Rc` is
/// not `Copy` — but it overlaps every generic container impl below, since `Option<T>`,
/// `[T; N]`, and tuples are `Copy` when their parameters are.
macro_rules! atomic {
    ($($ty:ty),* $(,)?) => {$(
        impl DeepClone for $ty {
            fn deep_clone_in(&self, _cloner: &mut Cloner) -> Self {
                self.clone()
            }
        }
    )*};
}

/// Atomics have no `Clone`, so this is the one place a value is read rather than cloned.
/// `Relaxed` suffices: a concurrent writer would make any stronger ordering equally
/// arbitrary.
macro_rules! atomics {
    ($($ty:ident),* $(,)?) => {$(
        impl DeepClone for $ty {
            fn deep_clone_in(&self, _cloner: &mut Cloner) -> Self {
                $ty::new(self.load(AtomicOrdering::Relaxed))
            }
        }
    )*};
}

/// `Copy` whenever `T` is, so they need no `T: DeepClone` bound, and not asking for one lets
/// them hold a foreign type that lacks the impl.
macro_rules! copy_wrapper {
	($($ty:ident),*) => {$(
		impl<T: Copy> DeepClone for $ty<T> {
			fn deep_clone_in(&self, _cloner: &mut Cloner) -> Self {
				*self
			}
		}
	)*};
}

/// Byte-backed slices behind a shared pointer, where the copy shares the original's
/// allocation. None can hide interior mutability, so sharing is unobservable and saves the
/// copy, while two fields on one source still reach one copy.
macro_rules! immutable_slice {
	($($ty:ty),* $(,)?) => {$(
		impl DeepClone for Rc<$ty> {
			fn deep_clone_in(&self, _cloner: &mut Cloner) -> Self {
				Rc::clone(self)
			}
		}

		impl DeepClone for Arc<$ty> {
			fn deep_clone_in(&self, _cloner: &mut Cloner) -> Self {
				Arc::clone(self)
			}
		}
	)*};
}

/// Deferred initialisation, which the copy reproduces: an unset source stays unset.
macro_rules! once {
    ($($ty:ident),*) => {$(
        impl<T: DeepClone> DeepClone for $ty<T> {
            fn deep_clone_in(&self, cloner: &mut Cloner) -> Self {
                let copy = $ty::new();
                if let Some(value) = self.get() {
                    let _ = copy.set(value.deep_clone_in(cloner));
                }
                copy
            }
        }
    )*};
}

/// Sequence containers, rebuilt from a mapped iterator — so a `BinaryHeap` copy is
/// re-heapified, which can reorder equal elements.
macro_rules! sequence {
    ($($container:ident<T $(: $bound:ident)?>),* $(,)?) => {$(
        impl<T: DeepClone $(+ $bound)?> DeepClone for $container<T> {
            fn deep_clone_in(&self, cloner: &mut Cloner) -> Self {
                self.iter().map(|value| value.deep_clone_in(cloner)).collect()
            }
        }
    )*};
}

/// The shared pointers, which are the entire point: each consults the cloner, so a source
/// reached twice is copied once.
macro_rules! shared {
    ($($ty:ident<T> => $method:ident),* $(,)?) => {$(
        impl<T: DeepClone + 'static> DeepClone for $ty<T> {
            fn deep_clone_in(&self, cloner: &mut Cloner) -> Self {
                cloner.$method(self)
            }
        }
    )*};
}

/// `[T]` cannot take the shortcut above, since `T` may hold an `Rc` or a `RefCell`, so the
/// copy is built element by element through the cloner.
macro_rules! shared_slice {
	($($strong:ident, $weak:ident => $method:ident),* $(,)?) => {$(
		impl<T: DeepClone + 'static> DeepClone for $strong<[T]> {
			fn deep_clone_in(&self, cloner: &mut Cloner) -> Self {
				cloner.$method(self, |cloner| {
					self.iter().map(|value| value.deep_clone_in(cloner)).collect()
				})
			}
		}

		impl<T: DeepClone + 'static> DeepClone for $weak<[T]> {
			fn deep_clone_in(&self, cloner: &mut Cloner) -> Self {
				let Some(strong) = self.upgrade() else {
					// `Weak::new` needs a `Sized` pointee, so the only way to a dangling
					// `Weak<[T]>` is to downgrade a real allocation and let it drop. The empty
					// slice costs the two counters, held until this `Weak` itself is dropped.
					let empty: $strong<[T]> = $strong::from(Vec::new());
					return $strong::downgrade(&empty);
				};
				$strong::downgrade(&strong.deep_clone_in(cloner))
			}
		}
	)*};
}

/// Tuples, up to the arity the standard library's own traits stop at.
macro_rules! tuples {
    ($(($($name:ident $index:tt),+))*) => {$(
        impl<$($name: DeepClone),+> DeepClone for ($($name,)+) {
            fn deep_clone_in(&self, cloner: &mut Cloner) -> Self {
                ($(self.$index.deep_clone_in(cloner),)+)
            }
        }
    )*};
}

use std::{
	any::TypeId,
	borrow::Cow,
	cell::{Cell, OnceCell, RefCell},
	cmp::{Ordering, Reverse},
	collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet, LinkedList, VecDeque},
	convert::Infallible,
	ffi::{CStr, CString, OsStr, OsString},
	hash::{BuildHasher, Hash},
	io::ErrorKind,
	marker::{PhantomData, PhantomPinned},
	net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6},
	num::{
		NonZeroI8, NonZeroI16, NonZeroI32, NonZeroI64, NonZeroI128, NonZeroIsize, NonZeroU8,
		NonZeroU16, NonZeroU32, NonZeroU64, NonZeroU128, NonZeroUsize, Saturating, Wrapping,
	},
	ops::{
		Bound, ControlFlow, Range, RangeFrom, RangeFull, RangeInclusive, RangeTo, RangeToInclusive,
	},
	path::{Path, PathBuf},
	rc::{Rc, Weak as RcWeak},
	sync::{
		Arc, Mutex, OnceLock, RwLock, Weak as ArcWeak,
		atomic::{
			AtomicBool, AtomicI8, AtomicI16, AtomicI32, AtomicI64, AtomicIsize, AtomicU8,
			AtomicU16, AtomicU32, AtomicU64, AtomicUsize, Ordering as AtomicOrdering,
		},
	},
	time::{Duration, Instant, SystemTime},
};

use crate::{Cloner, DeepClone, DynDeepClone, dyn_clone::deep_clone_box};

impl<K: DeepClone + Ord, V: DeepClone> DeepClone for BTreeMap<K, V> {
	fn deep_clone_in(&self, cloner: &mut Cloner) -> Self {
		self.iter()
			.map(|(key, value)| (key.deep_clone_in(cloner), value.deep_clone_in(cloner)))
			.collect()
	}
}

impl<T: DeepClone> DeepClone for Bound<T> {
	fn deep_clone_in(&self, cloner: &mut Cloner) -> Self {
		match self {
			Bound::Included(value) => Bound::Included(value.deep_clone_in(cloner)),
			Bound::Excluded(value) => Bound::Excluded(value.deep_clone_in(cloner)),
			Bound::Unbounded => Bound::Unbounded,
		}
	}
}

/// Every `Box` takes the dyn-compatible path, so `Box<dyn Trait>` and its auto-trait variants
/// need nothing from the trait's author but the supertrait bound.
impl<T: ?Sized + DynDeepClone> DeepClone for Box<T> {
	fn deep_clone_in(&self, cloner: &mut Cloner) -> Self {
		deep_clone_box(&**self, cloner)
	}
}

/// `[T]` has no [`DeepClone`] impl and so no [`DynDeepClone`] one either, which is what keeps
/// this from colliding with the blanket `Box` impl above.
impl<T: DeepClone> DeepClone for Box<[T]> {
	fn deep_clone_in(&self, cloner: &mut Cloner) -> Self {
		self.iter()
			.map(|value| value.deep_clone_in(cloner))
			.collect()
	}
}

impl<T: Copy> DeepClone for Cell<T> {
	fn deep_clone_in(&self, _cloner: &mut Cloner) -> Self {
		Cell::new(self.get())
	}
}

impl<B: DeepClone, C: DeepClone> DeepClone for ControlFlow<B, C> {
	fn deep_clone_in(&self, cloner: &mut Cloner) -> Self {
		match self {
			ControlFlow::Continue(value) => ControlFlow::Continue(value.deep_clone_in(cloner)),
			ControlFlow::Break(value) => ControlFlow::Break(value.deep_clone_in(cloner)),
		}
	}
}

impl<B: ToOwned + ?Sized> DeepClone for Cow<'_, B>
where
	B::Owned: DeepClone,
{
	fn deep_clone_in(&self, cloner: &mut Cloner) -> Self {
		// Borrowed data is not owned by the source, so there is nothing to copy independently.
		match self {
			Cow::Borrowed(value) => Cow::Borrowed(value),
			Cow::Owned(value) => Cow::Owned(value.deep_clone_in(cloner)),
		}
	}
}

/// Rebuilt by `collect`, with the same caveat as [`HashSet`].
impl<K: DeepClone + Eq + Hash, V: DeepClone, S: BuildHasher + Default> DeepClone
	for HashMap<K, V, S>
{
	fn deep_clone_in(&self, cloner: &mut Cloner) -> Self {
		self.iter()
			.map(|(key, value)| (key.deep_clone_in(cloner), value.deep_clone_in(cloner)))
			.collect()
	}
}

/// Rebuilt by `collect`, so the copy re-hashes with `S::default()` and may iterate in a
/// different order. Only observable with a stateful `S`.
impl<T: DeepClone + Eq + Hash, S: BuildHasher + Default> DeepClone for HashSet<T, S> {
	fn deep_clone_in(&self, cloner: &mut Cloner) -> Self {
		self.iter()
			.map(|value| value.deep_clone_in(cloner))
			.collect()
	}
}

impl<T: DeepClone> DeepClone for Mutex<T> {
	fn deep_clone_in(&self, cloner: &mut Cloner) -> Self {
		Mutex::new(
			self.lock()
				.expect("cannot deep clone through a poisoned `Mutex`")
				.deep_clone_in(cloner),
		)
	}
}

impl<T: DeepClone> DeepClone for Option<T> {
	fn deep_clone_in(&self, cloner: &mut Cloner) -> Self {
		self.as_ref().map(|value| value.deep_clone_in(cloner))
	}
}

/// Unconditional in `T`, since the derive bounds every type parameter by `DeepClone` and a
/// marker-only parameter could not satisfy that.
impl<T: ?Sized> DeepClone for PhantomData<T> {
	fn deep_clone_in(&self, _cloner: &mut Cloner) -> Self {
		PhantomData
	}
}

impl<T: DeepClone> DeepClone for Range<T> {
	fn deep_clone_in(&self, cloner: &mut Cloner) -> Self {
		self.start.deep_clone_in(cloner)..self.end.deep_clone_in(cloner)
	}
}

impl<T: DeepClone> DeepClone for RangeFrom<T> {
	fn deep_clone_in(&self, cloner: &mut Cloner) -> Self {
		self.start.deep_clone_in(cloner)..
	}
}

/// Rebuilt from its bounds, which resets the exhaustion an iterated `RangeInclusive` tracks
/// privately and `Clone` preserves. There is no public way to reproduce it.
impl<T: DeepClone> DeepClone for RangeInclusive<T> {
	fn deep_clone_in(&self, cloner: &mut Cloner) -> Self {
		self.start().deep_clone_in(cloner)..=self.end().deep_clone_in(cloner)
	}
}

impl<T: DeepClone> DeepClone for RangeTo<T> {
	fn deep_clone_in(&self, cloner: &mut Cloner) -> Self {
		..self.end.deep_clone_in(cloner)
	}
}

impl<T: DeepClone> DeepClone for RangeToInclusive<T> {
	fn deep_clone_in(&self, cloner: &mut Cloner) -> Self {
		..=self.end.deep_clone_in(cloner)
	}
}

impl<T: DeepClone> DeepClone for RefCell<T> {
	fn deep_clone_in(&self, cloner: &mut Cloner) -> Self {
		RefCell::new(self.borrow().deep_clone_in(cloner))
	}
}

impl<T: DeepClone, E: DeepClone> DeepClone for Result<T, E> {
	fn deep_clone_in(&self, cloner: &mut Cloner) -> Self {
		match self {
			Ok(value) => Ok(value.deep_clone_in(cloner)),
			Err(error) => Err(error.deep_clone_in(cloner)),
		}
	}
}

impl<T: DeepClone> DeepClone for Reverse<T> {
	fn deep_clone_in(&self, cloner: &mut Cloner) -> Self {
		Reverse(self.0.deep_clone_in(cloner))
	}
}

impl<T: DeepClone> DeepClone for RwLock<T> {
	fn deep_clone_in(&self, cloner: &mut Cloner) -> Self {
		RwLock::new(
			self.read()
				.expect("cannot deep clone through a poisoned `RwLock`")
				.deep_clone_in(cloner),
		)
	}
}

impl<T: DeepClone, const N: usize> DeepClone for [T; N] {
	fn deep_clone_in(&self, cloner: &mut Cloner) -> Self {
		std::array::from_fn(|index| self[index].deep_clone_in(cloner))
	}
}

immutable_slice!(str, CStr, OsStr, Path);

shared_slice!(Rc, RcWeak => rc_unsized, Arc, ArcWeak => arc_unsized);

atomic! {
	(), bool, char, Infallible, Ordering, PhantomPinned, RangeFull, TypeId,
	i8, i16, i32, i64, i128, isize,
	u8, u16, u32, u64, u128, usize,
	f32, f64,
	NonZeroI8, NonZeroI16, NonZeroI32, NonZeroI64, NonZeroI128, NonZeroIsize,
	NonZeroU8, NonZeroU16, NonZeroU32, NonZeroU64, NonZeroU128, NonZeroUsize,
	Duration, Instant, SystemTime,
	IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6,
	ErrorKind,
	String, PathBuf, OsString, CString,
	Box<str>, Box<Path>, Box<OsStr>, Box<CStr>,
}

copy_wrapper!(Wrapping, Saturating);

sequence!(Vec<T>, VecDeque<T>, LinkedList<T>, BTreeSet<T: Ord>, BinaryHeap<T: Ord>);

once!(OnceCell, OnceLock);

atomics! {
	AtomicBool,
	AtomicI8, AtomicI16, AtomicI32, AtomicI64, AtomicIsize,
	AtomicU8, AtomicU16, AtomicU32, AtomicU64, AtomicUsize,
}

shared! {
	Rc<T> => rc,
	Arc<T> => arc,
	RcWeak<T> => rc_weak,
	ArcWeak<T> => arc_weak,
}

tuples! {
	(A 0)
	(A 0, B 1)
	(A 0, B 1, C 2)
	(A 0, B 1, C 2, D 3)
	(A 0, B 1, C 2, D 3, E 4)
	(A 0, B 1, C 2, D 3, E 4, F 5)
	(A 0, B 1, C 2, D 3, E 4, F 5, G 6)
	(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7)
	(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8)
	(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9)
	(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10)
	(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10, L 11)
}
