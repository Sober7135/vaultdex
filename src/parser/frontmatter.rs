use pulldown_cmark::{Event, Tag, TagEnd};
use serde_json::{Map, Value};

use crate::parser::types::{ParseWarning, ParsedFrontmatter, WarningCode};
use crate::parser::util::count_lines;

pub(crate) struct FrontmatterSlice<'a> {
    pub(crate) frontmatter: Option<RawFrontmatter>,
    pub(crate) body: &'a str,
    pub(crate) body_line_offset: usize,
}

pub(crate) struct RawFrontmatter {
    pub(crate) content: String,
    pub(crate) line_end: usize,
}

// Split a fully closed top-of-file `--- ... ---` block from the rest of the note.
// Only the first line may open frontmatter, and the block must have a matching
// closing `---` line. If the closing delimiter is missing, the whole file is
// treated as ordinary body text.
pub(crate) fn split_frontmatter(input: &str) -> FrontmatterSlice<'_> {
    let mut parser = pulldown_cmark::Parser::new_ext(
        input,
        crate::parser::markdown::frontmatter_parser_options(),
    )
    .into_offset_iter();
    let Some((first_event, _first_range)) = parser.next() else {
        return FrontmatterSlice {
            frontmatter: None,
            body: input,
            body_line_offset: 0,
        };
    };

    let Event::Start(Tag::MetadataBlock(_)) = first_event else {
        return FrontmatterSlice {
            frontmatter: None,
            body: input,
            body_line_offset: 0,
        };
    };

    let mut raw_content = String::new();

    for (event, range) in parser {
        match event {
            Event::Text(text) => raw_content.push_str(&text),
            Event::End(TagEnd::MetadataBlock(_)) => {
                let body_start = consume_one_line_break(input, range.end);
                let line_end = count_lines(&input[..body_start]);
                return FrontmatterSlice {
                    frontmatter: Some(RawFrontmatter {
                        content: raw_content,
                        line_end,
                    }),
                    body: &input[body_start..],
                    body_line_offset: line_end,
                };
            }
            _ => {}
        }
    }
    FrontmatterSlice {
        frontmatter: None,
        body: input,
        body_line_offset: 0,
    }
}

// Parse the raw frontmatter text as YAML and normalize it into a key-value map.
// Successful parses must produce a top-level mapping/object. Non-mapping YAML
// and invalid YAML are both downgraded to warnings, while the raw frontmatter
// text is preserved even when normalized fields are empty.
pub(crate) fn parse_frontmatter(
    raw: &str,
    line_end: usize,
    warnings: &mut Vec<ParseWarning>,
) -> Option<ParsedFrontmatter> {
    match serde_yaml_ng::from_str::<Value>(raw) {
        Ok(Value::Object(fields)) => Some(ParsedFrontmatter {
            raw: raw.to_string(),
            fields,
            line_start: 1,
            line_end,
        }),
        Ok(_) => {
            warnings.push(ParseWarning {
                code: WarningCode::InvalidFrontmatter,
                message: "frontmatter top level must be a mapping/object".to_string(),
                line: Some(1),
            });
            Some(ParsedFrontmatter {
                raw: raw.to_string(),
                fields: Map::new(),
                line_start: 1,
                line_end,
            })
        }
        Err(err) => {
            warnings.push(ParseWarning {
                code: WarningCode::InvalidFrontmatter,
                message: format!("invalid YAML frontmatter: {err}"),
                line: Some(1),
            });
            Some(ParsedFrontmatter {
                raw: raw.to_string(),
                fields: Map::new(),
                line_start: 1,
                line_end,
            })
        }
    }
}

fn consume_one_line_break(input: &str, start: usize) -> usize {
    if input[start..].starts_with("\r\n") {
        start + 2
    } else if input[start..].starts_with('\n') {
        start + 1
    } else {
        start
    }
}
