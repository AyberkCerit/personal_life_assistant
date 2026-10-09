//! Turning a model answer into validated items (SRS FR-EXT).

mod dates;
mod prompt;
mod types;
mod validate;

pub use dates::{resolve_date, DateError};
pub use prompt::{messages, request_body, schema, user_message};
pub use types::*;
pub mod guard;
pub use guard::guard;
pub use validate::{canonical_value, validate, InvalidReason, ValidItem, ValidationSettings};
pub(crate) use validate::plausible_range;
