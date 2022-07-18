use std::cell::Cell;
use std::fmt::{Debug, Formatter};
use std::mem::{MaybeUninit, size_of, transmute};
use crate::misc::stable_deref2::{Deref2, StableDeref2};
use crate::misc::frozen_vec::FrozenSlice;
use crate::misc::assert_variance::assert_is_covariant;
use crate::dag::{RxInput, RxSubDAG};
use crate::dag_uid::RxDAGUid;

#[derive(Debug)]
pub(crate) enum RxDAGElem<'c> {
    Node(Box<Rx<'c>>),
    Edge(Box<RxEdge<'c>>)
}

#[derive(Debug)]
pub(crate) enum RxDAGElemRef<'a, 'c> {
    Node(&'a Rx<'c>),
    Edge(&'a RxEdge<'c>)
}

pub(crate) type Rx<'c> = dyn RxTrait + 'c;
assert_is_covariant!((Rx<'c>) over 'c);
pub(crate) type RxEdge<'c> = dyn RxEdgeTrait + 'c;
assert_is_covariant!((RxEdge<'c>) over 'c);

pub(crate) trait RxTrait: Debug {
    fn post_read(&self) -> bool;

    fn recompute(&mut self);
    fn did_recompute(&self) -> bool;
    fn post_recompute(&mut self);

    unsafe fn _get_dyn(&self) -> *const ();
    unsafe fn _take_latest_dyn(&self, ptr: *mut MaybeUninit<CurrentOrNext<'_, ()>>, size: usize);
    unsafe fn _set_dyn(&self, ptr: *mut MaybeUninit<()>, size: usize);
}

pub(crate) struct RxImpl<T> {
    current: T,
    next: Cell<Option<T>>,
    // Rx flags (might have same flags for a group to reduce traversing all Rxs)
    did_read: Cell<bool>,
    did_recompute: bool
}

// trait RxEdgeTrait<cov 'c>: Debug
pub(crate) trait RxEdgeTrait: Debug {
    // fn recompute(&mut self, index: usize, before: &[RxDAGElem<'c>], after: &[RxDAGElem<'c>], graph_id: RxDAGUid<'c>);
    // 'c2 must outlive 'c, this is a workaround beause there aren't covariant trait lifetime parameters
    fn recompute<'c2>(&mut self, index: usize, before: &[RxDAGElem<'c2>], after: &[RxDAGElem<'c2>], graph_id: RxDAGUid<'c2>);
}

pub(crate) struct RxEdgeImpl<'c, F: FnMut(&mut Vec<usize>, RxInput<'_, 'c>, &mut dyn Iterator<Item=&Rx<'c>>) + 'c> {
    // Takes current of input values (first argument) and sets next of output values (second argument).
    compute: F,
    num_outputs: usize,
    input_backwards_offsets: Vec<usize>,
    cached_inputs: Vec<*const Rx<'c>>
}

pub(crate) enum CurrentOrNext<'a, T> {
    Current(&'a T),
    Next(T)
}

impl<'c> RxDAGElem<'c> {
    pub(crate) fn recompute(&mut self, index: usize, before: &[RxDAGElem<'c>], after: &[RxDAGElem<'c>], graph_id: RxDAGUid<'c>) {
        match self {
            RxDAGElem::Node(x) => x.recompute(),
            // this is ok because this allows an arbitrary lifetime, but we pass 'c which is required
            RxDAGElem::Edge(x) => x.recompute(index, before, after, graph_id)
        }
    }

    pub(crate) fn post_recompute(&mut self) {
        match self {
            RxDAGElem::Node(x) => x.post_recompute(),
            RxDAGElem::Edge(_) => {}
        }
    }

    pub(crate) fn as_node(&self) -> Option<&Rx<'c>> {
        match self {
            RxDAGElem::Node(x) => Some(x.as_ref()),
            _ => None
        }
    }
}

impl<'a, 'c> RxDAGElemRef<'a, 'c> {
    pub(crate) fn post_read(self) -> bool {
        match self {
            RxDAGElemRef::Node(node) => node.post_read(),
            RxDAGElemRef::Edge(_) => false
        }
    }

    //noinspection RsSelfConvention because this is itself a reference
    pub(crate) fn as_node(self) -> Option<&'a Rx<'c>> {
        match self {
            RxDAGElemRef::Node(x) => Some(x),
            _ => None
        }
    }
}

impl<T> RxImpl<T> {
    pub(crate) fn new(init: T) -> Self {
        Self {
            current: init,
            next: Cell::new(None),
            did_read: Cell::new(false),
            did_recompute: false
        }
    }

    pub(crate) fn get(&self) -> &T {
        self.did_read.set(true);
        &self.current
    }

    /// Take `next` if set, otherwise returns a reference to `current`.
    /// The value should then be re-assigned to `next` via `set`.
    pub(crate) fn take_latest(&self) -> CurrentOrNext<'_, T> {
        self.did_read.set(true);
        match self.next.take() {
            None => CurrentOrNext::Current(&self.current),
            Some(next) => CurrentOrNext::Next(next)
        }
    }

    pub(crate) fn set(&self, value: T) {
        self.next.set(Some(value));
    }
}

impl<T> RxTrait for RxImpl<T> {
    fn post_read(&self) -> bool {
        self.did_read.take()
    }

    fn recompute(&mut self) {
        debug_assert!(!self.did_recompute);
        match self.next.take() {
            // Didn't update
            None => {}
            // Did update
            Some(next) => {
                self.current = next;
                self.did_recompute = true;
            }
        }
    }

    fn did_recompute(&self) -> bool {
        self.did_recompute
    }

    fn post_recompute(&mut self) {
        self.did_recompute = false;
    }

    unsafe fn _get_dyn(&self) -> *const () {
        self.get() as *const T as *const ()
    }

    unsafe fn _take_latest_dyn(&self, ptr: *mut MaybeUninit<CurrentOrNext<'_, ()>>, size: usize) {
        debug_assert_eq!(size, size_of::<T>(), "_take_latest_dyn called with wrong size");
        let ptr = ptr as *mut MaybeUninit<CurrentOrNext<'_, T>>;
        let value = self.take_latest();

        ptr.write(MaybeUninit::new(value));
    }

    unsafe fn _set_dyn(&self, ptr: *mut MaybeUninit<()>, size: usize) {
        debug_assert_eq!(size, size_of::<T>(), "_set_dyn called with wrong size");
        let ptr = ptr as *mut MaybeUninit<T>;
        let value = std::mem::replace(&mut *ptr, MaybeUninit::uninit());

        self.set(value.assume_init());
    }
}

impl<'c> Deref2 for RxDAGElem<'c> {
    type Target<'a> = RxDAGElemRef<'a, 'c> where Self: 'a;

    fn deref2(&self) -> Self::Target<'_> {
        match self {
            RxDAGElem::Node(x) => RxDAGElemRef::Node(x.deref2()),
            RxDAGElem::Edge(x) => RxDAGElemRef::Edge(x.deref2())
        }
    }
}

unsafe impl<'c> StableDeref2 for RxDAGElem<'c> {}

impl<'c, F: FnMut(&mut Vec<usize>, RxInput<'_, 'c>, &mut dyn Iterator<Item=&Rx<'c>>) + 'c> RxEdgeImpl<'c, F> {
    pub(crate) fn new(input_backwards_offsets: Vec<usize>, num_outputs: usize, compute: F) -> Self {
        let num_inputs = input_backwards_offsets.len();
        Self {
            input_backwards_offsets,
            num_outputs,
            compute,
            cached_inputs: Vec::with_capacity(num_inputs)
        }
    }

    pub(crate) fn output_forwards_offsets(&self) -> impl Iterator<Item=usize> {
        // Maybe this is a dumb abstraction.
        // This is very simple, outputs are currently always right after the edge.
        0..self.num_outputs
    }
}

impl<'c, F: FnMut(&mut Vec<usize>, RxInput<'_, 'c>, &mut dyn Iterator<Item=&Rx<'c>>) + 'c> RxEdgeTrait for RxEdgeImpl<'c, F> {
    fn recompute<'c2>(&mut self, index: usize, before: &[RxDAGElem<'c2>], after: &[RxDAGElem<'c2>], graph_id: RxDAGUid<'c2>) {
        // 'c2 must outlive 'c, this is a workaround because there aren't covariant trait lifetime parameters
        let (before, after, graph_id) = unsafe {
            transmute::<(&[RxDAGElem<'c2>], &[RxDAGElem<'c2>], RxDAGUid<'c2>), (&[RxDAGElem<'c>], &[RxDAGElem<'c>], RxDAGUid<'c>)>((before, after, graph_id))
        };

        debug_assert!(self.cached_inputs.is_empty());
        self.input_backwards_offsets.iter().copied().map(|offset| {
            before[before.len() - offset].as_node().expect("broken RxDAG: RxEdge input must be a node") as *const Rx<'c>
        }).collect_into(&mut self.cached_inputs);
        let mut inputs = self.cached_inputs.iter().map(|x| unsafe { &**x });

        if inputs.any(|x| x.did_recompute()) {
            // Needs update
            let mut outputs = self.output_forwards_offsets().map(|offset| {
                after[offset].as_node().expect("broken RxDAG: RxEdge output must be a node")
            });
            let input_dag = RxInput(RxSubDAG {
                before: FrozenSlice::from(before),
                index,
                id: graph_id
            });
            (self.compute)(&mut self.input_backwards_offsets, input_dag, &mut outputs);
        }
        self.cached_inputs.clear();
    }
}

impl<'c> dyn RxTrait + 'c {
    pub(crate) unsafe fn set_dyn<T>(&self, value: T) {
        debug_assert_eq!(size_of::<*const T>(), size_of::<*const ()>(), "won't work");
        let mut value = MaybeUninit::new(value);
        self._set_dyn(&mut value as *mut MaybeUninit<T> as *mut MaybeUninit<()>, size_of::<T>());
    }

    pub(crate) unsafe fn get_dyn<T>(&self) -> &T {
        debug_assert_eq!(size_of::<*const T>(), size_of::<*const ()>(), "won't work");
        &*(self._get_dyn() as *const T)
    }

    pub(crate) unsafe fn take_latest_dyn<T>(&self) -> CurrentOrNext<'_, T> {
        debug_assert_eq!(size_of::<*const T>(), size_of::<*const ()>(), "won't work");
        let mut value = MaybeUninit::<CurrentOrNext<'_, T>>::uninit();
        self._take_latest_dyn(&mut value as *mut MaybeUninit<CurrentOrNext<'_, T>> as *mut MaybeUninit<CurrentOrNext<'_, ()>>, size_of::<T>());
        value.assume_init()
    }
}

impl<T> Debug for RxImpl<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RxImpl")
            .field("next.is_some()", &unsafe { &*self.next.as_ptr() }.is_some())
            .field("did_read", &self.did_read.get())
            .field("did_recompute", &self.did_recompute)
            .finish_non_exhaustive()
    }
}

impl<'c, F: FnMut(&mut Vec<usize>, RxInput<'_, 'c>, &mut dyn Iterator<Item=&Rx<'c>>) + 'c> Debug for RxEdgeImpl<'c, F> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RxEdgeImpl")
            .field("num_outputs", &self.num_outputs)
            .field("input_backwards_offsets", &self.input_backwards_offsets)
            .finish_non_exhaustive()
    }
}

impl<'a, T> AsRef<T> for CurrentOrNext<'a, T> {
    fn as_ref(&self) -> &T {
        match self {
            CurrentOrNext::Current(x) => x,
            CurrentOrNext::Next(x) => x
        }
    }
}