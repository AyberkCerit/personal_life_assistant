//! Turning a model answer into validated items (SRS FR-EXT).

mod dates;
mod prompt;
mod types;

pub use dates::{resolve_date, DateError};
pub use prompt::{messages, request_body, schema, user_message};
pub use types::*;
