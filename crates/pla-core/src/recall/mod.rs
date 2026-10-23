//! The assistant's memory files (`memory/*.md`): small Markdown lists the assistant reads and writes.

pub mod apply;
pub mod format;
pub mod load;

pub use apply::{apply, reconcile, rejected, Change, Proposal, RecallError};
pub use format::{parse, parse_doc, render_doc, Block, Doc, render, topic_key, Entry, Kind, Role};
pub use load::{block, load, people_block, Recall, MEMORY_TOKENS, PEOPLE_TOKENS};
