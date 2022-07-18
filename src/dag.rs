use std::fmt::Debug;
use crate::dag_uid::RxDAGUid;
use crate::rx_impl::{RxDAGElem, RxImpl, Rx, RxEdgeImpl};
use crate::rx_ref::{RxRef, Var, CRx};
use crate::misc::frozen_vec::{FrozenVec, FrozenSlice};
use crate::misc::assert_variance::assert_is_covariant;
use crate::misc::slice_split3::SliceSplit3;

/// Returns a graph you can write. Note that `RxContext` and `MutRxContext` are neither subset nor superset of each other.
/// You can't read snapshots without recomputing, and you can't write inputs.
pub trait RxContext<'a, 'c: 'a> {
    fn sub_dag(self) -> RxSubDAG<'a, 'c>;
}

/// Returns a graph you can write. Note that `RxContext` and `MutRxContext` are neither subset nor superset of each other.
/// You can't read snapshots without recomputing, and you can't write inputs.
pub trait MutRxContext<'a, 'c: 'a> {
    fn sub_dag(self) -> RxSubDAG<'a, 'c>;
}

/// The DAG is a list of interspersed nodes and edges. The edges refer to other nodes relative to their own position.
/// Later Rxs *must* depend on earlier Rxs.
///
/// When the DAG recomputes, it simply iterates through each node and edge in order and calls `RxDAGElem::recompute`.
/// If the nodes were changed (directly or as edge output), they set their new value, and mark that they got recomputed.
/// The edges will recompute and change their output nodes if any of their inputs got recomputed.
///
/// The DAG has interior mutability, in that it can add nodes without a mutable borrow.
/// See `elsa` crate for why this is sound (though actually the soundness argument is contested).
/// Internally we use a modified version because of `elsa` and `stable-deref-trait`
///
/// Setting `Rx` values is also interior mutability, and OK because we don't use those values until `RxDAGElem::recompute`.
///
/// The DAG and refs have an ID so that you can't use one ref on another DAG, however this is checked at runtime.
/// The lifetimes are checked at compile-time though.
///
/// Currently no `Rx`s are deallocated until the entire DAG is deallocated,
/// so if you keep creating and discarding `Rx`s you will leak memory (TODO fix this?)
#[derive(Debug)]
pub struct RxDAG<'c>(FrozenVec<RxDAGElem<'c>>, RxDAGUid<'c>);

#[derive(Debug, Clone, Copy)]
pub struct RxDAGSnapshot<'a, 'c: 'a>(&'a RxDAG<'c>);

#[derive(Debug, Clone, Copy)]
pub struct RxSubDAG<'a, 'c: 'a> {
    pub(crate) before: FrozenSlice<'a, RxDAGElem<'c>>,
    pub(crate) index: usize,
    pub(crate) id: RxDAGUid<'c>
}
assert_is_covariant!(for['a] (RxSubDAG<'a, 'c>) over 'c);

#[derive(Debug, Clone, Copy)]
pub struct RxInput<'a, 'c: 'a>(pub(crate) RxSubDAG<'a, 'c>);

impl<'c> RxDAG<'c> {
    /// Create an empty DAG
    pub fn new() -> Self {
        Self(FrozenVec::new(), RxDAGUid::next())
    }

    /// Create a variable `Rx` in this DAG.
    pub fn new_var<T: 'c>(&self, init: T) -> Var<'c, T> {
        let index = self.next_index();
        let rx = RxImpl::new(init);
        self.0.push(RxDAGElem::Node(Box::new(rx)));
        Var::new(RxRef::new(self, index))
    }

    /// Run a closure when inputs change, without creating any outputs (for side-effects).
    pub fn run_crx<F: FnMut(RxInput<'_, 'c>) + 'c>(&self, mut compute: F) {
        let mut input_backwards_offsets = Vec::new();
        let () = Self::run_compute(&mut compute, RxInput(self.sub_dag()), &mut input_backwards_offsets);
        let compute_edge = RxEdgeImpl::<'c, _>::new(input_backwards_offsets, 0, move |mut input_backwards_offsets: &mut Vec<usize>, input: RxInput<'_, 'c>, outputs: &mut dyn Iterator<Item=&Rx<'c>>| {
            input_backwards_offsets.clear();
            let () = Self::run_compute(&mut compute, input, &mut input_backwards_offsets);
            debug_assert!(outputs.next().is_none());
        });
        self.0.push(RxDAGElem::Edge(Box::new(compute_edge)));
    }

    /// Create a computed `Rx` in this DAG.
    pub fn new_crx<T: 'c, F: FnMut(RxInput<'_, 'c>) -> T + 'c>(&self, mut compute: F) -> CRx<'c, T> {
        let mut input_backwards_offsets = Vec::new();
        let init = Self::run_compute(&mut compute, RxInput(self.sub_dag()), &mut input_backwards_offsets);
        let compute_edge = RxEdgeImpl::<'c, _>::new(input_backwards_offsets, 1, move |mut input_backwards_offsets: &mut Vec<usize>, input: RxInput<'_, 'c>, outputs: &mut dyn Iterator<Item=&Rx<'c>>| {
            input_backwards_offsets.clear();
            let output = Self::run_compute(&mut compute, input, &mut input_backwards_offsets);
            unsafe { outputs.next().unwrap().set_dyn(output); }
            debug_assert!(outputs.next().is_none());
        });
        self.0.push(RxDAGElem::Edge(Box::new(compute_edge)));

        let index = self.next_index();
        let rx = RxImpl::new(init);
        self.0.push(RxDAGElem::<'c>::Node(Box::new(rx)));
        CRx::new(RxRef::new(self, index))
    }

    /// Create 2 computed `Rx` in this DAG which are created from the same function.
    pub fn new_crx2<T1: 'c, T2: 'c, F: FnMut(RxInput<'_, 'c>) -> (T1, T2) + 'c>(&self, mut compute: F) -> (CRx<'c, T1>, CRx<'c, T2>) {
        let mut input_backwards_offsets = Vec::new();
        let (init1, init2) = Self::run_compute(&mut compute, RxInput(self.sub_dag()), &mut input_backwards_offsets);
        let compute_edge = RxEdgeImpl::<'c, _>::new(input_backwards_offsets, 2, move |mut input_backwards_offsets: &mut Vec<usize>, input: RxInput<'_, 'c>, outputs: &mut dyn Iterator<Item=&Rx<'c>>| {
            input_backwards_offsets.clear();
            let (output1, output2) = Self::run_compute(&mut compute, input, &mut input_backwards_offsets);
            unsafe { outputs.next().unwrap().set_dyn(output1); }
            unsafe { outputs.next().unwrap().set_dyn(output2); }
            debug_assert!(outputs.next().is_none());
        });
        self.0.push(RxDAGElem::Edge(Box::new(compute_edge)));

        let index = self.next_index();
        let rx1 = RxImpl::new(init1);
        let rx2 = RxImpl::new(init2);
        self.0.push(RxDAGElem::<'c>::Node(Box::new(rx1)));
        self.0.push(RxDAGElem::<'c>::Node(Box::new(rx2)));
        (CRx::new(RxRef::new(self, index)), CRx::new(RxRef::new(self, index + 1)))
    }

    /// Create 3 computed `Rx` in this DAG which are created from the same function.
    pub fn new_crx3<T1: 'c, T2: 'c, T3: 'c, F: FnMut(RxInput<'_, 'c>) -> (T1, T2, T3) + 'c>(&self, mut compute: F) -> (CRx<'c, T1>, CRx<'c, T2>, CRx<'c, T3>) {
        let mut input_backwards_offsets = Vec::new();
        let (init1, init2, init3) = Self::run_compute(&mut compute, RxInput(self.sub_dag()), &mut input_backwards_offsets);
        let compute_edge = RxEdgeImpl::<'c, _>::new(input_backwards_offsets, 3, move |mut input_backwards_offsets: &mut Vec<usize>, input: RxInput<'_, 'c>, outputs: &mut dyn Iterator<Item=&Rx<'c>>| {
            input_backwards_offsets.clear();
            let (output1, output2, output3) = Self::run_compute(&mut compute, input, &mut input_backwards_offsets);
            unsafe { outputs.next().unwrap().set_dyn(output1); }
            unsafe { outputs.next().unwrap().set_dyn(output2); }
            unsafe { outputs.next().unwrap().set_dyn(output3); }
            debug_assert!(outputs.next().is_none());
        });
        self.0.push(RxDAGElem::Edge(Box::new(compute_edge)));

        let index = self.next_index();
        let rx1 = RxImpl::new(init1);
        let rx2 = RxImpl::new(init2);
        let rx3 = RxImpl::new(init3);
        self.0.push(RxDAGElem::<'c>::Node(Box::new(rx1)));
        self.0.push(RxDAGElem::<'c>::Node(Box::new(rx2)));
        self.0.push(RxDAGElem::<'c>::Node(Box::new(rx3)));
        (CRx::new(RxRef::new(self, index)), CRx::new(RxRef::new(self, index + 1)), CRx::new(RxRef::new(self, index + 2)))
    }

    fn next_index(&self) -> usize {
        self.0.len()
    }

    fn run_compute<T, F: FnMut(RxInput<'_, 'c>) -> T + 'c>(compute: &mut F, input: RxInput<'_, 'c>, input_backwards_offsets: &mut Vec<usize>) -> T {
        debug_assert!(input_backwards_offsets.is_empty());

        let result = compute(input);
        let input_indices = input.post_read();

        input_indices
            .into_iter()
            .map(|index| input.0.index - index)
            .collect_into(input_backwards_offsets);
        result
    }

    /// Update all `Var`s with their new values and recompute `Rx`s.
    pub fn recompute(&mut self) {
        for (index, (before, current, after)) in self.0.as_mut().iter_mut_split3s().enumerate() {
            current.recompute(index, before, after, self.1);
        }

        for current in self.0.as_mut().iter_mut() {
            current.post_recompute();
        }
    }

    /// Recomputes if necessary and then returns a `RxContext` you can use to get the current value.
    pub fn now(&mut self) -> RxDAGSnapshot<'_, 'c> {
        self.recompute();
        RxDAGSnapshot(self)
    }

    /// Returns a `RxContext` you can use to get the current value.
    /// However any newly-set values or computations will not be returned until `recompute` is called.
    pub fn stale(&self) -> RxDAGSnapshot<'_, 'c> {
        RxDAGSnapshot(self)
    }

    pub(crate) fn id(&self) -> RxDAGUid<'c> {
        self.1
    }
}

impl<'a, 'c: 'a> RxContext<'a, 'c> for RxDAGSnapshot<'a, 'c> {
    fn sub_dag(self) -> RxSubDAG<'a, 'c> {
        RxSubDAG {
            before: FrozenSlice::from(&self.0.0),
            index: self.0.0.len(),
            id: self.0.1
        }
    }
}

impl<'a, 'c: 'a> MutRxContext<'a, 'c> for &'a RxDAG<'c> {
    fn sub_dag(self) -> RxSubDAG<'a, 'c> {
        RxDAGSnapshot(self).sub_dag()
    }
}

impl<'a, 'c: 'a> RxContext<'a, 'c> for RxInput<'a, 'c> {
    fn sub_dag(self) -> RxSubDAG<'a, 'c> {
        self.0
    }
}

impl<'a, 'c: 'a> RxInput<'a, 'c> {
    fn post_read(&self) -> Vec<usize> {
        let mut results = Vec::new();
        for (index, current) in self.0.before.iter().enumerate() {
            if current.post_read() {
                results.push(index)
            }
        }
        results
    }
}