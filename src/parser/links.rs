use pulldown_cmark::{Event, LinkType, Parser, Tag, TagEnd};

use crate::parser::markdown::body_parser_options;
use crate::parser::types::{ParseWarning, ParsedLink, WarningCode};
use crate::parser::util::{LineIndex, split_once_unescaped};

// Extract Obsidian wikilinks and embeds using pulldown-cmark's native wikilink extension.
// This keeps the parser aligned with pulldown-cmark's syntax decisions while preserving
// vaultdex-specific post-processing for note targets, aliases, and warnings.
pub(crate) fn extract_links(
    body_text: &str,
    line_index: &LineIndex,
    body_line_offset: usize,
    warnings: &mut Vec<ParseWarning>,
) -> Vec<ParsedLink> {
    let parser = Parser::new_ext(body_text, body_parser_options()).into_offset_iter();
    let mut links = Vec::new();
    let mut current = None;

    for (event, range) in parser {
        match event {
            // This arm starts a normal Obsidian wikilink.
            // pulldown-cmark emits `Tag::Link` for `[[note]]`, `[[note#heading]]`, and
            // `[[note|alias]]`, so we begin buffering the shared link state here.
            Event::Start(Tag::Link {
                link_type: LinkType::WikiLink { has_pothole },
                dest_url,
                ..
            }) => {
                current = Some(CurrentWikiLink {
                    dest_url: dest_url.to_string(),
                    display_text: String::new(),
                    has_pothole,
                    is_embed: false,
                    start: range.start,
                    line: line_index.line_for_offset(range.start) + body_line_offset,
                });
            }
            // This arm starts an Obsidian embed.
            // pulldown-cmark emits `Tag::Image` for `![[note]]`, `![[note#heading]]`, and
            // `![[note|alias]]`, and we record the same target data with `is_embed = true`.
            Event::Start(Tag::Image {
                link_type: LinkType::WikiLink { has_pothole },
                dest_url,
                ..
            }) => {
                current = Some(CurrentWikiLink {
                    dest_url: dest_url.to_string(),
                    display_text: String::new(),
                    has_pothole,
                    is_embed: true,
                    start: range.start,
                    line: line_index.line_for_offset(range.start) + body_line_offset,
                });
            }
            // These events carry the visible label inside a wikilink.
            // For `[[note|alias]]` and `![[note|alias]]`, pulldown-cmark emits the alias text
            // between the start and end events, so we accumulate it for later reconstruction.
            Event::Text(text) | Event::Code(text) => {
                if let Some(current) = current.as_mut() {
                    current.display_text.push_str(&text);
                }
            }
            // These events close the current wikilink or embed.
            // Once the parser reaches the end tag, we have enough information to normalize the
            // target, reconstruct the raw link form, and emit one `ParsedLink`.
            Event::End(TagEnd::Link) | Event::End(TagEnd::Image) => {
                let Some(current) = current.take() else {
                    continue;
                };
                let start = current.start;
                // Slice the raw link text from the source instead of rebuilding it.
                // We normalize `alias`, `target_note`, and `target_heading`, but `raw` and the
                // byte span must preserve the exact original wikilink text, including spacing.
                let byte_end = range.end;

                finalize_wikilink(
                    current,
                    body_text[start..byte_end].to_string(),
                    byte_end,
                    warnings,
                    &mut links,
                );
            }
            // All non-wikilink markdown events are irrelevant here.
            // Headings, paragraphs, code fences, and normal markdown links are handled elsewhere.
            _ => {}
        }
    }

    links
}

fn finalize_wikilink(
    current: CurrentWikiLink,
    raw: String,
    byte_end: usize,
    warnings: &mut Vec<ParseWarning>,
    links: &mut Vec<ParsedLink>,
) {
    match parse_link_target(&current.dest_url) {
        Some((target_note, target_heading)) => {
            if target_heading
                .as_deref()
                .is_some_and(|heading| heading.trim_start().starts_with('^'))
            {
                warnings.push(ParseWarning {
                    code: WarningCode::UnsupportedSyntax,
                    message: "block reference links are not supported in Phase 1".to_string(),
                    line: Some(current.line),
                });
            }

            let alias = current
                .has_pothole
                .then_some(current.display_text.trim().to_string())
                .filter(|value| !value.is_empty());

            links.push(ParsedLink {
                raw,
                target_note,
                target_heading,
                alias,
                is_embed: current.is_embed,
                line: current.line,
                byte_start: current.start,
                byte_end,
            });
        }
        None => warnings.push(ParseWarning {
            code: WarningCode::InvalidLink,
            message: format!("invalid Obsidian link target: {}", current.dest_url),
            line: Some(current.line),
        }),
    }
}

fn parse_link_target(raw: &str) -> Option<(String, Option<String>)> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }

    let (note_part, heading_part) = split_once_unescaped(raw, '#');
    // `target_note`` could be empty, because Obisidian support wikilinks like [[#what]]
    let target_note = note_part.trim();

    Some((
        target_note.to_string(),
        heading_part
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
    ))
}

struct CurrentWikiLink {
    dest_url: String,
    display_text: String,
    has_pothole: bool,
    is_embed: bool,
    start: usize,
    line: usize,
}
