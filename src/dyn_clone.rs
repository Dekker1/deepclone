//! Deep cloning through trait objects.
//!
//! `DeepClone::deep_clone_in` returns `Self`, so it is not dyn-compatible. The way around it
//! is David Tolnay's from `dyn-clone`: a hidden method returns the copy as a thin `*mut ()`,
//! and the caller splices that data pointer into a fat pointer whose metadata it already has.
//!
//! `dyn-clone` needs a macro on top of that, because `Clone` is foreign to it and the orphan
//! rule forces the `Box<dyn Trait>` impl into the caller's crate. Ours is local, so one blanket
//! impl over [`DynDeepClone`] covers every trait object and auto-trait variant.

use std::{ptr, rc::Rc, sync::Arc};

use crate::{
	Cloner, DeepClone,
	dyn_clone::sealed::{Private, Sealed},
};

/// Supertrait that lets a trait object be deep cloned.
///
/// Implemented for every [`DeepClone`] type; you never write an impl. Naming it as a
/// supertrait is all a trait needs for `Box<dyn YourTrait>` to deep clone.
pub trait DynDeepClone: Sealed {
	/// Not public API.
	#[doc(hidden)]
	fn __deep_clone_box(&self, cloner: &mut Cloner, _: Private) -> *mut ();
}

/// Deep clone a possibly unsized value into a `Box`, threading `cloner` through.
///
/// The dyn-compatible counterpart of [`DeepClone::deep_clone_in`], and the crate's only
/// `unsafe`. Reached through `Box`'s impl and the unsized helpers.
pub(crate) fn deep_clone_box<T>(value: &T, cloner: &mut Cloner) -> Box<T>
where
	T: ?Sized + DynDeepClone,
{
	let mut fat_ptr = ptr::from_ref(value);
	// SAFETY: `__deep_clone_box` is sealed, and its only impl returns `Box::into_raw` of a
	// `Box<Concrete>` for the erased type behind `T`. Overwriting only the data half of the
	// fat pointer therefore pairs that allocation with the metadata `value` already carried,
	// which is correct because the copy has the same concrete type as the source. The
	// assertion pins the assumption that the data pointer is the fat pointer's first word.
	unsafe {
		let data_ptr = ptr::addr_of_mut!(fat_ptr).cast::<*mut ()>();
		assert_eq!(*data_ptr as *const (), ptr::from_ref(value).cast::<()>());
		*data_ptr = T::__deep_clone_box(value, cloner, Private);
	}
	// SAFETY: `fat_ptr` now describes the `Box` allocated above, not reachable anywhere else.
	unsafe { Box::from_raw(fat_ptr as *mut T) }
}

/// Deep clone an `Arc<dyn YourTrait>`, as [`deep_clone_unsized_rc`] does for `Rc`.
pub fn deep_clone_unsized_arc<T>(src: &Arc<T>, cloner: &mut Cloner) -> Arc<T>
where
	T: ?Sized + DynDeepClone + 'static,
{
	cloner.arc_unsized(src, |cloner| Arc::from(deep_clone_box(&**src, cloner)))
}

/// Deep clone an `Rc<dyn YourTrait>`.
///
/// `Rc` is not `#[fundamental]`, so neither this crate nor yours may write
/// `impl DeepClone for Rc<dyn YourTrait>`. Name this at the field instead:
///
/// ```
/// # use std::rc::Rc;
/// use deepclone::{DeepClone, DynDeepClone};
///
/// trait Propagator: DynDeepClone {}
///
/// #[derive(DeepClone)]
/// struct Solver {
///     #[deepclone(with = deepclone::deep_clone_unsized_rc)]
///     propagator: Rc<dyn Propagator>,
/// }
/// ```
///
/// For unsized pointees only. A sized `Rc<T>` already deep clones on its own, through a path
/// that supports cycles, which this one cannot.
pub fn deep_clone_unsized_rc<T>(src: &Rc<T>, cloner: &mut Cloner) -> Rc<T>
where
	T: ?Sized + DynDeepClone + 'static,
{
	cloner.rc_unsized(src, |cloner| Rc::from(deep_clone_box(&**src, cloner)))
}

impl<T: DeepClone> DynDeepClone for T {
	fn __deep_clone_box(&self, cloner: &mut Cloner, _: Private) -> *mut () {
		Box::<T>::into_raw(Box::new(self.deep_clone_in(cloner))).cast::<()>()
	}
}

/// Machinery that keeps [`DynDeepClone`] closed to outside impls.
#[expect(unnameable_types, reason = "being unnameable is what seals the trait")]
mod sealed {
	/// Makes the hidden trait method uncallable from outside this crate.
	#[derive(Debug)]
	pub struct Private;

	/// Prevents a hand-written [`DynDeepClone`](super::DynDeepClone) impl, which could return
	/// a pointer to something other than a `Box<Self>` and make `deep_clone_box` unsound.
	pub trait Sealed {}

	impl<T: crate::DeepClone> Sealed for T {}
}
