use std::alloc::{Allocator, Global};
use std::fmt::Debug;
use std::marker::PhantomData;
use derivative::Derivative;
use crate::dag::{RxDAG, RxContext, MutRxContext};
use crate::dag_uid::RxDAGUid;
use crate::clone_set_fn::CloneSetFn;
use crate::rx_impl::Rx;
use crate::RxSubDAG;

/// Index into the DAG which will give you a node, which may be a variable or computed value.
/// It is untyped though, so you can't interact with it directly.
/// Instead you must re-wrap it in [RxRef] and potentially [Var] or [CRx],
/// which know what type of data and node it is.
///
/// ## RxRef notes
///
/// Technically you the interfaces of [RxRef] and [CRx] are identical.
/// However, it's good practice to use [CRx] whenever you know for sure that you are dealing with
/// a computed value, and only use [RxRef] when you may be dealing with a [Var].
/// It's UB to wrap an [RxRef] belonging to a [Var] inside of a [CRx].
///
/// The DAG and refs have an ID so that you can't use one ref on another DAG, however this is
/// checked at runtime and may be disable-able in future versions.
#[derive(Debug, Derivative)]
#[derivative(Clone(bound = ""), Copy(bound = ""))]
pub struct UntypedRxRef<'c, A: Allocator = Global> {
    index: usize,
    graph_id: RxDAGUid<'c, A>
}

/// [RawRxRef] with type information.So you know the type of data but not whether this is a
/// [Var] (variable) or [CRx] (computed value).
///
/// ## RxRef notes
///
/// Technically you the interfaces of [RxRef] and [CRx] are identical.
/// However, it's good practice to use [CRx] whenever you know for sure that you are dealing with
/// a computed value, and only use [RxRef] when you may be dealing with a [Var].
/// It's UB to wrap an [RxRef] belonging to a [Var] inside of a [CRx].
///
/// The DAG and refs have an ID so that you can't use one ref on another DAG, however this is
/// checked at runtime and may be disable-able in future versions.
#[derive(Debug, Derivative)]
#[derivative(Clone(bound = ""), Copy(bound = ""))]
pub struct RxRef<'c, T, A: Allocator = Global>(UntypedRxRef<'c, A>, PhantomData<T>);

/// Index into the [RxDAG] which will give you a variable of type `T`.
///
/// **Note:** to actually get or set the value you need a shared reference to the [RxDAG].
#[derive(Debug, Derivative)]
#[derivative(Clone(bound = ""), Copy(bound = ""))]
pub struct Var<'c, T, A: Allocator = Global>(RxRef<'c, T, A>);

/// Index into the [RxDAG] which will give you a computed value of type `T`.
///
/// **Note:** to actually get the value you need a shared reference to the [RxDAG].
/// You cannot set the value, instead it's computed from other values.
#[derive(Debug, Derivative)]
#[derivative(Clone(bound = ""), Copy(bound = ""))]
pub struct CRx<'c, T, A: Allocator = Global>(RxRef<'c, T, A>);

/// View and mutate a part of a [Var].
#[derive(Debug)]
pub struct DVar<'c, S, T, GetFn: Fn(&S) -> &T, SetFn: Fn(&S, T) -> S, A: Allocator = Global> {
    source: RxRef<'c, S, A>,
    get: GetFn,
    set: SetFn
}

/// View a part of a [CRx].
#[derive(Debug)]
pub struct DCRx<'c, S, T, GetFn: Fn(&S) -> &T, A: Allocator = Global> {
    source: RxRef<'c, S, A>,
    get: GetFn
}

/// [DVar] where the getter and setter are static.
pub type SDVar<'c, S, T, A = Global> = DVar<'c, S, T, fn(&S) -> &T, fn(&S, T) -> S, A>;

/// [DCRx] where the getter is static.
pub type SDCRx<'c, S, T, A = Global> = DCRx<'c, S, T, fn(&S) -> &T, A>;

impl<'c, A: Allocator> UntypedRxRef<'c, A> {
    fn new(graph: &RxDAG<'c, A>, index: usize) -> Self {
        UntypedRxRef {
            index,
            graph_id: graph.id(),
        }
    }

    /// Get the underlying [Rx] where the data is stored.
    fn get_rx<'a>(self, graph: RxSubDAG<'a, 'c, A>) -> &'a Rx<'c, A> where 'c: 'a {
        debug_assert!(self.graph_id == graph.id, "RxRef::get_rx: different graph");
        debug_assert!(self.index < graph.before.len(), "RxRef refers to a future node (not a DAG?)");
        // Since we already checked the index, we can use get_unchecked
        let elem = unsafe { graph.before.get_unchecked(self.index) };
        elem.as_node().expect("RxRef is corrupt: it points to an edge")
    }
}

impl<'c, T, A: Allocator + 'c> RxRef<'c, T, A> {
    pub(crate) fn new(graph: &RxDAG<'c, A>, index: usize) -> Self {
        RxRef(UntypedRxRef::new(graph, index), PhantomData)
    }

    /// Construct a (typed) [RxRef] from an [UntypedRxRef].
    /// You are responsible for ensuring that it came from `RxRef<T>::raw`, where `T` is the correct type.
    pub unsafe fn from_raw(raw: UntypedRxRef<'c, A>) -> Self {
        RxRef(raw, PhantomData)
    }

    /// Get the [RxRef] from this [Var].
    /// This is safe because you can't interact with the [UntypedRxRef]'s untyped values directly.
    pub fn raw(self) -> UntypedRxRef<'c, A> {
        self.0
    }


    /// Read the node. You can do this on both [Var] and [CRx].
    pub fn get<'a>(self, c: impl RxContext<'a, 'c, A>) -> &'a T where 'c: 'a {
        unsafe { self.0.get_rx(c.sub_dag()).get_dyn() }
    }

    /// Write a new value to the node. The changes will be applied on recompute.
    fn set<'a>(self, c: impl MutRxContext<'a, 'c, A>, value: T) where 'c: 'a {
        unsafe { self.0.get_rx(c.sub_dag()).set_dyn(value); }
    }

    /// Apply a transformation to the latest value. If `set` this will apply to the recently-set value.
    /// This must be used instead of chaining [RxRef::set] and [RxRef::get], since setting a value doesn't make it
    /// returned by [RxRef::get] until the graph is recomputed.
    ///
    /// Like `set` the changes only actually reflect in [RxRef::get] on recompute.
    fn modify<'a, F: FnOnce(&T) -> T>(self, c: impl MutRxContext<'a, 'c, A>, modify: F) where 'c: 'a {
        let rx = self.0.get_rx(c.sub_dag());

        let latest = unsafe { rx.take_latest_dyn() };
        let next = modify(latest.as_ref());
        unsafe { rx.set_dyn(next); }
    }
}

impl<'c, T, A: Allocator + 'c> Var<'c, T, A> {
    pub(crate) fn new(internal: RxRef<'c, T, A>) -> Self {
        Var(internal)
    }

    /// Construct a [Var] from an [RxRef].
    /// You are responsible for ensuring that it came from [Var::raw] and not [CRx::raw].
    pub unsafe fn from_raw(raw: RxRef<'c, T, A>) -> Self {
        Var(raw)
    }

    /// Get the [RxRef] from this [Var].
    pub fn raw(self) -> RxRef<'c, T, A> {
        self.0
    }

    /// Read the variable
    pub fn get<'a>(self, c: impl RxContext<'a, 'c, A>) -> &'a T where 'c: 'a {
        self.0.get(c)
    }

    /// Write a new value to the variable. The changes will be applied on recompute.
    pub fn set<'a>(self, c: impl MutRxContext<'a, 'c, A>, value: T) where 'c: 'a {
        self.0.set(c, value);
    }

    /// Apply a transformation to the latest value. If [Var::set] this will apply to the recently-set value.
    /// This must be used instead of chaining [Var::set] and [Var::get], since setting a value doesn't make it
    /// returned by [Var::get] until the graph is recomputed.
    ///
    /// Like `set` the changes only actually reflect in [Var::get] on recompute.
    pub fn modify<'a, F: FnOnce(&T) -> T>(self, c: impl MutRxContext<'a, 'c, A>, modify: F) where 'c: 'a {
        self.0.modify(c, modify)
    }

    /// Create a view of part of the variable.
    ///
    /// Do know that `SetFn` will take the most recently-set value even if the graph hasn't been recomputed.
    /// This means you can create multiple `derive`s and set them all before recompute, and you don't have to worry
    /// about the later derived values setting their part on the stale whole.
    pub fn derive<U, GetFn: Fn(&T) -> &U, SetFn: Fn(&T, U) -> T>(self, get: GetFn, set: SetFn) -> DVar<'c, T, U, GetFn, SetFn, A> {
        DVar {
            source: self.0,
            get,
            set
        }
    }

    /// Create a view of part of the variable, which clones the value on set.
    ///
    /// Do know that `SetFn` will take the most recently-set value even if the graph hasn't been recomputed.
    /// This means you can create multiple `derive`s and set them all before recompute, and you don't have to worry
    /// about the later derived values setting their part on the stale whole.
    pub fn derive_using_clone<U, GetFn: Fn(&T) -> &U, SetFn: Fn(&mut T, U)>(self, get: GetFn, set: SetFn) -> DVar<'c, T, U, GetFn, CloneSetFn<T, U, SetFn>, A> where T: Clone {
        self.derive(get, CloneSetFn::new(set))
    }
}

impl<'c, T, A: Allocator + 'c> CRx<'c, T, A> {
    pub(crate) fn new(internal: RxRef<'c, T, A>) -> Self {
        CRx(internal)
    }

    /// Construct a [CRx] from an [RxRef].
    /// You are responsible for ensuring that it came from [CRx::raw] and not [Var::raw].
    pub unsafe fn from_raw(raw: RxRef<'c, T, A>) -> Self {
        CRx(raw)
    }

    /// Get the [UntypedRxRef] from this [CRx]. This is safe because you can't interact with the [UntypedRxRef] directly.
    pub fn raw(self) -> RxRef<'c, T, A> {
        self.0
    }

    /// Read the computed value
    pub fn get<'a>(self, c: impl RxContext<'a, 'c, A>) -> &'a T where 'c: 'a {
        self.0.get(c)
    }

    /// Create a view of part of the computed value.
    pub fn derive<U, GetFn: Fn(&T) -> &U>(self, get: GetFn) -> DCRx<'c, T, U, GetFn, A> {
        DCRx {
            source: self.0,
            get
        }
    }
}

impl<'c, S, T, GetFn: Fn(&S) -> &T, SetFn: Fn(&S, T) -> S, A: Allocator + 'c> DVar<'c, S, T, GetFn, SetFn, A> {
    /// Read the part of the variable this view gets.
    pub fn get<'a>(&self, c: impl RxContext<'a, 'c, A>) -> &'a T where 'c: 'a, S: 'a {
        (self.get)(self.source.get(c))
    }

    /// Write a new value to the part of the variable this view gets.
    ///
    /// Do know that this uses the most recently-set value even if the graph hasn't been recomputed.
    /// This means you can create multiple `derive`s and set them all before recompute, and you don't have to worry
    /// about the later derived values setting their part on the stale whole.
    pub fn set<'a>(&self, c: impl MutRxContext<'a, 'c, A>, value: T) where 'c: 'a, S: 'a {
        self.source.modify(c, move |old_value| {
            (self.set)(old_value, value)
        })
    }
}

impl<'c, S, T, GetFn: Fn(&S) -> &T, A: Allocator + 'c> DCRx<'c, S, T, GetFn, A> {
    /// Read the part of the computed value this view gets.
    pub fn get<'a>(&self, c: impl RxContext<'a, 'c, A>) -> &'a T where 'c: 'a, S: 'a {
        (self.get)(self.source.get(c))
    }
}

