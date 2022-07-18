use std::cell::Cell;
use std::marker::PhantomData;
use std::thread_local;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RxDAGUid<'c>(usize, PhantomData<&'c ()>);

thread_local! {
    static RX_DAG_UID: Cell<usize> = Cell::new(0);
}

impl<'c> RxDAGUid<'c> {
    pub(crate) fn next() -> RxDAGUid<'c> {
        RX_DAG_UID.with(|uid_cell| {
            RxDAGUid(uid_cell.update(|uid| uid + 1), PhantomData)
        })
    }
}

