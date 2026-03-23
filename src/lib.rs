#![deny(missing_docs)]
//! Core parsing primitives for `vaultdex`.

/// Parsing support for single Obsidian-style notes.
pub mod parser;

pub use parser::{
    NoteStats, ParseNoteError, ParseWarning, ParsedFrontmatter, ParsedHeading, ParsedLink,
    ParsedNote, ParsedTag, TagSource, WarningCode, parse_note_file, parse_note_str,
};
