//! Turning a model answer into validated items (SRS FR-EXT).

mod prompt;
mod types;

pub use prompt::{messages, request_body, schema, user_message};
pub use types::*;
