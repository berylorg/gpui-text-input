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
