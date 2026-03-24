#![deny(missing_docs)]
//! Core parsing primitives for `vaultdex`.

/// Vault-wide indexing orchestration.
pub mod indexer;
/// Parsing support for single Obsidian-style notes.
pub mod parser;
/// SQLite-backed storage for parsed note data.
pub mod storage;

pub use indexer::{IndexError, IndexFailure, IndexReport, index_vault};
pub use parser::{
    NoteStats, ParseNoteError, ParseWarning, ParsedFrontmatter, ParsedHeading, ParsedLink,
    ParsedNote, ParsedTag, TagSource, WarningCode, parse_note_file, parse_note_str,
};
pub use storage::{
    LinkTargetResolution, Note, NoteMetadata, SCHEMA_SQL, StorageError, initialize_schema,
    persist_note, refresh_resolved_link_targets, resolve_target_note_path,
};
