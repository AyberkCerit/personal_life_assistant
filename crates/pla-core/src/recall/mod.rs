//! The assistant's memory files (`memory/*.md`): small Markdown lists the assistant reads and writes.

pub mod apply;
pub mod extract;
pub mod format;
pub mod load;

pub use apply::{apply, reconcile, rejected, undo, Change, Proposal, RecallError, UNDONE};
pub use format::{parse, parse_doc, render_doc, Block, Doc, render, topic_key, Entry, Kind, Role};
pub use load::{block, load, people_block, Recall, MEMORY_TOKENS, PEOPLE_TOKENS};
