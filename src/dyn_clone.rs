//! Deep cloning through trait objects.
//!
//! `DeepClone::deep_clone_in` returns `Self`, so it is not dyn-compatible. The way around it
//! is David Tolnay's from `dyn-clone`: a hidden method returns the copy as a thin `*mut ()`,
//! and the caller splices that data pointer into a fat pointer whose metadata it already has.
//!
//! Unlike `dyn-clone`, no macro is needed on top. `Clone` and `Box` are both foreign to that
//! crate, so the orphan rule forces the `Box<dyn Trait>` impl into the caller's crate;
//! [`DeepClone`](crate::DeepClone) is local here, so one blanket impl over [`DynDeepClone`]
//! covers every trait object and every auto-trait variant of it.

use std::ptr;

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
/// The dyn-compatible counterpart of [`DeepClone::deep_clone_in`]. Rarely called directly,
/// since `Box<T>` already routes through it; reach for it when writing an impl for another
/// pointer, such as `Rc<dyn YourTrait>`.
pub fn deep_clone_box<T>(value: &T, cloner: &mut Cloner) -> Box<T>
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
