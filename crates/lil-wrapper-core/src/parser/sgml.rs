use regex::Regex;

use super::comments::{parse_html_comment, parse_source};
use crate::model::{Block, ParsedLine, protect_spaces_in_ranges};
use crate::{CustomMarkers, File, Settings, WrapRequest};

const XMLDOC_BLOCK_TAGS: &[&str] = &[
    "code",
    "description",
    "example",
    "exception",
    "include",
    "inheritdoc",
    "list",
    "listheader",
    "item",
    "para",
    "param",
    "permission",
    "remarks",
    "seealso",
    "summary",
    "term",
    "typeparam",
    "typeparamref",
    "returns",
    "value",
];

fn tag_at_start(value: &str) -> Option<String> {
    Regex::new(r"(?i)^\s*</?([\w.-]+)")
        .expect("valid SGML tag regex")
        .captures(value)
        .and_then(|captures| captures.get(1))
        .map(|tag| tag.as_str().to_ascii_lowercase())
}

fn tag_at_end(value: &str) -> Option<String> {
    Regex::new(r"(?i)</?([\w.-]+)(?:\s[^>]*)?>\s*$")
        .expect("valid SGML tag regex")
        .captures(value)
        .and_then(|captures| captures.get(1))
        .map(|tag| tag.as_str().to_ascii_lowercase())
}

fn is_exact_tag(value: &str, allowed: &[&str]) -> bool {
    let Some(captures) = Regex::new(r"(?i)^\s*</?([\w.-]+)(?:\s[^>]*)?>\s*$")
        .expect("valid SGML tag regex")
        .captures(value)
    else {
        return false;
    };
    captures.get(1).is_some_and(|tag| {
        allowed.is_empty() || allowed.contains(&tag.as_str().to_ascii_lowercase().as_str())
    })
}

fn freeze_tag_spaces(value: &str) -> (String, bool) {
    let mut rest = value;
    let mut offset = 0;
    let mut ranges = Vec::new();
    while let Some(start) = rest.find('<') {
        let mut quote = None;
        let end = rest[start..].char_indices().find_map(|(index, character)| {
            match character {
                '\'' | '"' if quote == Some(character) => quote = None,
                '\'' | '"' if quote.is_none() => quote = Some(character),
                '>' if quote.is_none() => return Some(start + index + character.len_utf8()),
                _ => {}
            }
            None
        });
        let Some(end) = end else {
            break;
        };
        ranges.push(offset + start..offset + end);
        offset += end;
        rest = &rest[end..];
    }
    let protected = !ranges.is_empty();
    (protect_spaces_in_ranges(value, &ranges), protected)
}

fn begins_block(value: &str, allowed: &[&str]) -> bool {
    tag_at_start(value).is_some_and(|tag| allowed.is_empty() || allowed.contains(&tag.as_str()))
}

fn ends_block(value: &str, allowed: &[&str]) -> bool {
    value.trim_end().ends_with("-->")
        || value.ends_with("  ")
        || tag_at_end(value)
            .is_some_and(|tag| allowed.is_empty() || allowed.contains(&tag.as_str()))
}

fn parse_sgml(
    lines: &[ParsedLine],
    start: usize,
    allowed: &[&str],
    protect_start_tags: bool,
) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if lines[index].content.trim().is_empty() || is_exact_tag(&lines[index].content, allowed) {
            blocks.push(Block::no_wrap(start + index, vec![lines[index].clone()]));
            index += 1;
            continue;
        }
        let mut end = index + 1;
        if !ends_block(&lines[index].content, allowed) {
            while end < lines.len()
                && !lines[end].content.trim().is_empty()
                && !begins_block(&lines[end].content, allowed)
            {
                let finished = ends_block(&lines[end].content, allowed);
                end += 1;
                if finished {
                    break;
                }
            }
        }
        let mut paragraph = lines[index..end].to_vec();
        for line in &mut paragraph {
            let indent = line.content.len() - line.content.trim_start().len();
            line.prefix.push_str(&line.content[..indent]);
            if protect_start_tags {
                let (content, protected_spaces) = freeze_tag_spaces(&line.content[indent..]);
                line.content = content;
                line.protected_spaces |= protected_spaces;
            } else {
                line.content = line.content[indent..].to_owned();
            }
        }
        blocks.push(Block::wrap(start + index, paragraph, None));
        index = end;
    }
    blocks
}

pub(crate) fn parse_xmldoc(lines: &[ParsedLine], start: usize, settings: Settings) -> Vec<Block> {
    let _ = settings;
    parse_sgml(lines, start, XMLDOC_BLOCK_TAGS, true)
}

fn embedded_source(
    request: &WrapRequest,
    lines: &[ParsedLine],
    start: usize,
    language: &str,
) -> Vec<Block> {
    let embedded_request = WrapRequest {
        file: File {
            language: language.to_owned(),
            path: String::new(),
            custom_markers: CustomMarkers::default(),
        },
        settings: request.settings,
        selections: request.selections.clone(),
        lines: lines.iter().map(ParsedLine::original).collect(),
    };
    let raw = embedded_request
        .lines
        .iter()
        .cloned()
        .map(|line| ParsedLine::new("", line))
        .collect::<Vec<_>>();
    let mut blocks = parse_source(&embedded_request, &raw);
    for block in &mut blocks {
        block.start += start;
    }
    blocks
}

pub(crate) fn parse_html(request: &WrapRequest, lines: &[ParsedLine]) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut index = 0;
    let mut comment_id = 0;
    while index < lines.len() {
        let trimmed = lines[index].content.trim_start().to_ascii_lowercase();
        if trimmed.starts_with("<!--") {
            let (mut parsed, consumed) =
                parse_html_comment(&lines[index..], index, request.settings, comment_id);
            blocks.append(&mut parsed);
            index += consumed;
            comment_id += 1;
            continue;
        }
        let embedded = if trimmed.contains("<script") {
            Some(("</script", "javascript"))
        } else if trimmed.contains("<style") {
            Some(("</style", "css"))
        } else {
            None
        };
        if let Some((end_marker, language)) = embedded {
            blocks.push(Block::no_wrap(index, vec![lines[index].clone()]));
            let mut end = index + 1;
            while end < lines.len() && !lines[end].content.to_ascii_lowercase().contains(end_marker)
            {
                end += 1;
            }
            if end > index + 1 {
                blocks.extend(embedded_source(
                    request,
                    &lines[index + 1..end],
                    index + 1,
                    language,
                ));
            }
            if end < lines.len() {
                blocks.push(Block::no_wrap(end, vec![lines[end].clone()]));
                end += 1;
            }
            index = end;
            continue;
        }
        let mut end = index + 1;
        while end < lines.len() {
            let value = lines[end].content.trim_start().to_ascii_lowercase();
            if value.starts_with("<!--") || value.contains("<script") || value.contains("<style") {
                break;
            }
            end += 1;
        }
        blocks.extend(parse_sgml(&lines[index..end], index, &[], false));
        index = end;
    }
    blocks
}
