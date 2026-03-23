use std::ops::Range;

pub(crate) fn normalize_heading_text_for_display(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn normalize_heading_lookup(text: &str) -> String {
    normalize_heading_text_for_display(text).to_lowercase()
}

pub(crate) fn normalize_tag(tag: &str) -> String {
    let trimmed = tag.trim().trim_start_matches('#');
    if trimmed.is_empty() {
        "#".to_string()
    } else {
        format!("#{}", trimmed.to_lowercase())
    }
}

pub(crate) fn is_atx_heading(body_text: &str, offset: usize) -> bool {
    let line_start = body_text[..offset].rfind('\n').map_or(0, |index| index + 1);
    let line_end = body_text[offset..]
        .find('\n')
        .map_or(body_text.len(), |index| offset + index);
    let line = &body_text[line_start..line_end];
    line.trim_start().starts_with('#')
}

pub(crate) fn is_heading_marker(body_text: &str, hash_index: usize) -> bool {
    let line_start = body_text[..hash_index]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    let before_hash = &body_text[line_start..hash_index];
    if !before_hash.trim().is_empty() {
        return false;
    }

    let after_hash = &body_text[hash_index + 1..];
    after_hash.chars().next().is_some_and(char::is_whitespace)
}

pub(crate) fn range_is_excluded(
    start: usize,
    end: usize,
    excluded_ranges: &[Range<usize>],
) -> bool {
    excluded_ranges
        .iter()
        .any(|range| start < range.end && end > range.start)
}

pub(crate) fn split_once_unescaped(input: &str, needle: char) -> (&str, Option<&str>) {
    if let Some(index) = input.find(needle) {
        let left = &input[..index];
        let right = &input[index + needle.len_utf8()..];
        (left, Some(right))
    } else {
        (input, None)
    }
}

pub(crate) fn count_lines(text: &str) -> usize {
    if text.is_empty() {
        0
    } else {
        let newline_count = text.chars().filter(|ch| *ch == '\n').count();
        if text.ends_with('\n') {
            newline_count
        } else {
            newline_count + 1
        }
    }
}

pub(crate) fn count_words(text: &str) -> usize {
    text.split_whitespace().count()
}

pub(crate) fn is_tag_body_char(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, '_' | '-' | '/')
}

pub(crate) struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    pub(crate) fn new(text: &str) -> Self {
        let mut starts = vec![0];
        for (index, byte) in text.bytes().enumerate() {
            if byte == b'\n' && index + 1 < text.len() {
                starts.push(index + 1);
            }
        }
        Self { starts }
    }

    pub(crate) fn line_for_offset(&self, offset: usize) -> usize {
        self.starts.partition_point(|start| *start <= offset)
    }
}
