//! Copied from [elsa FrozenVec](https://docs.rs/elsa/latest/src/elsa/vec.rs.html#10-14),
//! modified.

use std::alloc::{Allocator, Global};
use std::cell::UnsafeCell;
use std::cmp::Ordering;
use std::fmt::{Debug, Formatter};
use std::iter::{Iterator, IntoIterator};
use std::mem::transmute;

use crate::misc::stable_deref2::StableDeref2;

/// Version of `std::vec::Vec` where insertion does not require mutable access,
/// but without mutable access, you may only retrieve pointers which deref to the same location after move.
///
/// This is sound because insertion may reallocate the vector itself and move the elements,
/// but it will not move what they deref to. As long as we don't replace or delete any elements,
/// the pointed-to values will not be dropped.
///
/// Furthermore, you can get the underlying `&mut` vector with mutable access, because that ensures
/// that there are no active shared references to the vector's elements.
pub struct FrozenVec<T, A: Allocator = Global>(UnsafeCell<Vec<T, A>>);

// safety: UnsafeCell implies !Sync

impl<T> FrozenVec<T> {
    /// Constructs a new, empty vector.
    pub fn new() -> Self {
        Self(UnsafeCell::new(Vec::new()))
    }
}

impl<T, A: Allocator> FrozenVec<T, A> {
    /// Constructs a new, empty vector.
    pub fn new_in(alloc: A) -> Self {
        Self(UnsafeCell::new(Vec::new_in(alloc)))
    }

    /// Appends an element to the back of the vector.
    pub fn push(&self, val: T) {
        unsafe {
            let vec = self.0.get();
            (*vec).push(val)
        }
    }
}

impl<T: StableDeref2, A: Allocator> FrozenVec<T, A> {
    // these should never return &T
    // these should never delete any entries

    /// Push, immediately getting a reference to the element
    pub fn push_get(&self, val: T) -> T::Target<'_> {
        self.push(val);
        unsafe { self.get_unchecked(self.len() - 1) }
    }

    /// Returns a reference to an element.
    pub fn get(&self, index: usize) -> Option<T::Target<'_>> {
        unsafe {
            let vec = self.0.get();
            (*vec).get(index).map(|x| x.deref2())
        }
    }

    /// Returns a reference to an element, without doing bounds checking.
    ///
    /// ## Safety
    ///
    /// `index` must be in bounds, i.e. it must be less than `self.len()`
    pub unsafe fn get_unchecked(&self, index: usize) -> T::Target<'_> {
        let vec = self.0.get();
        (*vec).get_unchecked(index).deref2()
    }

    /// **Panics** if out-of-bounds.
    pub fn index(&self, idx: usize) -> T::Target<'_> {
        self.get(idx).unwrap_or_else(|| {
            panic!(
                "index out of bounds: the len is {} but the index is {}",
                self.len(),
                idx
            )
        })
    }

    /// Returns the number of elements in the vector.
    pub fn len(&self) -> usize {
        unsafe {
            let vec = self.0.get();
            (*vec).len()
        }
    }

    /// Returns `true` if the vector contains no elements.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the first element of the vector, or `None` if empty.
    pub fn first(&self) -> Option<T::Target<'_>> {
        unsafe {
            let vec = self.0.get();
            (*vec).first().map(|x| x.deref2())
        }
    }

    /// Returns the last element of the vector, or `None` if empty.
    pub fn last(&self) -> Option<T::Target<'_>> {
        unsafe {
            let vec = self.0.get();
            (*vec).last().map(|x| x.deref2())
        }
    }

    /// Returns an iterator over the vector.
    pub fn iter(&self) -> Iter<T> {
        self.into_iter()
    }

    /// Converts the frozen vector into a plain vector.
    pub fn into_vec(self) -> Vec<T> {
        self.0.into_inner()
    }

    /// Get mutable access to the underlying vector.
    ///
    /// This is safe, as it requires a `&mut self`, ensuring nothing is using
    /// the 'frozen' contents.
    pub fn as_mut(&mut self) -> &mut Vec<T> {
        unsafe { &mut *self.0.get() }
    }

    // binary search functions: they need to be reimplemented here to be safe (instead of calling
    // their equivalents directly on the underlying Vec), as they run user callbacks that could
    // reentrantly call other functions on this vector

    /// Binary searches this sorted vector for a given element, analogous to [slice::binary_search].
    pub fn binary_search<'a>(&'a self, x: T::Target<'a>) -> Result<usize, usize>
        where
            T::Target<'a>: Ord,
    {
        self.binary_search_by(|p| p.cmp(&x))
    }

    /// Binary searches this sorted vector with a comparator function, analogous to
    /// [slice::binary_search_by].
    pub fn binary_search_by<'a, F>(&'a self, mut f: F) -> Result<usize, usize>
        where
            F: FnMut(T::Target<'a>) -> Ordering,
    {
        let mut size = self.len();
        let mut left = 0;
        let mut right = size;
        while left < right {
            let mid = left + size / 2;

            // safety: like the core algorithm, mid is always within original vector len; in
            // pathlogical cases, user could push to the vector in the meantime, but this can only
            // increase the length, keeping this safe
            let cmp = f(unsafe { self.get_unchecked(mid) });

            if cmp == Ordering::Less {
                left = mid + 1;
            } else if cmp == Ordering::Greater {
                right = mid;
            } else {
                return Ok(mid);
            }

            size = right - left;
        }
        Err(left)
    }

    /// Binary searches this sorted vector with a key extraction function, analogous to
    /// [slice::binary_search_by_key].
    pub fn binary_search_by_key<'a, B, F>(&'a self, b: &B, mut f: F) -> Result<usize, usize>
        where
            F: FnMut(T::Target<'a>) -> B,
            B: Ord,
    {
        self.binary_search_by(|k| f(k).cmp(b))
    }

    /// Returns the index of the partition point according to the given predicate
    /// (the index of the first element of the second partition), analogous to
    /// [slice::partition_point].
    pub fn partition_point<P>(&self, mut pred: P) -> usize
        where
            P: FnMut(T::Target<'_>) -> bool,
    {
        let mut left = 0;
        let mut right = self.len();

        while left != right {
            let mid = left + (right - left) / 2;
            // safety: like in binary_search_by
            let value = unsafe { self.get_unchecked(mid) };
            if pred(value) {
                left = mid + 1;
            } else {
                right = mid;
            }
        }

        left
    }

    // TODO add more
}

impl<T, A: Allocator> Default for FrozenVec<T, A> {
    fn default() -> Self {
        FrozenVec::new()
    }
}

impl<T, A: Allocator> From<Vec<T, A>> for FrozenVec<T, A> {
    fn from(vec: Vec<T>) -> Self {
        Self(UnsafeCell::new(vec))
    }
}

impl<I> FromIterator<I> for FrozenVec<I> {
    fn from_iter<T>(iter: T) -> Self
        where
            T: IntoIterator<Item =I>,
    {
        let vec: Vec<_> = iter.into_iter().collect();
        vec.into()
    }
}

/// Iterator over FrozenVec, obtained via `.iter()`
///
/// It is safe to push to the vector during iteration
pub struct Iter<'a, T, A: Allocator = Global> {
    vec: &'a FrozenVec<T, A>,
    idx: usize,
}

impl<'a, T: StableDeref2, A: Allocator> Iterator for Iter<'a, T, A> {
    type Item = T::Target<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(ret) = self.vec.get(self.idx) {
            self.idx += 1;
            Some(ret)
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.vec.len()))
    }
}

impl<'a, T: StableDeref2, A: Allocator> IntoIterator for &'a FrozenVec<T, A> {
    type Item = T::Target<'a>;
    type IntoIter = Iter<'a, T, A>;

    fn into_iter(self) -> Iter<'a, T, A> {
        Iter { vec: self, idx: 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iteration() {
        let vec = vec!["a", "b", "c", "d"];
        let frozen: FrozenVec<_> = vec.clone().into();

        assert_eq!(vec, frozen.iter().collect::<Vec<_>>());
        for (e1, e2) in vec.iter().zip(frozen.iter()) {
            assert_eq!(*e1, e2);
        }

        assert_eq!(vec.len(), frozen.iter().count())
    }

    #[test]
    fn test_accessors() {
        let vec: FrozenVec<String> = FrozenVec::new();

        assert_eq!(vec.is_empty(), true);
        assert_eq!(vec.len(), 0);
        assert_eq!(vec.first(), None);
        assert_eq!(vec.last(), None);
        assert_eq!(vec.get(1), None);

        vec.push("a".to_string());
        vec.push("b".to_string());
        vec.push("c".to_string());

        assert_eq!(vec.is_empty(), false);
        assert_eq!(vec.len(), 3);
        assert_eq!(vec.first(), Some("a"));
        assert_eq!(vec.last(), Some("c"));
        assert_eq!(vec.get(1), Some("b"));
    }

    #[test]
    fn test_binary_search() {
        let vec: FrozenVec<_> = vec!["ab", "cde", "fghij"].into();

        assert_eq!(vec.binary_search("cde"), Ok(1));
        assert_eq!(vec.binary_search("cdf"), Err(2));
        assert_eq!(vec.binary_search("a"), Err(0));
        assert_eq!(vec.binary_search("g"), Err(3));

        assert_eq!(vec.binary_search_by_key(&1, |x| x.len()), Err(0));
        assert_eq!(vec.binary_search_by_key(&3, |x| x.len()), Ok(1));
        assert_eq!(vec.binary_search_by_key(&4, |x| x.len()), Err(2));

        assert_eq!(vec.partition_point(|x| x.len() < 4), 2);
        assert_eq!(vec.partition_point(|_| false), 0);
        assert_eq!(vec.partition_point(|_| true), 3);
    }
}

impl<T: StableDeref2, A: Allocator> Debug for FrozenVec<T, A> where for<'a> T::Target<'a>: Debug {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let this = unsafe { transmute::<&Self, &'static Self>(&self) };
        f.debug_list().entries(this.iter()).finish()
    }
}

impl<'a, T: StableDeref2> Debug for FrozenSlice<'a, T> where for<'b> T::Target<'b>: Debug {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let this = unsafe { transmute::<&Self, &'static Self>(&self) };
        f.debug_list().entries(this.iter()).finish()
    }
}

/// Value which may be a slice of a regular vector or `FrozenVec`.
/// Like `FrozenVec` you can only get pointed-to values, not the elements themselves.
pub struct FrozenSlice<'a, T>(&'a [T]);

pub struct FrozenSliceIter<'a, T>(std::slice::Iter<'a, T>);

impl<'a, T: StableDeref2> FrozenSlice<'a, T> {
    pub fn iter(&self) -> FrozenSliceIter<'a, T> {
        FrozenSliceIter(self.0.iter())
    }

    pub unsafe fn get_unchecked(&self, index: usize) -> T::Target<'a> {
        self.0.get_unchecked(index).deref2()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    // TODO add more
}

impl<'a, T, A: Allocator> From<&'a FrozenVec<T, A>> for FrozenSlice<'a, T> {
    /// Get a `FrozenSlice`, which is the "slice" equivalent of a `FrozenVec`.
    /// This is safe, because you can only access the `FrozenSlice` like a frozen vector.
    /// This is useful, because you can also convert regular slices into `FrozenSlice`.
    fn from(vec: &'a FrozenVec<T>) -> Self {
        FrozenSlice(&unsafe { &*vec.0.get() })
    }
}

impl<'a, T> From<&'a [T]> for FrozenSlice<'a, T> {
    fn from(slice: &'a [T]) -> Self {
        FrozenSlice(slice)
    }
}

impl<'a, T: StableDeref2> IntoIterator for FrozenSlice<'a, T> {
    type Item = T::Target<'a>;
    type IntoIter = FrozenSliceIter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        FrozenSliceIter(self.0.into_iter())
    }
}

impl<'a, T: StableDeref2> Iterator for FrozenSliceIter<'a, T> {
    type Item = T::Target<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|x| x.deref2())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl<'a, T> Clone for FrozenSlice<'a, T> {
    fn clone(&self) -> Self {
        Self(self.0)
    }
}

impl<'a, T> Copy for FrozenSlice<'a, T> {}