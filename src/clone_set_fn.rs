use std::marker::PhantomData;

#[doc(hidden)]
pub struct CloneSetFn<T: Clone, U, F: Fn(&mut T, U)>(F, PhantomData<(T, U)>);

impl<T: Clone, U, F: Fn(&mut T, U)> CloneSetFn<T, U, F> {
    pub(crate) fn new(f: F) -> CloneSetFn<T, U, F> {
        CloneSetFn(f, PhantomData)
    }
}

impl<T: Clone, U, F: Fn(&mut T, U)> FnOnce<(&T, U)> for CloneSetFn<T, U, F> {
    type Output = T;

    extern "rust-call" fn call_once(self, (root, child): (&T, U)) -> T {
        let mut root = root.clone();
        self.0(&mut root, child);
        root
    }
}

impl<T: Clone, U, F: Fn(&mut T, U)> FnMut<(&T, U)> for CloneSetFn<T, U, F> {
    extern "rust-call" fn call_mut(&mut self, (root, child): (&T, U)) -> T {
        let mut root = root.clone();
        self.0(&mut root, child);
        root
    }
}

impl<T: Clone, U, F: Fn(&mut T, U)> Fn<(&T, U)> for CloneSetFn<T, U, F> {
    extern "rust-call" fn call(&self, (root, child): (&T, U)) -> T {
        let mut root = root.clone();
        self.0(&mut root, child);
        root
    }
}

