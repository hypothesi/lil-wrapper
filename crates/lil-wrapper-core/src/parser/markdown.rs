use regex::Regex;

use crate::model::{Block, BlockKind, ParsedLine};
use crate::{Settings, str_width};

fn regex(pattern: &str) -> Regex {
    Regex::new(&format!("(?i){pattern}")).expect("valid static markdown regex")
}

fn content(line: &ParsedLine) -> &str {
    &line.content
}

fn indent_width(line: &ParsedLine) -> usize {
    let value = content(line);
    let end = value
        .find(|character: char| !matches!(character, ' ' | '\t'))
        .unwrap_or(value.len());
    str_width(4, &value[..end])
}

fn md_match(pattern: &str, line: &ParsedLine) -> bool {
    regex(&format!(r"^ {{0,3}}{pattern}")).is_match(content(line))
}

fn is_thematic(line: &ParsedLine) -> bool {
    md_match(
        r"(?:\*\s*\*\s*(?:\*\s*)+|-\s*-\s*(?:-\s*)+|_\s*_\s*(?:_\s*)+)$",
        line,
    )
}

fn is_setext(line: &ParsedLine) -> bool {
    md_match(r"(?:=+|-+)\s*$", line)
}

fn fence_start(line: &ParsedLine) -> Option<(char, usize)> {
    let captures = regex(r"^ {0,3}(`{3,}|~{3,})(.*)$").captures(content(line))?;
    let marker = captures.get(1)?.as_str();
    if marker.starts_with('`')
        && captures
            .get(2)
            .is_some_and(|tail| tail.as_str().contains('`'))
    {
        return None;
    }
    Some((marker.chars().next()?, marker.len()))
}

fn is_fence_end(line: &ParsedLine, marker: char, length: usize) -> bool {
    let trimmed = content(line).trim_start_matches(' ');
    let indent = content(line).len() - trimmed.len();
    if indent > 3 {
        return false;
    }
    let count = trimmed
        .chars()
        .take_while(|character| *character == marker)
        .count();
    count >= length && trimmed[count..].trim().is_empty()
}

fn html_end(line: &ParsedLine) -> Option<&'static str> {
    let value = content(line).trim_start();
    if regex(r"^<(script|pre|style)( |>|$)").is_match(value) {
        Some("tag")
    } else if value.starts_with("<!--") {
        Some("-->")
    } else if value.starts_with("<?") {
        Some("?>")
    } else if regex(r"^<![A-Z]").is_match(value) {
        Some(">")
    } else if value.starts_with("<![CDATA[") {
        Some("]]>")
    } else if regex(r"^</?(address|article|aside|base|basefont|blockquote|body|caption|center|col|colgroup|dd|details|dialog|dir|div|dl|dt|fieldset|figcaption|figure|footer|form|frame|frameset|h[1-6]|head|header|hr|html|iframe|legend|li|link|main|menu|menuitem|meta|nav|noframes|ol|optgroup|option|p|param|section|source|summary|table|tbody|td|tfoot|th|thead|title|tr|track|ul)(\s|/?>|$)").is_match(value) {
        Some("blank")
    } else {
        None
    }
}

fn html_finished(line: &ParsedLine, end: &str) -> bool {
    match end {
        "tag" => regex(r"</(script|pre|style)>").is_match(content(line)),
        "blank" => content(line).trim().is_empty(),
        marker => content(line).contains(marker),
    }
}

fn table_end(lines: &[ParsedLine], index: usize) -> Option<usize> {
    let cells = regex(r"^ {0,3}(?:\|.*[^\\]\||\S.*?[^\\]\|\s*\S)");
    if !cells.is_match(content(&lines[index])) {
        return None;
    }
    let mut end = index;
    let mut separator = false;
    while end < lines.len() && cells.is_match(content(&lines[end])) {
        let trimmed = content(&lines[end]).trim();
        separator |= trimmed.contains('|')
            && trimmed.contains('-')
            && trimmed
                .chars()
                .all(|character| matches!(character, '|' | ':' | '-' | ' '));
        end += 1;
    }
    (separator && end - index >= 2).then_some(end)
}

fn list_match(line: &ParsedLine) -> Option<(usize, usize)> {
    let captures = regex(r"^ {0,3}([-+*]|[0-9]{1,9}[.)])( +)").captures(content(line))?;
    let full = captures.get(0)?;
    let spaces = captures.get(2)?.as_str().len();
    let child_indent = if spaces <= 4 {
        full.end()
    } else {
        full.end() - spaces + 1
    };
    Some((full.end(), child_indent))
}

fn footnote_match(line: &ParsedLine) -> Option<usize> {
    regex(r"^ {0,3}\[\^\S+?\]:( +)")
        .find(content(line))
        .map(|found| found.end())
}

fn link_reference_match(line: &ParsedLine) -> Option<usize> {
    regex(r"^ {0,3}\[\s*\S.*?\]:\s*")
        .find(content(line))
        .map(|found| found.end())
}

fn consume_prefix(mut line: ParsedLine, bytes: usize) -> ParsedLine {
    let bytes = bytes.min(line.content.len());
    line.prefix.push_str(&line.content[..bytes]);
    line.content = line.content[bytes..].to_owned();
    line
}

fn parse_container(
    lines: &[ParsedLine],
    start: usize,
    settings: Settings,
    consume: usize,
    tail_indent: usize,
    footnote: bool,
) -> (Vec<Block>, usize) {
    let normalize = if settings.reformat && !footnote {
        content(&lines[0]).len() - content(&lines[0]).trim_start().len()
    } else {
        0
    };
    let mut first = lines[0].clone();
    if normalize > 0 {
        first.content.drain(..normalize);
    }
    let consume = consume.saturating_sub(normalize);
    let tail_indent = tail_indent.saturating_sub(normalize);
    let mut inner = vec![consume_prefix(first, consume)];
    let starts_fence = fence_start(&inner[0]).is_some();
    let mut end = 1;
    let mut paragraph = !inner[0].content.trim().is_empty();
    while end < lines.len() {
        let value = content(&lines[end]);
        if list_match(&lines[end]).is_some() && value.len() - value.trim_start().len() < tail_indent
        {
            break;
        }
        if value.trim().is_empty() {
            inner.push(lines[end].clone());
            paragraph = false;
            end += 1;
            continue;
        }
        let indent = value.len() - value.trim_start().len();
        if indent >= tail_indent + normalize {
            let mut line = lines[end].clone();
            if normalize > 0 && line.content.starts_with(&" ".repeat(normalize)) {
                line.content.drain(..normalize);
            }
            inner.push(consume_prefix(line, tail_indent));
            paragraph = !value.trim().is_empty();
            end += 1;
        } else if paragraph
            && !starts_fence
            && !md_match(r"#{1,6} ", &lines[end])
            && fence_start(&lines[end]).is_none()
        {
            inner.push(lines[end].clone());
            end += 1;
        } else {
            break;
        }
        if footnote && indent >= 8 {
            paragraph = false;
        }
    }
    let mut blocks = parse_markdown(&inner, start, settings, false);
    if let Some(first) = blocks.first_mut() {
        if let BlockKind::Wrap { default_tail } = &mut first.kind {
            if first.lines.len() == 1 {
                if let Some(tail) = default_tail {
                    let replace_start = lines[0].prefix.len().min(tail.len());
                    let replace_end = (replace_start + consume).min(tail.len());
                    tail.replace_range(replace_start..replace_end, &" ".repeat(tail_indent));
                    return (blocks, end);
                }
                let prefix = &first.lines[0].prefix;
                let base = lines[0].prefix.len();
                let suffix_start = (base + consume).min(prefix.len());
                *default_tail = Some(format!(
                    "{}{}{}",
                    &prefix[..base],
                    " ".repeat(tail_indent),
                    &prefix[suffix_start..]
                ));
            }
        }
    }
    (blocks, end)
}

fn blockquote(lines: &[ParsedLine], start: usize, settings: Settings) -> (Vec<Block>, usize) {
    let marker = regex(r"^ {0,3}> ?");
    let mut inner = Vec::new();
    let mut end = 0;
    let mut in_paragraph = false;
    let mut fence_indent = None;
    while end < lines.len() {
        if let Some(found) = marker.find(content(&lines[end])) {
            let mut parsed = consume_prefix(lines[end].clone(), found.end());
            if settings.reformat {
                parsed.prefix = format!("{}> ", lines[end].prefix);
            }
            in_paragraph = !parsed.content.trim().is_empty();
            if end == 0 && fence_start(&parsed).is_some() {
                fence_indent =
                    Some(found.end() + parsed.content.len() - parsed.content.trim_start().len());
            }
            inner.push(parsed);
            end += 1;
        } else if fence_indent.is_some_and(|minimum| {
            content(&lines[end]).len() - content(&lines[end]).trim_start().len() >= minimum
        }) || (in_paragraph
            && fence_indent.is_none()
            && !content(&lines[end]).trim().is_empty())
        {
            inner.push(lines[end].clone());
            end += 1;
        } else {
            break;
        }
    }
    let mut blocks = parse_markdown(&inner, start, settings, false);
    for block in &mut blocks {
        if block.lines.len() == 1
            && let BlockKind::Wrap { default_tail } = &mut block.kind
            && default_tail.is_none()
        {
            *default_tail = Some(block.lines[0].prefix.clone());
        }
    }
    (blocks, end)
}

fn is_interrupting(line: &ParsedLine) -> bool {
    content(line).trim().is_empty()
        || md_match(r"#{1,6} ", line)
        || is_thematic(line)
        || list_match(line).is_some()
        || md_match(">", line)
        || fence_start(line).is_some()
        || html_end(line).is_some()
        || footnote_match(line).is_some()
}

fn front_matter_end(lines: &[ParsedLine], allowed: bool) -> usize {
    if !allowed
        || lines
            .first()
            .is_none_or(|line| !regex(r"^---\s*$").is_match(content(line)))
    {
        return 0;
    }
    let mut end = 1;
    while end < lines.len() {
        end += 1;
        if regex(r"^---").is_match(content(&lines[end - 1])) {
            break;
        }
    }
    end
}

fn trim_paragraph(lines: &[ParsedLine]) -> Vec<ParsedLine> {
    lines
        .iter()
        .cloned()
        .map(|mut line| {
            let indent = line.content.len() - line.content.trim_start().len();
            line.prefix.push_str(&line.content[..indent]);
            line.content.drain(..indent);
            line
        })
        .collect()
}

fn parse_paragraph(lines: &[ParsedLine], start: usize) -> (Vec<Block>, usize) {
    if link_reference_match(&lines[0]).is_some() {
        let mut end = 1;
        while end < lines.len()
            && !is_interrupting(&lines[end])
            && link_reference_match(&lines[end]).is_none()
        {
            end += 1;
        }
        let default_tail = Some(format!("{}    ", lines[0].prefix));
        return (
            vec![Block::wrap(
                start,
                trim_paragraph(&lines[..end]),
                default_tail,
            )],
            end,
        );
    }

    let forced_break = regex(r"(\\|\s{2}|<br/?>)$");
    let mut end = 1;
    while end < lines.len() && !is_interrupting(&lines[end]) {
        if is_setext(&lines[end]) || forced_break.is_match(content(&lines[end - 1])) {
            break;
        }
        end += 1;
    }
    let mut blocks = vec![Block::wrap(start, trim_paragraph(&lines[..end]), None)];
    if end < lines.len() && is_setext(&lines[end]) {
        blocks.push(Block::no_wrap(start + end, vec![lines[end].clone()]));
        end += 1;
    }
    (blocks, end)
}

pub(crate) fn parse_markdown(
    lines: &[ParsedLine],
    start: usize,
    settings: Settings,
    allow_front_matter: bool,
) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut index = front_matter_end(lines, allow_front_matter);
    if index > 0 {
        blocks.push(Block::no_wrap(start, lines[..index].to_vec()));
    }

    while index < lines.len() {
        if content(&lines[index]).trim().is_empty() {
            blocks.push(Block::no_wrap(start + index, vec![lines[index].clone()]));
            index += 1;
            continue;
        }
        if let Some((marker, length)) = fence_start(&lines[index]) {
            let mut end = index + 1;
            while end < lines.len() {
                let finished = is_fence_end(&lines[end], marker, length);
                end += 1;
                if finished {
                    break;
                }
            }
            blocks.push(Block::no_wrap(start + index, lines[index..end].to_vec()));
            index = end;
            continue;
        }
        if let Some(end_marker) = html_end(&lines[index]) {
            let mut end = index + 1;
            if !html_finished(&lines[index], end_marker) {
                while end < lines.len() {
                    let finished = html_finished(&lines[end], end_marker);
                    end += 1;
                    if finished {
                        break;
                    }
                }
            }
            blocks.push(Block::no_wrap(start + index, lines[index..end].to_vec()));
            index = end;
            continue;
        }
        if let Some(end) = table_end(lines, index) {
            blocks.push(Block::no_wrap(start + index, lines[index..end].to_vec()));
            index = end;
            continue;
        }
        if md_match(r"#{1,6} ", &lines[index]) || is_thematic(&lines[index]) {
            blocks.push(Block::no_wrap(start + index, vec![lines[index].clone()]));
            index += 1;
            continue;
        }
        if indent_width(&lines[index]) >= 4 {
            let mut end = index + 1;
            while end < lines.len()
                && (content(&lines[end]).trim().is_empty() || indent_width(&lines[end]) >= 4)
            {
                end += 1;
            }
            blocks.push(Block::no_wrap(start + index, lines[index..end].to_vec()));
            index = end;
            continue;
        }
        if md_match(">", &lines[index]) {
            let (mut parsed, consumed) = blockquote(&lines[index..], start + index, settings);
            blocks.append(&mut parsed);
            index += consumed;
            continue;
        }
        if let Some((_consume, child_indent)) = list_match(&lines[index]) {
            let (mut parsed, consumed) = parse_container(
                &lines[index..],
                start + index,
                settings,
                child_indent,
                child_indent,
                false,
            );
            blocks.append(&mut parsed);
            index += consumed;
            continue;
        }
        if let Some(consume) = footnote_match(&lines[index]) {
            let (mut parsed, consumed) =
                parse_container(&lines[index..], start + index, settings, consume, 4, true);
            blocks.append(&mut parsed);
            index += consumed;
            continue;
        }
        let (mut parsed, consumed) = parse_paragraph(&lines[index..], start + index);
        blocks.append(&mut parsed);
        index += consumed;
    }
    blocks
}
