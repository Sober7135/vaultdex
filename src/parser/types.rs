use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Fatal error returned when a note cannot be loaded for parsing.
#[derive(Debug)]
pub enum ParseNoteError {
    /// Reading the input note from the filesystem failed.
    Io(std::io::Error),
}

impl std::fmt::Display for ParseNoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "failed to read note: {err}"),
        }
    }
}

impl std::error::Error for ParseNoteError {}

impl From<std::io::Error> for ParseNoteError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

/// Normalized Phase 1 representation of a single parsed note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedNote {
    /// Full original note contents, including frontmatter when present.
    pub raw_text: String,
    /// Note contents with top-level frontmatter removed.
    pub body_text: String,
    /// Parsed frontmatter, if the note contains a top-level frontmatter block.
    pub frontmatter: Option<ParsedFrontmatter>,
    /// Extracted ATX headings with heading-path metadata.
    pub headings: Vec<ParsedHeading>,
    /// Extracted Obsidian-style links and embeds.
    pub links: Vec<ParsedLink>,
    /// Extracted inline and frontmatter-derived tags.
    pub tags: Vec<ParsedTag>,
    /// Non-fatal parse issues discovered while parsing the note.
    pub warnings: Vec<ParseWarning>,
    /// Basic note statistics derived during parsing.
    pub stats: NoteStats,
}

/// Parsed top-level frontmatter block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedFrontmatter {
    /// Raw frontmatter contents between the opening and closing delimiters.
    pub raw: String,
    /// Parsed frontmatter fields represented as a JSON-compatible key-value map.
    pub fields: Map<String, Value>,
    /// One-based start line for the frontmatter block.
    pub line_start: usize,
    /// One-based end line for the frontmatter block, including the closing delimiter.
    pub line_end: usize,
}

/// Parsed ATX heading with lookup metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedHeading {
    /// Heading level from `1` to `6`.
    pub level: u8,
    /// Visible heading text.
    pub text: String,
    /// Normalized heading text for later lookup and resolution.
    pub normalized_text: String,
    /// Visible heading chain from the root heading to this heading.
    pub heading_path: Vec<String>,
    /// One-based line where the heading starts.
    pub start_line: usize,
    /// One-based line where the heading section ends.
    pub end_line: usize,
}

/// Parsed Obsidian link or embed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedLink {
    /// Raw source text including brackets and optional embed marker.
    pub raw: String,
    /// Parsed note target before vault-wide resolution.
    pub target_note: String,
    /// Optional parsed heading target.
    pub target_heading: Option<String>,
    /// Optional parsed alias text.
    pub alias: Option<String>,
    /// Whether the link is an embed such as `![[Note]]`.
    pub is_embed: bool,
    /// One-based line where the link starts.
    pub line: usize,
    /// Byte offset where the raw link starts inside `body_text`.
    pub byte_start: usize,
    /// Byte offset immediately after the raw link inside `body_text`.
    pub byte_end: usize,
}

/// Parsed tag occurrence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedTag {
    /// Raw tag text as it appeared in the source.
    pub raw: String,
    /// Canonical normalized tag value.
    pub normalized: String,
    /// Origin of the extracted tag.
    pub source: TagSource,
    /// One-based line where the tag occurred, when line data exists.
    pub line: Option<usize>,
}

/// Origin of a parsed tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TagSource {
    /// Tag parsed from note body text.
    Inline,
    /// Tag derived from frontmatter metadata.
    Frontmatter,
}

/// Recoverable parse warning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseWarning {
    /// Warning classification.
    pub code: WarningCode,
    /// Human-readable warning message.
    pub message: String,
    /// One-based line associated with the warning when available.
    pub line: Option<usize>,
}

/// Supported warning categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WarningCode {
    /// Frontmatter existed but could not be parsed as valid YAML.
    InvalidFrontmatter,
    /// Heading data was malformed or could not be normalized.
    InvalidHeading,
    /// Obsidian link syntax was malformed.
    InvalidLink,
    /// The note used syntax intentionally deferred beyond Phase 1.
    UnsupportedSyntax,
}

/// Lightweight statistics derived during parsing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteStats {
    /// Total number of lines in the original note.
    pub line_count: usize,
    /// Total whitespace-delimited words in the note body.
    pub word_count: usize,
    /// Total number of Unicode scalar values in the original note.
    pub char_count: usize,
}
