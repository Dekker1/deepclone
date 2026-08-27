//! Compile-fail tests for the derive's own diagnostics.
//!
//! Every case here fails on a message this crate writes itself, so the snapshots do not move
//! with the compiler's own wording. Cases resting on a rustc-generated message, such as an
//! unsatisfied `DeepClone` bound, would drift between channels and do not belong here.

#![cfg(feature = "derive")]
#![expect(
	clippy::tests_outside_test_module,
	reason = "an integration test crate is entirely tests"
)]

#[cfg_attr(miri, ignore = "trybuild shells out to cargo, which miri cannot run")]
#[test]
fn ui() {
	let cases = trybuild::TestCases::new();
	cases.compile_fail("tests/ui/*.rs");
}
