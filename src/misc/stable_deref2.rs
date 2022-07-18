//! From [stable-deref-trait](https://docs.rs/stable_deref_trait/1.2.0/src/stable_deref_trait/lib.rs.html#122)

// Copyright 2017 Robert Grosse

// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

/** Deref but you can implement it returning lifetime-parameterized objects like enums */
pub trait Deref2 {
    type Target<'a> where Self: 'a;

    fn deref2(&self) -> Self::Target<'_>;
}

pub macro impl_deref2_from_deref($([$($impl_params:tt)*])? ($($impl_ty:tt)*)) {
impl$(<$($impl_params)*>)? Deref2 for $($impl_ty)* {
    type Target<'a> = &'a <Self as ::std::ops::Deref>::Target where Self: 'a;

    fn deref2(&self) -> Self::Target<'_> {
        ::std::ops::Deref::deref(self)
    }
}
}

pub macro impl_stable_deref2_from_deref($([$($impl_params:tt)*])? ($($impl_ty:tt)*)) {
impl_deref2_from_deref!($([$($impl_params)*])? ($($impl_ty)*));
unsafe impl$(<$($impl_params)*>)? StableDeref2 for $($impl_ty)* {}
}

pub macro impl_clone_stable_deref2_from_deref($([$($impl_params:tt)*])? ($($impl_ty:tt)*)) {
impl_stable_deref2_from_deref!($([$($impl_params)*])? ($($impl_ty)*));
unsafe impl$(<$($impl_params)*>)? CloneStableDeref2 for $($impl_ty)* {}
}


/** [`StableDeref`](https://docs.rs/stable_deref_trait/1.2.0/stable_deref_trait/trait.StableDeref.html) but with relaxed `Deref` requirements */
pub unsafe trait StableDeref2: Deref2 {}

/** [`CloneStableDeref`](https://docs.rs/stable_deref_trait/1.2.0/stable_deref_trait/trait.CloneStableDeref.html) but with relaxed `Deref` requirements */
pub unsafe trait CloneStableDeref2: StableDeref2 + Clone {}

/////////////////////////////////////////////////////////////////////////////
// std types integration
/////////////////////////////////////////////////////////////////////////////

use std::ffi::{CString, OsString};
use std::path::PathBuf;
use std::sync::{Arc, MutexGuard, RwLockReadGuard, RwLockWriteGuard};

use core::cell::{Ref, RefMut};
use std::rc::Rc;

impl_stable_deref2_from_deref!([T: ?Sized] (Box<T>));
impl_stable_deref2_from_deref!((String));
impl_stable_deref2_from_deref!((CString));
impl_stable_deref2_from_deref!((OsString));
impl_stable_deref2_from_deref!((PathBuf));

impl_clone_stable_deref2_from_deref!([T: ?Sized] (Rc<T>));
impl_clone_stable_deref2_from_deref!([T: ?Sized] (Arc<T>));

impl_stable_deref2_from_deref!(['b, T: ?Sized] (Ref<'b, T>));
impl_stable_deref2_from_deref!(['b, T: ?Sized] (RefMut<'b, T>));
impl_stable_deref2_from_deref!(['b, T: ?Sized] (MutexGuard<'b, T>));
impl_stable_deref2_from_deref!(['b, T: ?Sized] (RwLockReadGuard<'b, T>));
impl_stable_deref2_from_deref!(['b, T: ?Sized] (RwLockWriteGuard<'b, T>));

impl_clone_stable_deref2_from_deref!(['b, T: ?Sized] (&'b T));
impl_stable_deref2_from_deref!(['b, T: ?Sized] (&'b mut T));