use std::fmt::Debug;
use std::marker::PhantomData;
use derivative::Derivative;
use crate::dag::{RxDAG, RxSubDAG, RxContext, MutRxContext};
use crate::dag_uid::RxDAGUid;
use crate::clone_set_fn::CloneSetFn;
use crate::rx_impl::{CurrentOrNext, Rx};

/// Index into the DAG which will give you an `Rx` value.
/// However, to get or set the value you need a shared reference to the `DAG`.
///
/// The DAG and refs have an ID so that you can't use one ref on another DAG, however this is checked at runtime.
/// The lifetimes are checked at compile-time though.
#[derive(Debug, Derivative)]
#[derivative(Clone(bound = ""), Copy(bound = ""))]
pub(crate) struct RxRef<'c, T> {
    index: usize,
    graph_id: RxDAGUid<'c>,
    phantom: PhantomData<T>
}

/// Index into the DAG which will give you an `Rx` variable.
/// However, to get or set the value you need a shared reference to the `DAG`.
/// This value is not computed from other values, instead you set it directly.
#[derive(Debug, Derivative)]
#[derivative(Clone(bound = ""), Copy(bound = ""))]
pub struct Var<'c, T>(RxRef<'c, T>);

/// Index into the DAG which will give you a computed `Rx` value.
/// However, to get the value you need a shared reference to the `DAG`.
/// You cannot set the value because it's computed from other values.
#[derive(Debug, Derivative)]
#[derivative(Clone(bound = ""), Copy(bound = ""))]
pub struct CRx<'c, T>(RxRef<'c, T>);

/// View and mutate part of a `Var`
#[derive(Debug)]
pub struct DVar<'c, S, T, GetFn: Fn(&S) -> &T, SetFn: Fn(&S, T) -> S> {
    source: RxRef<'c, S>,
    get: GetFn,
    set: SetFn
}

#[derive(Debug)]
pub struct DCRx<'c, S, T, GetFn: Fn(&S) -> &T> {
    source: RxRef<'c, S>,
    get: GetFn
}

impl<'c, T> RxRef<'c, T> {
    pub(crate) fn new(graph: &RxDAG<'c>, index: usize) -> Self {
        RxRef {
            index,
            graph_id: graph.id(),
            phantom: PhantomData
        }
    }

    fn get<'a>(self, graph: RxSubDAG<'a, 'c>) -> &'a T where 'c: 'a {
        unsafe { self.get_rx(graph).get_dyn() }
    }

    /// Take `next` if set, otherwise returns a reference to `current`.
    /// The value should then be re-passed to `next` via `set`
    fn take_latest<'a>(self, graph: RxSubDAG<'a, 'c>) -> CurrentOrNext<'a, T> where 'c: 'a {
        unsafe { self.get_rx(graph).take_latest_dyn() }
    }

    fn set(self, graph: RxSubDAG<'_, 'c>, value: T) {
        unsafe { self.get_rx(graph).set_dyn(value); }
    }

    /// Apply a transformation to the latest value.
    /// This must be used instead of chaining `set` and `get`, since setting a value doesn't make it
    /// returned by `get` until the graph is recomputed.
    fn modify<F: FnOnce(&T) -> T>(self, graph: RxSubDAG<'_, 'c>, modify: F) {
        let latest = self.take_latest(graph);
        let next = modify(latest.as_ref());
        self.set(graph, next);
    }

    fn get_rx<'a>(self, graph: RxSubDAG<'a, 'c>) -> &'a Rx<'c> where 'c: 'a {
        debug_assert!(self.graph_id == graph.id, "RxRef::get_rx: different graph");
        debug_assert!(self.index < graph.before.len(), "RxRef refers to a future node (not a DAG?)");
        // Since we already checked the index, we can use get_unchecked
        let elem = unsafe { graph.before.get_unchecked(self.index) };
        elem.as_node().expect("RxRef is corrupt: it points to an edge")
    }
}

impl<'c, T> Var<'c, T> {
    pub(crate) fn new(internal: RxRef<'c, T>) -> Self {
        Var(internal)
    }

    pub fn get<'a>(self, c: impl RxContext<'a, 'c>) -> &'a T where 'c: 'a {
        let graph = c.sub_dag();
        self.0.get(graph)
    }

    pub fn set<'a>(self, c: impl MutRxContext<'a, 'c>, value: T) where 'c: 'a {
        let graph = c.sub_dag();
        self.0.set(graph, value);
    }

    /// Apply a transformation to the latest value.
    /// This must be used instead of chaining `set` and `get`, since setting a value doesn't make it
    /// returned by `get` until the graph is recomputed.
    pub fn modify<'a, F: FnOnce(&T) -> T>(self, c: impl MutRxContext<'a, 'c>, modify: F) where 'c: 'a {
        let graph = c.sub_dag();
        self.0.modify(graph, modify)
    }

    pub fn derive<U, GetFn: Fn(&T) -> &U, SetFn: Fn(&T, U) -> T>(self, get: GetFn, set: SetFn) -> DVar<'c, T, U, GetFn, SetFn> {
        DVar {
            source: self.0,
            get,
            set
        }
    }

    pub fn derive_using_clone<U, GetFn: Fn(&T) -> &U, SetFn: Fn(&mut T, U)>(self, get: GetFn, set: SetFn) -> DVar<'c, T, U, GetFn, CloneSetFn<T, U, SetFn>> where T: Clone {
        self.derive(get, CloneSetFn::new(set))
    }
}

impl<'c, T> CRx<'c, T> {
    pub(crate) fn new(internal: RxRef<'c, T>) -> Self {
        CRx(internal)
    }

    pub fn get<'a>(self, c: impl RxContext<'a, 'c>) -> &'a T where 'c: 'a {
        let graph = c.sub_dag();
        self.0.get(graph)
    }

    pub fn derive<U, GetFn: Fn(&T) -> &U>(self, get: GetFn) -> DCRx<'c, T, U, GetFn> {
        DCRx {
            source: self.0,
            get
        }
    }
}

impl<'c, S, T, GetFn: Fn(&S) -> &T, SetFn: Fn(&S, T) -> S> DVar<'c, S, T, GetFn, SetFn> {
    pub fn get<'a>(&self, c: impl RxContext<'a, 'c>) -> &'a T where 'c: 'a, S: 'a {
        let graph = c.sub_dag();
        (self.get)(self.source.get(graph))
    }

    pub fn set<'a>(&self, c: impl MutRxContext<'a, 'c>, value: T) where 'c: 'a, S: 'a {
        let graph = c.sub_dag();
        let old_value = self.source.take_latest(graph);
        let new_value = (self.set)(old_value.as_ref(), value);
        self.source.set(graph, new_value)
    }
}

impl<'c, S, T, GetFn: Fn(&S) -> &T> DCRx<'c, S, T, GetFn> {
    pub fn get<'a>(&self, c: impl RxContext<'a, 'c>) -> &'a T where 'c: 'a, S: 'a {
        let graph = c.sub_dag();
        (self.get)(self.source.get(graph))
    }
}

