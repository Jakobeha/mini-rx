//! Split at an index and return references to the elements before, after, and the element itself.

use std::iter::{Iterator, ExactSizeIterator};
use std::mem::transmute;

pub struct IterSplit3s<'a, T> {
    slice: &'a [T],
    index: usize
}

pub struct IterMutSplit3s<'a, T> {
    slice: &'a mut [T],
    index: usize
}

pub trait SliceSplit3<T> {
    fn split3(&self, index: usize) -> (&[T], &T, &[T]);
    fn iter_split3s(&self) -> IterSplit3s<'_, T>;
    fn split3_mut(&mut self, index: usize) -> (&mut [T], &mut T, &mut [T]);
    fn iter_mut_split3s(&mut self) -> IterMutSplit3s<'_, T>;
}

impl<T> SliceSplit3<T> for [T] {
    fn split3(&self, index: usize) -> (&[T], &T, &[T]) {
        let (before, current_and_after) = self.split_at(index);
        let (current, after) = current_and_after.split_first().unwrap();
        (before, current, after)
    }

    fn iter_split3s(&self) -> IterSplit3s<'_, T> {
        IterSplit3s::new(self)
    }

    fn split3_mut(&mut self, index: usize) -> (&mut [T], &mut T, &mut [T]) {
        let (before, current_and_after) = self.split_at_mut(index);
        let (current, after) = current_and_after.split_first_mut().unwrap();
        (before, current, after)
    }

    fn iter_mut_split3s(&mut self) -> IterMutSplit3s<'_, T> {
        IterMutSplit3s::new(self)
    }
}

impl<'a, T> IterSplit3s<'a, T> {
    fn new(slice: &'a [T]) -> IterSplit3s<'a, T> {
        IterSplit3s {
            slice,
            index: 0
        }
    }
}

impl<'a, T> Iterator for IterSplit3s<'a, T> {
    type Item = (&'a [T], &'a T, &'a [T]);

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.slice.len() {
            None
        } else {
            let split3 = self.slice.split3(self.index);
            self.index += 1;
            Some(unsafe { transmute::<(&[T], &T, &[T]), (&'a [T], &'a T, &'a [T])>(split3) })
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.slice.len() - self.index, Some(self.slice.len() - self.index))
    }

    fn count(self) -> usize {
        self.slice.len() - self.index
    }
}

impl<'a, T> IterMutSplit3s<'a, T> {
    fn new(slice: &'a mut [T]) -> IterMutSplit3s<'a, T> {
        IterMutSplit3s {
            slice,
            index: 0
        }
    }
}

impl<'a, T> Iterator for IterMutSplit3s<'a, T> {
    type Item = (&'a mut [T], &'a mut T, &'a mut [T]);

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.slice.len() {
            None
        } else {
            let split3 = self.slice.split3_mut(self.index);
            self.index += 1;
            Some(unsafe { transmute::<(&mut [T], &mut T, &mut [T]), (&'a mut [T], &'a mut T, &'a mut [T])>(split3) })
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.slice.len() - self.index, Some(self.slice.len() - self.index))
    }

    fn count(self) -> usize {
        self.slice.len() - self.index
    }
}

impl<'a, T> ExactSizeIterator for IterMutSplit3s<'a, T> {}