//! Deep cloning through trait objects.
//!
//! `DeepClone::deep_clone_in` returns `Self`, so it is not dyn-compatible. The way around it
//! is David Tolnay's from `dyn-clone`: a hidden method returns the copy as a thin `*mut ()`,
//! and the caller splices that data pointer into a fat pointer whose metadata it already has.
//! A blanket impl instead runs into an unsize-coercion bound that creates a supertrait
//! cycle.

use std::ptr;

use crate::{Cloner, DeepClone};

/// Not public API.
#[doc(hidden)]
pub mod __private {
	pub use std::{
		boxed::Box,
		marker::{Send, Sync},
	};
}

mod sealed {
	/// Prevents a hand-written [`DynDeepClone`](super::DynDeepClone) impl, which could return
	/// a pointer to something other than a `Box<Self>` and make `deep_clone_box` unsound.
	pub trait Sealed {}
	impl<T: crate::DeepClone> Sealed for T {}

	/// Makes the hidden trait method uncallable from outside this crate.
	pub struct Private;
}
use crate::dyn_clone::sealed::{Private, Sealed};

/// Supertrait that lets a trait object be deep cloned.
///
/// Implemented for every [`DeepClone`] type; you never write an impl. Add it as a supertrait
/// of your own trait, then invoke
/// [`deep_clone_trait_object!`](crate::deep_clone_trait_object).
pub trait DynDeepClone: Sealed {
	/// Not public API.
	#[doc(hidden)]
	fn __deep_clone_box(&self, cloner: &mut Cloner, _: Private) -> *mut ();
}

impl<T: DeepClone> DynDeepClone for T {
	fn __deep_clone_box(&self, cloner: &mut Cloner, _: Private) -> *mut () {
		Box::<T>::into_raw(Box::new(self.deep_clone_in(cloner))).cast::<()>()
	}
}

/// Deep clone a possibly unsized value into a `Box`, threading `cloner`'s memo.
///
/// The dyn-compatible counterpart of [`DeepClone::deep_clone_in`]. Rarely called directly:
/// [`deep_clone_trait_object!`](crate::deep_clone_trait_object) wraps it into the impl you
/// actually want.
pub fn deep_clone_box<T>(value: &T, cloner: &mut Cloner) -> Box<T>
where
	T: ?Sized + DynDeepClone,
{
	let mut fat_ptr = value as *const T;
	// SAFETY: `__deep_clone_box` is sealed, and its only impl returns `Box::into_raw` of a
	// `Box<Concrete>` for the erased type behind `T`. Overwriting only the data half of the
	// fat pointer therefore pairs that allocation with the metadata `value` already carried,
	// which is correct because the copy has the same concrete type as the source. The
	// assertion pins the assumption that the data pointer is the fat pointer's first word.
	unsafe {
		let data_ptr = ptr::addr_of_mut!(fat_ptr).cast::<*mut ()>();
		assert_eq!(*data_ptr as *const (), (value as *const T).cast::<()>());
		*data_ptr = T::__deep_clone_box(value, cloner, Private);
	}
	// SAFETY: `fat_ptr` now describes the `Box` allocated above, not reachable anywhere else.
	unsafe { Box::from_raw(fat_ptr as *mut T) }
}

/// Implement [`DeepClone`] for `Box<dyn YourTrait>`.
///
/// `YourTrait` must have [`DynDeepClone`] as a supertrait. The `+ Send`, `+ Sync`, and
/// `+ Send + Sync` variants of the trait object are covered too.
///
/// ```
/// use deepclone::{DynDeepClone, deep_clone_trait_object};
///
/// trait Propagator: DynDeepClone {}
/// deep_clone_trait_object!(Propagator);
/// ```
///
/// Type parameters and where-clauses are supported, spelled as in `dyn-clone`:
///
/// ```
/// use deepclone::{DynDeepClone, deep_clone_trait_object};
/// use std::fmt::Debug;
///
/// trait Awkward<T>: DynDeepClone where T: Debug {}
/// deep_clone_trait_object!(<T> Awkward<T> where T: Debug);
/// ```
#[macro_export]
macro_rules! deep_clone_trait_object {
    ($($path:tt)+) => {
        $crate::__internal_deep_clone_trait_object!(begin $($path)+);
    };
}

/// Not public API.
///
/// Splits `<generics> Path<..> where ..` into its three parts one token at a time, because
/// `macro_rules!` cannot match a generic parameter list as a fragment.
#[doc(hidden)]
#[macro_export]
macro_rules! __internal_deep_clone_trait_object {
    // Invocation started with `<`, so parse generics.
    (begin < $($rest:tt)*) => {
        $crate::__internal_deep_clone_trait_object!(generics () () $($rest)*);
    };

    // Invocation did not start with `<`.
    (begin $first:tt $($rest:tt)*) => {
        $crate::__internal_deep_clone_trait_object!(path () ($first) $($rest)*);
    };

    // End of generics.
    (generics ($($generics:tt)*) () > $($rest:tt)*) => {
        $crate::__internal_deep_clone_trait_object!(path ($($generics)*) () $($rest)*);
    };

    // Generics open bracket.
    (generics ($($generics:tt)*) ($($brackets:tt)*) < $($rest:tt)*) => {
        $crate::__internal_deep_clone_trait_object!(generics ($($generics)* <) ($($brackets)* <) $($rest)*);
    };

    // Generics close bracket.
    (generics ($($generics:tt)*) (< $($brackets:tt)*) > $($rest:tt)*) => {
        $crate::__internal_deep_clone_trait_object!(generics ($($generics)* >) ($($brackets)*) $($rest)*);
    };

    // Token inside of generics.
    (generics ($($generics:tt)*) ($($brackets:tt)*) $first:tt $($rest:tt)*) => {
        $crate::__internal_deep_clone_trait_object!(generics ($($generics)* $first) ($($brackets)*) $($rest)*);
    };

    // End with `where` clause.
    (path ($($generics:tt)*) ($($path:tt)*) where $($rest:tt)*) => {
        $crate::__internal_deep_clone_trait_object!(impl ($($generics)*) ($($path)*) ($($rest)*));
    };

    // End without `where` clause.
    (path ($($generics:tt)*) ($($path:tt)*)) => {
        $crate::__internal_deep_clone_trait_object!(impl ($($generics)*) ($($path)*) ());
    };

    // Token inside of path.
    (path ($($generics:tt)*) ($($path:tt)*) $first:tt $($rest:tt)*) => {
        $crate::__internal_deep_clone_trait_object!(path ($($generics)*) ($($path)* $first) $($rest)*);
    };

    // The impls, one per auto-trait combination the trait object can carry.
    (impl ($($generics:tt)*) ($($path:tt)*) ($($bound:tt)*)) => {
        $crate::__internal_deep_clone_trait_object!(@one ($($generics)*) (dyn $($path)*) ($($bound)*));
        $crate::__internal_deep_clone_trait_object!(@one ($($generics)*) (dyn $($path)* + $crate::__private::Send) ($($bound)*));
        $crate::__internal_deep_clone_trait_object!(@one ($($generics)*) (dyn $($path)* + $crate::__private::Sync) ($($bound)*));
        $crate::__internal_deep_clone_trait_object!(@one ($($generics)*) (dyn $($path)* + $crate::__private::Send + $crate::__private::Sync) ($($bound)*));
    };

    (@one ($($generics:tt)*) ($($object:tt)*) ($($bound:tt)*)) => {
        #[allow(unknown_lints, non_local_definitions)]
        impl<'clone, $($generics)*> $crate::DeepClone
            for $crate::__private::Box<$($object)* + 'clone>
        where
            $($bound)*
        {
            fn deep_clone_in(&self, cloner: &mut $crate::Cloner) -> Self {
                $crate::deep_clone_box(&**self, cloner)
            }
        }
    };
}
