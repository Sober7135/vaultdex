use rusqlite::{Connection, params};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::storage::{
    Note, StorageError, initialize_schema, persist_note, refresh_resolved_link_targets,
};

/// Fatal vault indexing error.
#[derive(Debug)]
pub enum IndexError {
    /// The vault root could not be traversed on disk.
    Io(std::io::Error),
    /// SQLite storage failed while initializing or writing the index.
    Storage(StorageError),
    /// The provided vault root was not a directory.
    InvalidVaultRoot(PathBuf),
}

impl std::fmt::Display for IndexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "vault io error: {err}"),
            Self::Storage(err) => write!(f, "storage error: {err}"),
            Self::InvalidVaultRoot(path) => {
                write!(f, "vault root is not a directory: {}", path.display())
            }
        }
    }
}

impl std::error::Error for IndexError {}

impl From<std::io::Error> for IndexError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<StorageError> for IndexError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}

/// Recoverable per-file failure collected during a vault indexing run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexFailure {
    /// Vault-relative logical path for the file that failed.
    pub path: String,
    /// Human-readable error message captured for the failed file.
    pub message: String,
}

/// Summary of one vault indexing run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexReport {
    /// Number of Markdown files discovered under the vault root.
    pub scanned_files: usize,
    /// Number of files successfully parsed and persisted.
    pub indexed_files: usize,
    /// Number of note rows removed because the files no longer exist in the vault.
    pub deleted_notes: usize,
    /// Recoverable per-file failures collected while the run continued.
    pub failed_files: Vec<IndexFailure>,
}

/// Build or refresh the full note-level index for one vault directory.
///
/// This performs a recursive full scan of `*.md` files, persists each successfully parsed note,
/// and deletes stored notes whose logical paths are no longer present in the vault.
pub fn index_vault(
    conn: &mut Connection,
    vault_root: impl AsRef<Path>,
) -> Result<IndexReport, IndexError> {
    let vault_root = vault_root.as_ref();
    if !vault_root.is_dir() {
        return Err(IndexError::InvalidVaultRoot(vault_root.to_path_buf()));
    }

    initialize_schema(conn)?;

    let markdown_files = collect_markdown_files(vault_root)?;
    let mut live_paths = HashSet::with_capacity(markdown_files.len());
    let mut report = IndexReport {
        scanned_files: markdown_files.len(),
        indexed_files: 0,
        deleted_notes: 0,
        failed_files: Vec::new(),
    };

    for file_path in markdown_files {
        let vault_relative_path = vault_relative_path(vault_root, &file_path)?;

        // Record the path before parsing so a transient read failure does not delete old rows.
        live_paths.insert(vault_relative_path.clone());

        match Note::from_source_and_vault_path(&file_path, &vault_relative_path) {
            Ok(note) => {
                persist_note(conn, &note)?;
                report.indexed_files += 1;
            }
            Err(err) => {
                report.failed_files.push(IndexFailure {
                    path: vault_relative_path,
                    message: err.to_string(),
                });
            }
        }
    }

    report.deleted_notes = delete_notes_not_in_paths(conn, &live_paths)?;
    refresh_resolved_link_targets(conn)?;
    Ok(report)
}

/// Delete note rows whose logical paths are no longer present in the current vault scan.
///
/// This stays in the indexer layer because it is only needed during vault-wide orchestration.
/// The storage layer still owns the schema semantics through `ON DELETE CASCADE`.
fn delete_notes_not_in_paths(
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

fn collect_markdown_files(vault_root: &Path) -> Result<Vec<PathBuf>, IndexError> {
    let mut stack = vec![vault_root.to_path_buf()];
    let mut files = Vec::new();

    while let Some(dir_path) = stack.pop() {
        let mut entries = fs::read_dir(&dir_path)?.collect::<Result<Vec<_>, std::io::Error>>()?;
        entries.sort_by_key(|entry| entry.path());

        for entry in entries {
            let file_type = entry.file_type()?;
            let entry_path = entry.path();

            if file_type.is_dir() {
                stack.push(entry_path);
                continue;
            }

            if file_type.is_file() && is_markdown_file(&entry_path) {
                files.push(entry_path);
            }
        }
    }

    files.sort();
    Ok(files)
}

fn is_markdown_file(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "md")
}

fn vault_relative_path(vault_root: &Path, file_path: &Path) -> Result<String, IndexError> {
    let relative_path = file_path
        .strip_prefix(vault_root)
        .map_err(|_| IndexError::InvalidVaultRoot(vault_root.to_path_buf()))?;
    Ok(relative_path.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use rusqlite::{Connection, params};
    use tempfile::tempdir;

    use super::index_vault;

    #[test]
    fn indexes_recursive_markdown_files_with_relative_paths() {
        let temp = tempdir().expect("create temp dir");
        let vault_root = temp.path();
        std::fs::create_dir_all(vault_root.join("nested")).expect("create nested dir");
        std::fs::write(vault_root.join("root.md"), "# Root\n").expect("write root note");
        std::fs::write(vault_root.join("nested/child.md"), "# Child\n").expect("write child note");
        std::fs::write(vault_root.join("ignore.txt"), "not markdown").expect("write text file");

        let mut conn = Connection::open_in_memory().expect("open sqlite");
        let report = index_vault(&mut conn, vault_root).expect("index vault");

        assert_eq!(report.scanned_files, 2);
        assert_eq!(report.indexed_files, 2);
        assert_eq!(report.deleted_notes, 0);
        assert!(report.failed_files.is_empty());

        let stored_paths: Vec<String> = {
            let mut statement = conn
                .prepare("SELECT path FROM notes ORDER BY path")
                .expect("prepare path query");
            statement
                .query_map([], |row| row.get(0))
                .expect("query paths")
                .collect::<rusqlite::Result<Vec<String>>>()
                .expect("collect paths")
        };

        assert_eq!(stored_paths, vec!["nested/child.md", "root.md"]);
    }

    #[test]
    fn deletes_stale_notes_removed_from_the_vault() {
        let temp = tempdir().expect("create temp dir");
        let vault_root = temp.path();
        std::fs::write(vault_root.join("keep.md"), "# Keep\n").expect("write keep note");
        std::fs::write(vault_root.join("remove.md"), "# Remove\n").expect("write remove note");

        let mut conn = Connection::open_in_memory().expect("open sqlite");
        let first_report = index_vault(&mut conn, vault_root).expect("index initial vault");
        assert_eq!(first_report.indexed_files, 2);

        std::fs::remove_file(vault_root.join("remove.md")).expect("remove note");
        let second_report = index_vault(&mut conn, vault_root).expect("reindex vault");

        assert_eq!(second_report.scanned_files, 1);
        assert_eq!(second_report.indexed_files, 1);
        assert_eq!(second_report.deleted_notes, 1);

        let remaining_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM notes", [], |row| row.get(0))
            .expect("count notes");
        assert_eq!(remaining_count, 1);
    }

    #[test]
    fn indexes_resolved_link_target_paths() {
        let temp = tempdir().expect("create temp dir");
        let vault_root = temp.path();
        std::fs::create_dir_all(vault_root.join("A/B")).expect("create A/B");
        std::fs::create_dir_all(vault_root.join("A/C")).expect("create A/C");
        std::fs::create_dir_all(vault_root.join("D/B")).expect("create D/B");
        std::fs::write(
            vault_root.join("Source.md"),
            "See [[A/B/Note]], [[C/Note]], [[B/Note]], and [[#Local Heading]]\n",
        )
        .expect("write source note");
        std::fs::write(vault_root.join("A/B/Note.md"), "# Note\n").expect("write A/B note");
        std::fs::write(vault_root.join("A/C/Note.md"), "# Note\n").expect("write A/C note");
        std::fs::write(vault_root.join("D/B/Note.md"), "# Note\n").expect("write D/B note");

        let mut conn = Connection::open_in_memory().expect("open sqlite");
        index_vault(&mut conn, vault_root).expect("index vault");

        let stored_targets: Vec<(String, Option<String>)> = {
            let mut statement = conn
                .prepare(
                    "SELECT links.target_note, links.target_note_path
                     FROM links
                     INNER JOIN notes ON notes.id = links.note_id
                     WHERE notes.path = ?1
                     ORDER BY links.id",
                )
                .expect("prepare targets query");
            statement
                .query_map(params!["Source.md"], |row| Ok((row.get(0)?, row.get(1)?)))
                .expect("query targets")
                .collect::<rusqlite::Result<Vec<(String, Option<String>)>>>()
                .expect("collect targets")
        };

        assert_eq!(
            stored_targets,
            vec![
                ("A/B/Note".to_string(), Some("A/B/Note.md".to_string())),
                ("C/Note".to_string(), Some("A/C/Note.md".to_string())),
                ("B/Note".to_string(), None),
                ("".to_string(), Some("Source.md".to_string())),
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn keeps_existing_rows_when_a_present_file_temporarily_fails_to_read() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempdir().expect("create temp dir");
        let vault_root = temp.path();
        let note_path = vault_root.join("locked.md");
        std::fs::write(&note_path, "# Locked\n").expect("write note");

        let mut conn = Connection::open_in_memory().expect("open sqlite");
        index_vault(&mut conn, vault_root).expect("index initial vault");

        let mut permissions = std::fs::metadata(&note_path)
            .expect("stat note")
            .permissions();
        permissions.set_mode(0o000);
        std::fs::set_permissions(&note_path, permissions.clone()).expect("lock note");

        let report = index_vault(&mut conn, vault_root).expect("reindex vault");

        permissions.set_mode(0o644);
        std::fs::set_permissions(&note_path, permissions).expect("unlock note");

        assert_eq!(report.scanned_files, 1);
        assert_eq!(report.indexed_files, 0);
        assert_eq!(report.deleted_notes, 0);
        assert_eq!(report.failed_files.len(), 1);
        assert_eq!(report.failed_files[0].path, "locked.md");

        let stored_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM notes WHERE path = ?1",
                params!["locked.md"],
                |row| row.get(0),
            )
            .expect("count locked note");
        assert_eq!(stored_count, 1);
    }
}
