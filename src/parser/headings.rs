use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

use crate::parser::markdown::body_parser_options;
use crate::parser::types::ParsedHeading;
use crate::parser::util::{
    LineIndex, count_lines, is_atx_heading, normalize_heading_lookup,
    normalize_heading_text_for_display,
};

pub(crate) fn extract_headings(
    body_text: &str,
    line_index: &LineIndex,
    body_line_offset: usize,
) -> Vec<ParsedHeading> {
    let parser = Parser::new_ext(body_text, body_parser_options()).into_offset_iter();
    let mut headings = Vec::new();
    let mut current_heading: Option<HeadingAccumulator> = None;
    let mut stack: Vec<String> = Vec::new();

    for (event, range) in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) if is_atx_heading(body_text, range.start) => {
                let start_line = line_index.line_for_offset(range.start) + body_line_offset;
                current_heading = Some(HeadingAccumulator {
                    level: heading_level_number(level),
                    text: String::new(),
                    start_line,
                });
            }
            Event::Text(text) | Event::Code(text) if current_heading.is_some() => {
                current_heading
                    .as_mut()
                    .expect("heading state exists")
                    .text
                    .push_str(&text);
            }
            Event::End(TagEnd::Heading(level)) => {
                let Some(current) = current_heading.take() else {
                    continue;
                };

                if current.level != heading_level_number(level) {
                    continue;
                }

                let text = normalize_heading_text_for_display(&current.text);
                if text.is_empty() {
                    continue;
                }

                while stack.len() >= usize::from(current.level) {
                    stack.pop();
                }
                stack.push(text.clone());

                headings.push(ParsedHeading {
                    level: current.level,
                    normalized_text: normalize_heading_lookup(&text),
                    heading_path: stack.clone(),
                    text,
                    start_line: current.start_line,
                    end_line: current.start_line,
                });
            }
            _ => {}
        }
    }

    headings
}

pub(crate) fn finalize_heading_ranges(headings: &mut [ParsedHeading], raw_text: &str) {
    let total_lines = count_lines(raw_text);

    for index in 0..headings.len() {
        let current_level = headings[index].level;
        let mut end_line = total_lines;

        for next in headings.iter().skip(index + 1) {
            if next.level <= current_level {
                end_line = next.start_line.saturating_sub(1);
                break;
            }
        }

        headings[index].end_line = end_line.max(headings[index].start_line);
    }
}

fn heading_level_number(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

struct HeadingAccumulator {
    level: u8,
    text: String,
    start_line: usize,
}
