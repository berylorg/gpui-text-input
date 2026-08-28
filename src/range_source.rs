//! App-neutral contracts for revision-bound, bounded text-source pages.

mod coordinates;
mod object;
mod page;
mod position;
mod request;

pub use coordinates::*;
pub use object::*;
pub use page::*;
pub use position::*;
pub use request::*;

use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_RESPONSE_INSTANCE: AtomicU64 = AtomicU64::new(1);

pub(crate) fn next_response_instance() -> Option<u64> {
    NEXT_RESPONSE_INSTANCE
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(1)
        })
        .ok()
}
