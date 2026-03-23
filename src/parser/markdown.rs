use std::ops::Range;

use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd};

pub(crate) fn body_parser_options() -> Options {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_WIKILINKS);
    options
}

pub(crate) fn frontmatter_parser_options() -> Options {
    let mut options = body_parser_options();
    options.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
    options
}

// Return byte ranges where the custom Obsidian scanners must stay silent.
//
// Phase 1 excludes code-like spans plus full wikilink/embed spans. Custom text
// scanners should treat syntax inside those regions as literal text.
pub(crate) fn collect_excluded_ranges(body_text: &str) -> Vec<Range<usize>> {
    let parser = Parser::new_ext(body_text, body_parser_options()).into_offset_iter();
    let mut excluded = Vec::new();
    let mut code_block_start = None;
    let mut wikilink_start = None;

    for (event, range) in parser {
        match event {
            Event::Start(Tag::CodeBlock(_)) => code_block_start = Some(range.start),
            Event::Start(Tag::Link {
                link_type: LinkType::WikiLink { .. },
                ..
            })
            | Event::Start(Tag::Image {
                link_type: LinkType::WikiLink { .. },
                ..
            }) => {
                wikilink_start = Some(range.start);
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(start) = code_block_start.take() {
                    // Exclude the full block span, not only inner text nodes.
                    // That keeps later scans to a single overlap check per candidate.
                    excluded.push(start..range.end);
                }
            }
            Event::End(TagEnd::Link) | Event::End(TagEnd::Image) => {
                if let Some(start) = wikilink_start.take() {
                    excluded.push(start..range.end);
                }
            }
            Event::Code(_) => excluded.push(range),
            _ => {}
        }
    }

    excluded
}
