use std::ops::Range;

use serde_json::Value;

use crate::parser::types::{ParsedFrontmatter, ParsedTag, TagSource};
use crate::parser::util::{
    LineIndex, is_heading_marker, is_tag_body_char, normalize_tag, range_is_excluded,
};

pub(crate) fn extract_inline_tags(
    body_text: &str,
    line_index: &LineIndex,
    body_line_offset: usize,
    excluded_ranges: &[Range<usize>],
) -> Vec<ParsedTag> {
    let bytes = body_text.as_bytes();
    let mut tags = Vec::new();
    let mut index = 0;

    while index < bytes.len() {
        // Inline tags are a custom text scan, so we explicitly suppress matches in code-like spans.
        if bytes[index] != b'#' || range_is_excluded(index, index + 1, excluded_ranges) {
            index += 1;
            continue;
        }

        let previous = if index == 0 {
            None
        } else {
            body_text[..index].chars().next_back()
        };
        let next = body_text[index + 1..].chars().next();

        if next.is_none() || next.is_some_and(|ch| ch.is_whitespace()) {
            index += 1;
            continue;
        }

        if previous.is_some_and(is_tag_body_char) {
            index += 1;
            continue;
        }

        if is_heading_marker(body_text, index) {
            index += 1;
            continue;
        }

        let mut end = index + 1;
        while end < bytes.len() {
            let Some(ch) = body_text[end..].chars().next() else {
                break;
            };
            if !is_tag_body_char(ch) {
                break;
            }
            end += ch.len_utf8();
        }

        if end == index + 1 {
            index += 1;
            continue;
        }

        let raw = &body_text[index..end];
        tags.push(ParsedTag {
            raw: raw.to_string(),
            normalized: normalize_tag(raw),
            source: TagSource::Inline,
            line: Some(line_index.line_for_offset(index) + body_line_offset),
        });
        index = end;
    }

    tags
}

pub(crate) fn extract_frontmatter_tags(frontmatter: Option<&ParsedFrontmatter>) -> Vec<ParsedTag> {
    let Some(frontmatter) = frontmatter else {
        return Vec::new();
    };

    let mut tags = Vec::new();
    for key in ["tags", "tag"] {
        let Some(value) = frontmatter.fields.get(key) else {
            continue;
        };
        match value {
            Value::String(tag) => push_frontmatter_tag(&mut tags, tag),
            Value::Array(items) => {
                for item in items {
                    if let Some(tag) = item.as_str() {
                        push_frontmatter_tag(&mut tags, tag);
                    }
                }
            }
            _ => {}
        }
    }

    tags
}

fn push_frontmatter_tag(tags: &mut Vec<ParsedTag>, tag: &str) {
    let trimmed = tag.trim();
    if trimmed.is_empty() {
        return;
    }

    tags.push(ParsedTag {
        raw: trimmed.to_string(),
        normalized: normalize_tag(trimmed),
        source: TagSource::Frontmatter,
        line: None,
    });
}
