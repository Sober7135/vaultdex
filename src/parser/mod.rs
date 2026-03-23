//! Single-note parser for Obsidian-style Markdown.

mod frontmatter;
mod headings;
mod links;
mod markdown;
mod tags;
mod types;
mod util;

use std::fs;
use std::path::Path;

pub use self::types::{
    NoteStats, ParseNoteError, ParseWarning, ParsedFrontmatter, ParsedHeading, ParsedLink,
    ParsedNote, ParsedTag, TagSource, WarningCode,
};

/// Read a note from disk and delegate to [`parse_note_str`].
///
/// This returns a fatal error only when the file cannot be read as UTF-8 text.
pub fn parse_note_file(path: impl AsRef<Path>) -> Result<ParsedNote, ParseNoteError> {
    let path = path.as_ref();
    let raw_text = fs::read_to_string(path)?;
    Ok(parse_note_str(&raw_text))
}

/// Parse one Obsidian-style note string into the normalized Phase 1 note model.
///
/// This slices top-level frontmatter before Markdown parsing, then parses headings,
/// scans the body for Obsidian links and inline tags while skipping code-like spans,
/// and derives note statistics. Recoverable parse issues are returned as warnings
/// instead of failing the whole note.
pub fn parse_note_str(raw_text: &str) -> ParsedNote {
    let mut warnings = Vec::new();
    // Frontmatter is handled before Markdown parsing so heading and link positions are
    // computed against the actual body text while still preserving the original line offset.
    let sliced = frontmatter::split_frontmatter(raw_text);
    let frontmatter = sliced.frontmatter.and_then(|raw_frontmatter| {
        frontmatter::parse_frontmatter(
            &raw_frontmatter.content,
            raw_frontmatter.line_end,
            &mut warnings,
        )
    });

    let body_text = sliced.body.to_string();
    let body_line_offset = sliced.body_line_offset;
    let body_line_index = util::LineIndex::new(&body_text);
    let excluded_ranges = markdown::collect_excluded_ranges(&body_text);
    let mut headings = headings::extract_headings(&body_text, &body_line_index, body_line_offset);
    headings::finalize_heading_ranges(&mut headings, raw_text);

    let mut tags = tags::extract_inline_tags(
        &body_text,
        &body_line_index,
        body_line_offset,
        &excluded_ranges,
    );
    tags.extend(tags::extract_frontmatter_tags(frontmatter.as_ref()));

    let links = links::extract_links(
        &body_text,
        &body_line_index,
        body_line_offset,
        &mut warnings,
    );

    ParsedNote {
        raw_text: raw_text.to_string(),
        body_text,
        frontmatter,
        headings,
        links,
        tags,
        warnings,
        stats: NoteStats {
            line_count: util::count_lines(raw_text),
            word_count: util::count_words(sliced.body),
            char_count: raw_text.chars().count(),
        },
    }
}

#[cfg(test)]
mod tests {
    use insta::assert_json_snapshot;

    use super::*;

    #[test]
    fn snapshot_parses_frontmatter_headings_links_and_tags() {
        let input = r#"---
title: My Custom Title
tags:
  - rust
  - Obsidian
---
# Distributed Systems

See [[CAP Theorem]] and [[Raft#Leader Election|leader election]].
![[Architecture]]

Use #Distributed-Systems in notes.
"#;

        let parsed = parse_note_str(input);

        assert_json_snapshot!(
            parsed,
            @r###"
        {
          "raw_text": "---\ntitle: My Custom Title\ntags:\n  - rust\n  - Obsidian\n---\n# Distributed Systems\n\nSee [[CAP Theorem]] and [[Raft#Leader Election|leader election]].\n![[Architecture]]\n\nUse #Distributed-Systems in notes.\n",
          "body_text": "# Distributed Systems\n\nSee [[CAP Theorem]] and [[Raft#Leader Election|leader election]].\n![[Architecture]]\n\nUse #Distributed-Systems in notes.\n",
          "frontmatter": {
            "raw": "title: My Custom Title\ntags:\n  - rust\n  - Obsidian\n",
            "fields": {
              "tags": [
                "rust",
                "Obsidian"
              ],
              "title": "My Custom Title"
            },
            "line_start": 1,
            "line_end": 6
          },
          "headings": [
            {
              "level": 1,
              "text": "Distributed Systems",
              "normalized_text": "distributed systems",
              "heading_path": [
                "Distributed Systems"
              ],
              "start_line": 7,
              "end_line": 12
            }
          ],
          "links": [
            {
              "raw": "[[CAP Theorem]]",
              "target_note": "CAP Theorem",
              "target_heading": null,
              "alias": null,
              "is_embed": false,
              "line": 9,
              "byte_start": 27,
              "byte_end": 42
            },
            {
              "raw": "[[Raft#Leader Election|leader election]]",
              "target_note": "Raft",
              "target_heading": "Leader Election",
              "alias": "leader election",
              "is_embed": false,
              "line": 9,
              "byte_start": 47,
              "byte_end": 87
            },
            {
              "raw": "![[Architecture]]",
              "target_note": "Architecture",
              "target_heading": null,
              "alias": null,
              "is_embed": true,
              "line": 10,
              "byte_start": 89,
              "byte_end": 106
            }
          ],
          "tags": [
            {
              "raw": "#Distributed-Systems",
              "normalized": "#distributed-systems",
              "source": "Inline",
              "line": 12
            },
            {
              "raw": "rust",
              "normalized": "#rust",
              "source": "Frontmatter",
              "line": null
            },
            {
              "raw": "Obsidian",
              "normalized": "#obsidian",
              "source": "Frontmatter",
              "line": null
            }
          ],
          "warnings": [],
          "stats": {
            "line_count": 12,
            "word_count": 15,
            "char_count": 202
          }
        }
        "###
        );
    }

    #[test]
    fn snapshot_ignores_fake_links_and_tags_inside_code() {
        let input = r#"# Note

Inline `[[FakeLink]] #fake-tag`

```rust
let x = "[[StillFake]] #still-fake";
```

Real [[Target]] and #real-tag
"#;

        let parsed = parse_note_str(input);

        assert_json_snapshot!(
            parsed,
            @r###"
        {
          "raw_text": "# Note\n\nInline `[[FakeLink]] #fake-tag`\n\n```rust\nlet x = \"[[StillFake]] #still-fake\";\n```\n\nReal [[Target]] and #real-tag\n",
          "body_text": "# Note\n\nInline `[[FakeLink]] #fake-tag`\n\n```rust\nlet x = \"[[StillFake]] #still-fake\";\n```\n\nReal [[Target]] and #real-tag\n",
          "frontmatter": null,
          "headings": [
            {
              "level": 1,
              "text": "Note",
              "normalized_text": "note",
              "heading_path": [
                "Note"
              ],
              "start_line": 1,
              "end_line": 9
            }
          ],
          "links": [
            {
              "raw": "[[Target]]",
              "target_note": "Target",
              "target_heading": null,
              "alias": null,
              "is_embed": false,
              "line": 9,
              "byte_start": 96,
              "byte_end": 106
            }
          ],
          "tags": [
            {
              "raw": "#real-tag",
              "normalized": "#real-tag",
              "source": "Inline",
              "line": 9
            }
          ],
          "warnings": [],
          "stats": {
            "line_count": 9,
            "word_count": 16,
            "char_count": 121
          }
        }
        "###
        );
    }

    #[test]
    fn snapshot_computes_nested_heading_paths_and_ranges() {
        let input = r#"# Root
Intro

## Child
Body

### Grandchild
More

## Sibling
End
"#;

        let parsed = parse_note_str(input);

        assert_json_snapshot!(
            parsed.headings,
            @r###"
        [
          {
            "level": 1,
            "text": "Root",
            "normalized_text": "root",
            "heading_path": [
              "Root"
            ],
            "start_line": 1,
            "end_line": 11
          },
          {
            "level": 2,
            "text": "Child",
            "normalized_text": "child",
            "heading_path": [
              "Root",
              "Child"
            ],
            "start_line": 4,
            "end_line": 9
          },
          {
            "level": 3,
            "text": "Grandchild",
            "normalized_text": "grandchild",
            "heading_path": [
              "Root",
              "Child",
              "Grandchild"
            ],
            "start_line": 7,
            "end_line": 9
          },
          {
            "level": 2,
            "text": "Sibling",
            "normalized_text": "sibling",
            "heading_path": [
              "Root",
              "Sibling"
            ],
            "start_line": 10,
            "end_line": 11
          }
        ]
        "###
        );
    }

    #[test]
    fn keeps_invalid_frontmatter_as_warning() {
        let input = r#"---
title: [oops
---
# Note
"#;

        let parsed = parse_note_str(input);

        assert_json_snapshot!(
            serde_json::json!({
                "frontmatter": parsed.frontmatter,
                "warnings": parsed.warnings,
            }),
            @r###"
        {
          "frontmatter": {
            "fields": {},
            "line_end": 3,
            "line_start": 1,
            "raw": "title: [oops\n"
          },
          "warnings": [
            {
              "code": "InvalidFrontmatter",
              "line": 1,
              "message": "invalid YAML frontmatter: did not find expected ',' or ']' at line 2 column 1, while parsing a flow sequence at line 1 column 8"
            }
          ]
        }
        "###
        );
    }

    #[test]
    fn warns_when_frontmatter_top_level_is_not_a_mapping() {
        let input = r#"---
- one
- two
---
# Note
"#;

        let parsed = parse_note_str(input);

        assert_json_snapshot!(
            serde_json::json!({
                "frontmatter": parsed.frontmatter,
                "warnings": parsed.warnings,
            }),
            @r###"
        {
          "frontmatter": {
            "fields": {},
            "line_end": 4,
            "line_start": 1,
            "raw": "- one\n- two\n"
          },
          "warnings": [
            {
              "code": "InvalidFrontmatter",
              "line": 1,
              "message": "frontmatter top level must be a mapping/object"
            }
          ]
        }
        "###
        );
    }

    #[test]
    fn warns_on_unsupported_block_reference_links() {
        let parsed = parse_note_str("[[Note#^block-id]]\n");

        assert_json_snapshot!(
            serde_json::json!({
                "links": parsed.links,
                "warnings": parsed.warnings,
            }),
            @r###"
        {
          "links": [
            {
              "alias": null,
              "byte_end": 18,
              "byte_start": 0,
              "is_embed": false,
              "line": 1,
              "raw": "[[Note#^block-id]]",
              "target_heading": "^block-id",
              "target_note": "Note"
            }
          ],
          "warnings": [
            {
              "code": "UnsupportedSyntax",
              "line": 1,
              "message": "block reference links are not supported in Phase 1"
            }
          ]
        }
        "###
        );
    }

    #[test]
    fn parses_heading_only_wikilinks_in_current_note() {
        let parsed = parse_note_str("See [[#Section]] and ![[#Embed Section]]\n");

        assert_json_snapshot!(
            serde_json::json!({
                "links": parsed.links,
                "warnings": parsed.warnings,
            }),
            @r###"
        {
          "links": [
            {
              "alias": null,
              "byte_end": 16,
              "byte_start": 4,
              "is_embed": false,
              "line": 1,
              "raw": "[[#Section]]",
              "target_heading": "Section",
              "target_note": ""
            },
            {
              "alias": null,
              "byte_end": 40,
              "byte_start": 21,
              "is_embed": true,
              "line": 1,
              "raw": "![[#Embed Section]]",
              "target_heading": "Embed Section",
              "target_note": ""
            }
          ],
          "warnings": []
        }
        "###
        );
    }

    #[test]
    fn excludes_wikilink_text_from_inline_tag_extraction() {
        let parsed = parse_note_str("[[Note#Heading|#alias]] #real-tag\n");

        assert_json_snapshot!(
            serde_json::json!({
                "links": parsed.links,
                "tags": parsed.tags,
            }),
            @r###"
        {
          "links": [
            {
              "alias": "#alias",
              "byte_end": 23,
              "byte_start": 0,
              "is_embed": false,
              "line": 1,
              "raw": "[[Note#Heading|#alias]]",
              "target_heading": "Heading",
              "target_note": "Note"
            }
          ],
          "tags": [
            {
              "line": 1,
              "normalized": "#real-tag",
              "raw": "#real-tag",
              "source": "Inline"
            }
          ]
        }
        "###
        );
    }

    #[test]
    fn preserves_exact_wikilink_raw_text_and_span() {
        let input = "prefix [[Note| alias ]] suffix\n";
        let parsed = parse_note_str(input);

        assert_json_snapshot!(
            parsed.links,
            @r###"
        [
          {
            "raw": "[[Note| alias ]]",
            "target_note": "Note",
            "target_heading": null,
            "alias": "alias",
            "is_embed": false,
            "line": 1,
            "byte_start": 7,
            "byte_end": 23
          }
        ]
        "###
        );
    }

    #[test]
    fn wikilink_spans_stop_before_following_text() {
        let input = "[[A]]x\n[[B]] \n![[C]]d\n";
        let parsed = parse_note_str(input);

        assert_json_snapshot!(
            parsed.links,
            @r###"
        [
          {
            "raw": "[[A]]",
            "target_note": "A",
            "target_heading": null,
            "alias": null,
            "is_embed": false,
            "line": 1,
            "byte_start": 0,
            "byte_end": 5
          },
          {
            "raw": "[[B]]",
            "target_note": "B",
            "target_heading": null,
            "alias": null,
            "is_embed": false,
            "line": 2,
            "byte_start": 7,
            "byte_end": 12
          },
          {
            "raw": "![[C]]",
            "target_note": "C",
            "target_heading": null,
            "alias": null,
            "is_embed": true,
            "line": 3,
            "byte_start": 14,
            "byte_end": 20
          }
        ]
        "###
        );
    }

    #[test]
    fn body_leading_yaml_block_is_not_treated_as_second_frontmatter() {
        let input = "---\n\
title: t\n\
---\n\
---\n\
# Heading\n\
---\n";

        let parsed = parse_note_str(input);

        assert_json_snapshot!(
            serde_json::json!({
                "frontmatter": parsed.frontmatter,
                "headings": parsed.headings,
                "links": parsed.links,
                "warnings": parsed.warnings,
            }),
            @r###"
        {
          "frontmatter": {
            "fields": {
              "title": "t"
            },
            "line_end": 3,
            "line_start": 1,
            "raw": "title: t\n"
          },
          "headings": [
            {
              "end_line": 6,
              "heading_path": [
                "Heading"
              ],
              "level": 1,
              "normalized_text": "heading",
              "start_line": 5,
              "text": "Heading"
            }
          ],
          "links": [],
          "warnings": []
        }
        "###
        );
    }
}
