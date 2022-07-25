use std::alloc::Allocator;
use std::cell::Cell;
use std::marker::PhantomData;
use std::thread_local;
use derivative::Derivative;

#[derive(Debug, Derivative)]
#[derivative(Clone(bound = ""), Copy(bound = ""), PartialEq(bound = ""), Eq(bound = ""))]
pub(crate) struct RxDAGUid<'c, A: Allocator>(usize, PhantomData<(&'c (), A)>);

thread_local! {
    static RX_DAG_UID: Cell<usize> = Cell::new(0);
}

impl<'c, A: Allocator> RxDAGUid<'c, A> {
    pub(crate) fn next() -> RxDAGUid<'c, A> {
        RX_DAG_UID.with(|uid_cell| {
            RxDAGUid(uid_cell.update(|uid| uid + 1), PhantomData)
        })
    }
}

