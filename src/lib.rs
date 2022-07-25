#![feature(decl_macro)]

#![feature(generic_associated_types)]

#![feature(iter_collect_into)]
#![feature(cell_update)]
#![feature(unboxed_closures)]
#![feature(fn_traits)]
#![feature(allocator_api)]

//! `Rx` means "reactive value" (or "reactive X"). It is a wrapper for a value which changes,
//! and these changes trigger dependencies to re-run and change themselves.
//!
//! Because of Rust's borrowing rules, you can't just have `Rx` values change arbitrarily,
//! because then references will be invalidated. Instead, when an `Rx` is updated, this update is delayed until there are no mutable references.
//! Furthermore, you cannot just get a mutable reference to an `Rx` value, you must set it to an entirely new value.
//!
//! The way it works is, there is an [RxDAG] which stores the entire dependency graph, and you can only get a reference to an `Rx` value
//! from a shared reference to the graph. The `Rx`s update when you call [RxDAG::recompute], which requires a mutable reference.
//!
//! Furthermore, `Rx` closures must have a specific lifetime, because they may be recomputed.
//! This lifetime is annotated `'c` and the same lifetime is for every closure in an [RxDAG].
//! value directly, instead you use an associated function like [RxDAG::run_rx] to access it in a closure
//! which can re-run whenever the dependency changes. You can create new `Rx`s from old ones.

pub(crate) mod misc;
pub(crate) mod dag;
pub(crate) mod dag_uid;
pub(crate) mod rx_impl;
pub(crate) mod rx_ref;
pub(crate) mod clone_set_fn;

pub use dag::*;
pub use rx_ref::*;
pub use clone_set_fn::*;