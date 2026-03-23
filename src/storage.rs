use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::collections::HashSet;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::parser::{ParseNoteError, ParsedNote, TagSource};

/// SQL schema used by the current storage layer.
pub const SCHEMA_SQL: &str = include_str!("storage/schema.sql");

/// Fatal storage error returned while initializing or writing the database.
#[derive(Debug)]
pub enum StorageError {
    /// SQLite returned an error.
    Sqlite(rusqlite::Error),
    /// JSON serialization failed while storing structured fields.
    Json(serde_json::Error),
    /// Parsing a note from disk failed before persistence started.
    Parse(ParseNoteError),
    /// The local clock could not be converted into a Unix timestamp.
    Time(std::time::SystemTimeError),
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(err) => write!(f, "sqlite error: {err}"),
            Self::Json(err) => write!(f, "json serialization error: {err}"),
            Self::Parse(err) => write!(f, "note parse error: {err}"),
            Self::Time(err) => write!(f, "time conversion error: {err}"),
        }
    }
}

impl std::error::Error for StorageError {}

impl From<rusqlite::Error> for StorageError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sqlite(value)
    }
}

impl From<serde_json::Error> for StorageError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<ParseNoteError> for StorageError {
    fn from(value: ParseNoteError) -> Self {
        Self::Parse(value)
    }
}

impl From<std::time::SystemTimeError> for StorageError {
    fn from(value: std::time::SystemTimeError) -> Self {
        Self::Time(value)
    }
}

/// Optional metadata supplied by the caller when persisting one parsed note.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoteMetadata {
    /// Stored note path used as the stable identity in the index.
    pub path: String,
    /// Source file modification time stored as a Unix timestamp in seconds.
    pub mtime: Option<i64>,
}

impl NoteMetadata {
    /// Build note metadata from a source file path and a vault-relative path.
    ///
    /// The source path is used only for filesystem access and `mtime`. The vault-relative path
    /// becomes the stored note identity in the index.
    pub(crate) fn from_source_and_vault_path(
        source_path: impl AsRef<Path>,
        vault_relative_path: impl Into<String>,
    ) -> Self {
        Self {
            path: vault_relative_path.into(),
            mtime: read_mtime(source_path),
        }
    }
}

/// Storage-facing note input that bundles parsed content with persistence metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// Parsed note content produced by the parser layer.
    pub parsed: ParsedNote,
    /// Extra metadata gathered by the caller outside the parser.
    pub metadata: NoteMetadata,
}

impl Note {
    /// Build a storage note from parsed content and optional persistence metadata.
    ///
    /// The parser stays focused on Markdown semantics. Filesystem-derived values such as `mtime`
    /// are attached here so storage can persist them without widening the parser contract.
    pub fn new(parsed: ParsedNote, metadata: NoteMetadata) -> Self {
        Self { parsed, metadata }
    }

    /// Parse one note from disk and attach vault-aware metadata.
    ///
    /// The source path is used to read the file and collect `mtime`. The vault-relative path is
    /// stored in metadata, which keeps the parser output path-free and pure.
    pub fn from_source_and_vault_path(
        source_path: impl AsRef<Path>,
        vault_relative_path: impl Into<String>,
    ) -> Result<Self, StorageError> {
        let parsed = crate::parser::parse_note_file(&source_path)?;
        Ok(Self::new(
            parsed,
            NoteMetadata::from_source_and_vault_path(source_path, vault_relative_path),
        ))
    }
}

/// Create the current schema in an existing SQLite connection.
///
/// The schema is intentionally small and local to the current Phase 1 note-level index.
/// Calling this more than once is safe because the statements are idempotent.
pub fn initialize_schema(conn: &Connection) -> Result<(), StorageError> {
    conn.execute_batch(SCHEMA_SQL)?;
    Ok(())
}

/// Upsert one storage note and rebuild its child rows inside a single transaction.
///
/// This treats `NoteMetadata.path` as the stable note identity. On update, the note row is kept
/// and all headings, links, and tags for that note are deleted and re-inserted from scratch.
pub fn persist_note(conn: &mut Connection, note: &Note) -> Result<i64, StorageError> {
    let tx = conn.transaction()?;
    let note_id = upsert_note(&tx, note)?;
    replace_headings(&tx, note_id, &note.parsed)?;
    replace_links(&tx, note_id, &note.parsed)?;
    replace_tags(&tx, note_id, &note.parsed)?;
    tx.commit()?;
    Ok(note_id)
}

/// Delete note rows whose logical paths are no longer present in the current vault scan.
///
/// This keeps the database consistent after files are deleted or moved in the vault. Child rows
/// disappear automatically because the schema uses `ON DELETE CASCADE`.
// This helper is consumed by the later indexer stack layer.
// Keep it local here so storage semantics land before vault orchestration.
#[allow(dead_code)]
pub(crate) fn delete_notes_not_in_paths(
    conn: &Connection,
    live_paths: &HashSet<String>,
) -> Result<usize, StorageError> {
    let mut statement = conn.prepare("SELECT path FROM notes ORDER BY path")?;
    let stored_paths = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;

    let mut deleted_count = 0;
    for path in stored_paths {
        if live_paths.contains(&path) {
            continue;
        }

        deleted_count += conn.execute("DELETE FROM notes WHERE path = ?1", params![path])?;
    }

    Ok(deleted_count)
}

fn upsert_note(tx: &Transaction<'_>, note: &Note) -> Result<i64, StorageError> {
    let frontmatter_json = note
        .parsed
        .frontmatter
        .as_ref()
        .map(|frontmatter| serde_json::to_string(&frontmatter.fields))
        .transpose()?;
    let content_hash = blake3::hash(note.parsed.raw_text.as_bytes())
        .to_hex()
        .to_string();
    let index_at = current_unix_timestamp()?;

    let existing_id: Option<i64> = tx
        .query_row(
            "SELECT id FROM notes WHERE path = ?1",
            params![note.metadata.path],
            |row| row.get(0),
        )
        .optional()?;

    match existing_id {
        Some(note_id) => {
            tx.execute(
                "UPDATE notes
                 SET mtime = ?1,
                     content_hash = ?2,
                     index_at = ?3,
                     frontmatter_json = ?4,
                     content = ?5,
                     word_count = ?6,
                     line_count = ?7
                 WHERE id = ?8",
                params![
                    note.metadata.mtime,
                    content_hash,
                    index_at,
                    frontmatter_json,
                    note.parsed.body_text,
                    note.parsed.stats.word_count as i64,
                    note.parsed.stats.line_count as i64,
                    note_id
                ],
            )?;
            delete_note_children(tx, note_id)?;
            Ok(note_id)
        }
        None => {
            tx.execute(
                "INSERT INTO notes (
                    path,
                    mtime,
                    content_hash,
                    index_at,
                    frontmatter_json,
                    content,
                    word_count,
                    line_count
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    note.metadata.path,
                    note.metadata.mtime,
                    content_hash,
                    index_at,
                    frontmatter_json,
                    note.parsed.body_text,
                    note.parsed.stats.word_count as i64,
                    note.parsed.stats.line_count as i64
                ],
            )?;
            Ok(tx.last_insert_rowid())
        }
    }
}

fn delete_note_children(tx: &Transaction<'_>, note_id: i64) -> Result<(), StorageError> {
    // Only used when an existing note row is being refreshed from a new parse result.
    // Child tables are currently rebuild-only caches, so stale rows must be cleared before reinsert.
    tx.execute(
        "DELETE FROM note_headings WHERE note_id = ?1",
        params![note_id],
    )?;
    tx.execute("DELETE FROM links WHERE note_id = ?1", params![note_id])?;
    tx.execute("DELETE FROM tags WHERE note_id = ?1", params![note_id])?;
    Ok(())
}

/// Return the current wall-clock time as Unix seconds.
///
/// Storage keeps timestamps as plain integers so SQLite queries stay simple and portable.
fn current_unix_timestamp() -> Result<i64, StorageError> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64)
}

/// Read a file modification time as Unix seconds when the filesystem exposes it.
///
/// This is best-effort because `mtime` is optional storage metadata. Missing metadata should not
/// block indexing as long as the note content itself was readable.
fn read_mtime(path: impl AsRef<Path>) -> Option<i64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    modified
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs() as i64)
}

fn replace_headings(
    tx: &Transaction<'_>,
    note_id: i64,
    note: &ParsedNote,
) -> Result<(), StorageError> {
    let mut statement = tx.prepare(
        "INSERT INTO note_headings (
            note_id,
            heading_path_json,
            level,
            title,
            normalized_text,
            start_line,
            end_line
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;

    for heading in &note.headings {
        let heading_path_json = serde_json::to_string(&heading.heading_path)?;
        statement.execute(params![
            note_id,
            heading_path_json,
            i64::from(heading.level),
            heading.text,
            heading.normalized_text,
            heading.start_line as i64,
            heading.end_line as i64
        ])?;
    }

    Ok(())
}

fn replace_links(
    tx: &Transaction<'_>,
    note_id: i64,
    note: &ParsedNote,
) -> Result<(), StorageError> {
    let mut statement = tx.prepare(
        "INSERT INTO links (
            note_id,
            raw,
            target_note,
            target_heading,
            alias,
            is_embed,
            line,
            byte_start,
            byte_end
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?;

    for link in &note.links {
        statement.execute(params![
            note_id,
            link.raw,
            link.target_note,
            link.target_heading,
            link.alias,
            if link.is_embed { 1_i64 } else { 0_i64 },
            link.line as i64,
            link.byte_start as i64,
            link.byte_end as i64
        ])?;
    }

    Ok(())
}

fn replace_tags(tx: &Transaction<'_>, note_id: i64, note: &ParsedNote) -> Result<(), StorageError> {
    let mut statement = tx.prepare(
        "INSERT INTO tags (
            note_id,
            raw,
            normalized,
            source,
            line
         ) VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;

    for tag in &note.tags {
        let source = match tag.source {
            TagSource::Inline => "inline",
            TagSource::Frontmatter => "frontmatter",
        };

        statement.execute(params![
            note_id,
            tag.raw,
            tag.normalized,
            source,
            tag.line.map(|line| line as i64)
        ])?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use rusqlite::{Connection, params};
    use tempfile::tempdir;

    use crate::parser::parse_note_str;

    use super::{Note, NoteMetadata, initialize_schema, persist_note};

    #[test]
    fn persists_parsed_note_into_all_tables() {
        let mut conn = Connection::open_in_memory().expect("open in-memory sqlite");
        initialize_schema(&conn).expect("initialize schema");

        let note = parse_note_str(
            r#"---
tags:
  - systems
---
# Distributed Systems

See [[CAP]] and #distributed
"#,
        );
        let expected_content_hash = blake3::hash(note.raw_text.as_bytes()).to_hex().to_string();
        let expected_body_text = note.body_text.clone();
        let expected_word_count = note.stats.word_count as i64;
        let expected_line_count = note.stats.line_count as i64;

        let note_id = persist_note(
            &mut conn,
            &Note::new(
                note,
                NoteMetadata {
                    path: "systems/distributed.md".to_string(),
                    mtime: Some(1_700_000_000),
                },
            ),
        )
        .expect("persist note");

        let stored_note: (String, Option<i64>, String, i64, Option<String>, String, i64, i64) = conn
            .query_row(
                "SELECT path, mtime, content_hash, index_at, frontmatter_json, content, word_count, line_count
                 FROM notes WHERE id = ?1",
                params![note_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                    ))
                },
            )
            .expect("load stored note");

        assert_eq!(stored_note.0, "systems/distributed.md");
        assert_eq!(stored_note.1, Some(1_700_000_000));
        assert_eq!(stored_note.2, expected_content_hash);
        assert!(stored_note.3 > 0);
        assert_eq!(stored_note.4.as_deref(), Some(r#"{"tags":["systems"]}"#));
        assert_eq!(stored_note.5, expected_body_text);
        assert_eq!(stored_note.6, expected_word_count);
        assert_eq!(stored_note.7, expected_line_count);

        let heading_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM note_headings WHERE note_id = ?1",
                params![note_id],
                |row| row.get(0),
            )
            .expect("count headings");
        let link_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM links WHERE note_id = ?1",
                params![note_id],
                |row| row.get(0),
            )
            .expect("count links");
        let tag_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tags WHERE note_id = ?1",
                params![note_id],
                |row| row.get(0),
            )
            .expect("count tags");

        assert_eq!(heading_count, 1);
        assert_eq!(link_count, 1);
        assert_eq!(tag_count, 2);
    }

    #[test]
    fn persist_note_accepts_grouped_storage_input() {
        let mut conn = Connection::open_in_memory().expect("open in-memory sqlite");
        initialize_schema(&conn).expect("initialize schema");

        let parsed = parse_note_str("# Distributed Systems\n");
        let note = Note::new(
            parsed,
            NoteMetadata {
                path: "systems/distributed.md".to_string(),
                mtime: Some(1_700_000_123),
            },
        );

        let note_id = persist_note(&mut conn, &note).expect("persist note");
        let stored: (String, Option<i64>) = conn
            .query_row(
                "SELECT path, mtime FROM notes WHERE id = ?1",
                params![note_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("load stored note");

        assert_eq!(stored.0, "systems/distributed.md");
        assert_eq!(stored.1, Some(1_700_000_123));
    }

    #[test]
    fn persist_parsed_note_replaces_child_rows_on_update() {
        let mut conn = Connection::open_in_memory().expect("open in-memory sqlite");
        initialize_schema(&conn).expect("initialize schema");

        let original = parse_note_str("# Distributed Systems\n\nSee [[CAP]] and #distributed\n");
        let updated =
            parse_note_str("# Distributed Systems\n\n## Raft\n\nSee [[Raft]] and #consensus\n");

        let original_note = Note::new(
            original,
            NoteMetadata {
                path: "systems/distributed.md".to_string(),
                mtime: None,
            },
        );
        let updated_note = Note::new(
            updated,
            NoteMetadata {
                path: "systems/distributed.md".to_string(),
                mtime: None,
            },
        );

        let original_id = persist_note(&mut conn, &original_note).expect("persist original");
        let updated_id = persist_note(&mut conn, &updated_note).expect("persist updated");

        assert_eq!(original_id, updated_id);

        let heading_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM note_headings WHERE note_id = ?1",
                params![updated_id],
                |row| row.get(0),
            )
            .expect("count headings");
        let link_targets: Vec<String> = {
            let mut statement = conn
                .prepare("SELECT target_note FROM links WHERE note_id = ?1 ORDER BY id")
                .expect("prepare links query");
            statement
                .query_map(params![updated_id], |row| row.get(0))
                .expect("query links")
                .collect::<rusqlite::Result<Vec<String>>>()
                .expect("collect links")
        };
        let tags: Vec<String> = {
            let mut statement = conn
                .prepare("SELECT normalized FROM tags WHERE note_id = ?1 ORDER BY id")
                .expect("prepare tags query");
            statement
                .query_map(params![updated_id], |row| row.get(0))
                .expect("query tags")
                .collect::<rusqlite::Result<Vec<String>>>()
                .expect("collect tags")
        };

        assert_eq!(heading_count, 2);
        assert_eq!(link_targets, vec!["Raft"]);
        assert_eq!(tags, vec!["#consensus"]);
    }

    #[test]
    fn persist_parsed_note_refreshes_content_hash_and_index_at() {
        let mut conn = Connection::open_in_memory().expect("open in-memory sqlite");
        initialize_schema(&conn).expect("initialize schema");

        let original = parse_note_str("# Distributed Systems\n");
        let updated = parse_note_str("# Distributed Systems\n\nSee [[CAP]]\n");

        let original_note = Note::new(
            original.clone(),
            NoteMetadata {
                path: "systems/distributed.md".to_string(),
                mtime: None,
            },
        );
        let updated_note = Note::new(
            updated.clone(),
            NoteMetadata {
                path: "systems/distributed.md".to_string(),
                mtime: None,
            },
        );

        let note_id = persist_note(&mut conn, &original_note).expect("persist original");
        let (original_hash, original_index_at): (String, i64) = conn
            .query_row(
                "SELECT content_hash, index_at FROM notes WHERE id = ?1",
                params![note_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("load original metadata");

        std::thread::sleep(std::time::Duration::from_secs(1));

        persist_note(&mut conn, &updated_note).expect("persist updated");
        let (updated_hash, updated_index_at): (String, i64) = conn
            .query_row(
                "SELECT content_hash, index_at FROM notes WHERE id = ?1",
                params![note_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("load updated metadata");

        assert_eq!(
            original_hash,
            blake3::hash(original.raw_text.as_bytes())
                .to_hex()
                .to_string()
        );
        assert_eq!(
            updated_hash,
            blake3::hash(updated.raw_text.as_bytes())
                .to_hex()
                .to_string()
        );
        assert_ne!(original_hash, updated_hash);
        assert!(updated_index_at >= original_index_at);
    }

    #[test]
    fn note_from_path_parses_file_and_collects_mtime() {
        let temp = tempdir().expect("create temp dir");
        let path = temp.path().join("distributed.md");
        std::fs::write(&path, "# Distributed Systems\n\nSee [[CAP]]\n").expect("write note");

        let note = Note::from_source_and_vault_path(&path, path.to_string_lossy().into_owned())
            .expect("build note from path");

        assert_eq!(note.metadata.path, path.to_string_lossy());
        assert!(note.metadata.mtime.is_some());
    }
}
