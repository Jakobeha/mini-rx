use std::alloc::{Allocator, Global};
use std::fmt::Debug;
use crate::dag_uid::RxDAGUid;
use crate::rx_impl::{RxDAGElem, RxImpl, Rx, RxEdgeImpl};
use crate::rx_ref::{RxRef, Var, CRx};
use crate::misc::frozen_vec::{FrozenVec, FrozenSlice};
use crate::misc::assert_variance::assert_is_covariant;
use crate::misc::slice_split3::SliceSplit3;

/// Returns a slice of [RxDAG] you can read nodes from.
///
/// Note that [RxContext] and [MutRxContext] are neither subset nor superset of each other.
/// You can't read snapshots without recomputing, and you can't write inputs.
pub trait RxContext<'a, 'c: 'a, A: Allocator = Global> {
    fn sub_dag(self) -> RxSubDAG<'a, 'c, A: Allocator>;
}

/// Returns a slice of [RxDAG] you can write variables in.
///
/// Note that [RxContext] and [MutRxContext] are neither subset nor superset of each other.
/// You can't read snapshots without recomputing, and you can't write inputs.
pub trait MutRxContext<'a, 'c: 'a, A: Allocator = Global> {
    fn sub_dag(self) -> RxSubDAG<'a, 'c, A: Allocator>;
}

/// The centralized structure which contains all your interconnected reactive values.
///
/// This structure is a directed-acyclic-graph (DAG), hence why its called [RxDAG].
/// We don't support computed values recursively depending on each other, which is why it's acyclic.
/// At the root/top of the DAG are your variables ([Var]), which you can explicitly set.
/// All other values are [CRx]s, which are computed.
/// [CRx] incoming edges are inputs, which are [Var]s and other [CRx]s.
/// [CRx] outgoing edges are outputs, which are more [CRx]s, but there are also "edges"
/// which just go nowhere, those are side-effects.
///
/// ## Performance notes
///
/// Currently no nodes ([Var]s or [CRx]s) are deallocated until the entire DAG is deallocated,
/// so if you keep creating and discarding nodes you will leak memory (TODO fix this?)
///
/// ## Implementation
///
/// Internally this is a vector of interspersed nodes and edges.
/// The edges refer to other nodes relative to their own position.
/// Later Rxs *must* depend on earlier Rxs.
/// "edges" can have zero outputs, those are side-effects as mentioned above.
///
/// When the DAG recomputes, it simply iterates through each node and edge in order and calls [RxDAGElem::recompute].
/// If the nodes were changed (directly or as edge output), they set their new value, and mark that they got recomputed.
/// The edges will recompute and change their output nodes if any of their inputs got recomputed.
///
/// The DAG has interior mutability, in that it can add nodes without a mutable borrow.
/// See [elsa](https://docs.rs/elsa/latest/elsa/) crate for why this is sound (though actually the soundness argument is contested).
/// Internally we use a modified version of [elsa](https://docs.rs/elsa/latest/elsa/) and [stable-deref-trait](https://docs.rs/stable-deref-trait/latest/stable-deref-trait/).
/// which lets us return lifetime-parameterized values instead of references among other things.
///
/// Setting [Var]s is also interior mutability, and OK because we don't use those values until [RxDAGElem::recompute].
///
/// The DAG and refs have an ID so that you can't use one ref on another DAG, however this is checked at runtime.
/// The lifetimes are checked at compile-time though.
#[derive(Debug)]
pub struct RxDAG<'c, A: Allocator = Global>(FrozenVec<RxDAGElem<'c, A>>, RxDAGUid<'c, A>);

/// Allows you to read from an [RxDAG].
#[derive(Debug, Clone, Copy)]
pub struct RxDAGSnapshot<'a, 'c: 'a, A: Allocator = Global>(&'a RxDAG<'c, A>);

/// Slice of an [RxDAG]
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct RxSubDAG<'a, 'c: 'a, A: Allocator = Global> {
    pub(crate) before: FrozenSlice<'a, RxDAGElem<'c, A>>,
    pub(crate) index: usize,
    pub(crate) id: RxDAGUid<'c, A>
}
assert_is_covariant!(for['a, A: Allocator] (RxSubDAG<'a, 'c, A>) over 'c);

/// Allows you to read from a slice of an [RxDAG].
#[derive(Debug, Clone, Copy)]
pub struct RxInput<'a, 'c: 'a, A: Allocator = Global>(pub(crate) RxSubDAG<'a, 'c, A>);

impl<'c, A: Allocator> RxDAG<'c, A> {
    /// Create an empty DAG
    pub fn new() -> Self {
        Self(FrozenVec::new(), RxDAGUid::next())
    }

    /// Create a variable ([Var]) in this DAG.
    pub fn new_var<T: 'c>(&self, init: T) -> Var<'c, T> {
        let index = self.next_index();
        let rx = RxImpl::new(init);
        self.0.push(RxDAGElem::Node(Box::new(rx)));
        Var::new(RxRef::new(self, index))
    }

    // region new_crx boilerplate

    /// Run a closure when inputs change, without creating any outputs (for side-effects).
    pub fn run_crx<F: FnMut(RxInput<'_, 'c, A>) + 'c>(&self, mut compute: F) {
        let mut input_backwards_offsets = Vec::new();
        let () = Self::run_compute(&mut compute, RxInput(self.sub_dag()), &mut input_backwards_offsets);
        let compute_edge = RxEdgeImpl::<'c, _, A>::new(input_backwards_offsets, 0, move |mut input_backwards_offsets: &mut Vec<usize>, input: RxInput<'_, 'c, A>, outputs: &mut dyn Iterator<Item=&Rx<'c, A>>| {
            input_backwards_offsets.clear();
            let () = Self::run_compute(&mut compute, input, &mut input_backwards_offsets);
            debug_assert!(outputs.next().is_none());
        });
        self.0.push(RxDAGElem::Edge(Box::new(compute_edge)));
    }

    /// Create a computed value ([CRx]) in this DAG.
    pub fn new_crx<T: 'c, F: FnMut(RxInput<'_, 'c, A>) -> T + 'c>(&self, mut compute: F) -> CRx<'c, T> {
        let mut input_backwards_offsets = Vec::new();
        let init = Self::run_compute(&mut compute, RxInput(self.sub_dag()), &mut input_backwards_offsets);
        let compute_edge = RxEdgeImpl::<'c, _, A>::new(input_backwards_offsets, 1, move |mut input_backwards_offsets: &mut Vec<usize>, input: RxInput<'_, 'c, A>, outputs: &mut dyn Iterator<Item=&Rx<'c, A>>| {
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

    /// Create 2 computed values ([CRx]s) in this DAG which are created from the same function.
    pub fn new_crx2<T1: 'c, T2: 'c, F: FnMut(RxInput<'_, 'c, A>) -> (T1, T2) + 'c>(&self, mut compute: F) -> (CRx<'c, T1>, CRx<'c, T2>) {
        let mut input_backwards_offsets = Vec::new();
        let (init1, init2) = Self::run_compute(&mut compute, RxInput(self.sub_dag()), &mut input_backwards_offsets);
        let compute_edge = RxEdgeImpl::<'c, _, A>::new(input_backwards_offsets, 2, move |mut input_backwards_offsets: &mut Vec<usize>, input: RxInput<'_, 'c, A>, outputs: &mut dyn Iterator<Item=&Rx<'c, A>>| {
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

    /// Create 3 computed values ([CRx]s) in this DAG which are created from the same function.
    pub fn new_crx3<T1: 'c, T2: 'c, T3: 'c, F: FnMut(RxInput<'_, 'c, A>) -> (T1, T2, T3) + 'c>(&self, mut compute: F) -> (CRx<'c, T1>, CRx<'c, T2>, CRx<'c, T3>) {
        let mut input_backwards_offsets = Vec::new();
        let (init1, init2, init3) = Self::run_compute(&mut compute, RxInput(self.sub_dag()), &mut input_backwards_offsets);
        let compute_edge = RxEdgeImpl::<'c, _, A>::new(input_backwards_offsets, 3, move |mut input_backwards_offsets: &mut Vec<usize>, input: RxInput<'_, 'c, A>, outputs: &mut dyn Iterator<Item=&Rx<'c, A>>| {
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

    /// Create 4 computed values ([CRx]s) in this DAG which are created from the same function.
    pub fn new_crx4<T1: 'c, T2: 'c, T3: 'c, T4: 'c, F: FnMut(RxInput<'_, 'c, A>) -> (T1, T2, T3, T4) + 'c>(&self, mut compute: F) -> (CRx<'c, T1>, CRx<'c, T2>, CRx<'c, T3>, CRx<'c, T4>) {
        let mut input_backwards_offsets = Vec::new();
        let (init1, init2, init3, init4) = Self::run_compute(&mut compute, RxInput(self.sub_dag()), &mut input_backwards_offsets);
        let compute_edge = RxEdgeImpl::<'c, _, A>::new(input_backwards_offsets, 4, move |mut input_backwards_offsets: &mut Vec<usize>, input: RxInput<'_, 'c, A>, outputs: &mut dyn Iterator<Item=&Rx<'c, A>>| {
            input_backwards_offsets.clear();
            let (output1, output2, output3, output4) = Self::run_compute(&mut compute, input, &mut input_backwards_offsets);
            unsafe { outputs.next().unwrap().set_dyn(output1); }
            unsafe { outputs.next().unwrap().set_dyn(output2); }
            unsafe { outputs.next().unwrap().set_dyn(output3); }
            unsafe { outputs.next().unwrap().set_dyn(output4); }
            debug_assert!(outputs.next().is_none());
        });
        self.0.push(RxDAGElem::Edge(Box::new(compute_edge)));

        let index = self.next_index();
        let rx1 = RxImpl::new(init1);
        let rx2 = RxImpl::new(init2);
        let rx3 = RxImpl::new(init3);
        let rx4 = RxImpl::new(init4);
        self.0.push(RxDAGElem::<'c>::Node(Box::new(rx1)));
        self.0.push(RxDAGElem::<'c>::Node(Box::new(rx2)));
        self.0.push(RxDAGElem::<'c>::Node(Box::new(rx3)));
        self.0.push(RxDAGElem::<'c>::Node(Box::new(rx4)));
        (CRx::new(RxRef::new(self, index)), CRx::new(RxRef::new(self, index + 1)), CRx::new(RxRef::new(self, index + 2)), CRx::new(RxRef::new(self, index + 3)))
    }

    /// Create 5 computed values ([CRx]s) in this DAG which are created from the same function.
    pub fn new_crx5<T1: 'c, T2: 'c, T3: 'c, T4: 'c, T5: 'c, F: FnMut(RxInput<'_, 'c, A>) -> (T1, T2, T3, T4, T5) + 'c>(&self, mut compute: F) -> (CRx<'c, T1>, CRx<'c, T2>, CRx<'c, T3>, CRx<'c, T4>, CRx<'c, T5>) {
        let mut input_backwards_offsets = Vec::new();
        let (init1, init2, init3, init4, init5) = Self::run_compute(&mut compute, RxInput(self.sub_dag()), &mut input_backwards_offsets);
        let compute_edge = RxEdgeImpl::<'c, _, A>::new(input_backwards_offsets, 5, move |mut input_backwards_offsets: &mut Vec<usize>, input: RxInput<'_, 'c, A>, outputs: &mut dyn Iterator<Item=&Rx<'c, A>>| {
            input_backwards_offsets.clear();
            let (output1, output2, output3, output4, output5) = Self::run_compute(&mut compute, input, &mut input_backwards_offsets);
            unsafe { outputs.next().unwrap().set_dyn(output1); }
            unsafe { outputs.next().unwrap().set_dyn(output2); }
            unsafe { outputs.next().unwrap().set_dyn(output3); }
            unsafe { outputs.next().unwrap().set_dyn(output4); }
            unsafe { outputs.next().unwrap().set_dyn(output5); }
            debug_assert!(outputs.next().is_none());
        });
        self.0.push(RxDAGElem::Edge(Box::new(compute_edge)));

        let index = self.next_index();
        let rx1 = RxImpl::new(init1);
        let rx2 = RxImpl::new(init2);
        let rx3 = RxImpl::new(init3);
        let rx4 = RxImpl::new(init4);
        let rx5 = RxImpl::new(init5);
        self.0.push(RxDAGElem::<'c>::Node(Box::new(rx1)));
        self.0.push(RxDAGElem::<'c>::Node(Box::new(rx2)));
        self.0.push(RxDAGElem::<'c>::Node(Box::new(rx3)));
        self.0.push(RxDAGElem::<'c>::Node(Box::new(rx4)));
        self.0.push(RxDAGElem::<'c>::Node(Box::new(rx5)));
        (CRx::new(RxRef::new(self, index)), CRx::new(RxRef::new(self, index + 1)), CRx::new(RxRef::new(self, index + 2)), CRx::new(RxRef::new(self, index + 3)), CRx::new(RxRef::new(self, index + 4)))
    }

    // endregion

    fn next_index(&self) -> usize {
        self.0.len()
    }

    fn run_compute<T, F: FnMut(RxInput<'_, 'c, A>) -> T + 'c>(compute: &mut F, input: RxInput<'_, 'c, A>, input_backwards_offsets: &mut Vec<usize>) -> T {
        debug_assert!(input_backwards_offsets.is_empty());

        let result = compute(input);
        let input_indices = input.post_read();

        input_indices
            .into_iter()
            .map(|index| input.0.index - index)
            .collect_into(input_backwards_offsets);
        result
    }

    /// Update all [Var]s with their new values and recompute [CRx]s.
    ///
    /// This requires a shared reference and actually does the "reactive updates".
    pub fn recompute(&mut self) {
        for (index, (before, current, after)) in self.0.as_mut().iter_mut_split3s().enumerate() {
            current.recompute(index, before, after, self.1);
        }

        for current in self.0.as_mut().iter_mut() {
            current.post_recompute();
        }
    }

    /// Recomputes if necessary and then returns an [RxContext] you can use to get the current value.
    pub fn now(&mut self) -> RxDAGSnapshot<'_, 'c, A> {
        self.recompute();
        RxDAGSnapshot(self)
    }

    /// Returns an [RxContext] you can use to get the current value.
    /// However any newly-set values or computations will not be returned until [RxDAG::recompute] is called.
    pub fn stale(&self) -> RxDAGSnapshot<'_, 'c, A> {
        RxDAGSnapshot(self)
    }

    pub(crate) fn id(&self) -> RxDAGUid<'c, A> {
        self.1
    }
}

impl<'a, 'c: 'a, A: Allocator> RxContext<'a, 'c> for RxDAGSnapshot<'a, 'c, A> {
    fn sub_dag(self) -> RxSubDAG<'a, 'c> {
        RxSubDAG {
            before: FrozenSlice::from(&self.0.0),
            index: self.0.0.len(),
            id: self.0.1
        }
    }
}

impl<'a, 'c: 'a, A: Allocator> MutRxContext<'a, 'c> for &'a RxDAG<'c, A> {
    fn sub_dag(self) -> RxSubDAG<'a, 'c> {
        RxDAGSnapshot(self).sub_dag()
    }
}

impl<'a, 'c: 'a, A: Allocator> RxContext<'a, 'c> for RxInput<'a, 'c, A> {
    fn sub_dag(self) -> RxSubDAG<'a, 'c> {
        self.0
    }
}

impl<'a, 'c: 'a, A: Allocator> RxInput<'a, 'c, A> {
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