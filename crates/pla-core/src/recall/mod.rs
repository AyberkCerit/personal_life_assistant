//! The assistant's memory files (`memory/*.md`): small Markdown lists the assistant reads and writes.

pub mod format;

pub use format::{parse, render, topic_key, Entry, Kind, Role};
