# AGENTS.md

## Purpose

This repository is building `vaultdex`, a Rust CLI and library for indexing and querying Obsidian-style vault content.

The current implementation focus is:

- single-note parsing
- note-level indexing
- heading-aware metadata
- small, explicit parser contracts

## Working Style

- Prefer correctness, clarity, and small local changes.
- Do not introduce broad refactors or abstraction layers without a concrete need.
- Preserve existing naming and structure unless there is a clear semantic problem.
- Keep parser behavior explicit and test-backed.

## Architecture Direction

- Treat note identity as path-based.
- Use `note_name = path` with a trailing `.md` suffix removed when present.
- Do not derive note identity from frontmatter `title` or from headings.
- Keep parser concerns separate from indexing and vault-wide resolution concerns.

## Parser Rules

- Frontmatter is parsed only from a fully closed top-of-file `--- ... ---` block.
- If the closing delimiter is missing, treat the whole file as ordinary body text.
- Frontmatter fields must normalize to a top-level key-value map.
- Invalid or unsupported frontmatter should produce warnings instead of crashing the parse.
- Obsidian-specific scanning must ignore code-like regions such as code fences and inline code.

## Comments Format

Use comments sparingly. Add them when the code would otherwise hide an important invariant, boundary, or design reason.

For both doc comments and normal comments, use this structure:

1. Start with one short sentence that gives the conclusion.
2. Follow with one or two short sentences that explain the important detail, edge case, or design reason.

Comment guidelines:

- Do not write labels like `TL;DR:` or `Details:`.
- Prefer comments that explain why or what invariant matters, not line-by-line narration.
- Keep comments compact and factual.
- Prefer ASCII.
- Update comments when behavior changes.

Examples of good comment shape:

```rust
/// Parse one Obsidian-style note into the normalized Phase 1 model.
///
/// This slices frontmatter first, then parses the Markdown body and records
/// recoverable issues as warnings instead of failing the whole note.
```

```rust
// Exclude the full code block span so later scans can do a single overlap check.
```

Examples to avoid:

```rust
// TL;DR: parse the note.
// Details: do several things.
```

```rust
// Increment i by one.
```

## Testing

- Prefer narrow tests first.
- Use `insta` snapshots for structured parser output when they provide meaningful regression coverage.
- Keep targeted assertions for fragile values that should not be over-snapshotted, such as third-party error strings.
- Do not claim verification unless `cargo test` or an equivalent command was actually run.

## Current Validation Baseline

Before finishing parser changes, run:

```bash
cargo fmt
cargo test
cargo clippy --all -- -D warnings
```
