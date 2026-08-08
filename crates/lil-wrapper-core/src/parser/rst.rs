use regex::Regex;

use crate::Settings;
use crate::model::{Block, BlockKind, ParsedLine};
use crate::width::str_width;

fn regex(pattern: &str) -> Regex {
    Regex::new(&format!("(?i){pattern}")).expect("valid static RST regex")
}

fn indent_bytes(line: &ParsedLine) -> usize {
    line.content.len() - line.content.trim_start().len()
}

fn indent_width(line: &ParsedLine, settings: Settings) -> usize {
    let prefix = line
        .prefix
        .find("\"\"\"")
        .or_else(|| line.prefix.find("'''"))
        .map_or(line.prefix.as_str(), |delimiter| {
            let before = &line.prefix[..delimiter];
            before
                .find(|character: char| !character.is_whitespace())
                .map_or(before, |content| &before[..content])
        });
    str_width(settings.tab_width, &line.content[..indent_bytes(line)])
        + str_width(settings.tab_width, prefix)
}

fn punctuation_line(line: &ParsedLine) -> Option<(usize, char, usize)> {
    let value = line.content.trim_start();
    let indent = line.content.len() - value.len();
    let value = value.trim_end();
    let mut characters = value.chars();
    let first = characters.next()?;
    if !first.is_ascii_punctuation() || !characters.all(|character| character == first) {
        return None;
    }
    Some((indent, first, value.chars().count()))
}

fn grid_table_end(lines: &[ParsedLine], index: usize) -> Option<usize> {
    let indent = indent_bytes(&lines[index]);
    let value = lines[index].content.trim_start();
    if !regex(r"^\+-{3}[-+]*\+\s*$").is_match(value)
        || lines.get(index + 1).is_none_or(|line| {
            indent_bytes(line) != indent || !line.content.trim_start().starts_with('|')
        })
    {
        return None;
    }
    let mut end = index + 1;
    while end < lines.len()
        && indent_bytes(&lines[end]) == indent
        && matches!(
            lines[end].content.trim_start().chars().next(),
            Some('|' | '+')
        )
    {
        end += 1;
    }
    Some(end)
}

fn simple_table_end(lines: &[ParsedLine], index: usize) -> Option<usize> {
    let separator = regex(r"^=+(?:\s+=+)+\s*$");
    if !separator.is_match(lines[index].content.trim_start()) {
        return None;
    }
    let indent = indent_bytes(&lines[index]);
    let mut end = index + 1;
    let mut after_header = false;
    while end < lines.len() {
        let value = lines[end].content.trim_start();
        if !value.is_empty() && indent_bytes(&lines[end]) < indent {
            break;
        }
        if separator.is_match(value) {
            if after_header {
                end += 1;
                break;
            }
            after_header = true;
        }
        end += 1;
    }
    Some(end)
}

fn bullet_marker(line: &ParsedLine) -> Option<usize> {
    let found = regex(r"^\s*[-+*•‣⁃]").find(&line.content)?;
    let rest = &line.content[found.end()..];
    (rest.is_empty() || rest.starts_with(char::is_whitespace)).then_some(found.end())
}

fn numbered_marker(line: &ParsedLine) -> Option<usize> {
    let captures = regex(r"^\s*\(?(#|[0-9]+|[a-z]+)[.)]").captures(&line.content)?;
    let found = captures.get(0)?;
    if found.as_str().starts_with('(') && found.as_str().ends_with('.') {
        return None;
    }
    let value = captures.get(1)?.as_str();
    if value.len() > 1 && value.as_bytes()[0].is_ascii_alphabetic() && !valid_roman(value) {
        return None;
    }
    let rest = &line.content[found.end()..];
    (rest.is_empty() || rest.starts_with(char::is_whitespace)).then_some(found.end())
}

fn valid_roman(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    value.chars().all(|character| "mdclxvi".contains(character))
        && regex(r"^m{0,4}(?:cm|cd|d?c{0,3})(?:xc|xl|l?x{0,3})(?:ix|iv|v?i{0,3})$").is_match(&value)
}

fn field_marker(line: &ParsedLine) -> Option<(usize, String)> {
    let captures = regex(r"^(\s*):(.*?[^\\]):").captures(&line.content)?;
    let full = captures.get(0)?;
    let rest = &line.content[full.end()..];
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }
    Some((full.end(), captures.get(2)?.as_str().to_owned()))
}

fn footnote_marker(line: &ParsedLine) -> Option<usize> {
    regex(r"^\s*\.\. \[(?:\*|#(?:[-_.a-z0-9]+)?|[-_.a-z0-9]+)\]")
        .find(&line.content)
        .map(|found| found.end())
}

fn consume(mut line: ParsedLine, bytes: usize) -> ParsedLine {
    let bytes = bytes.min(line.content.len());
    line.prefix.push_str(&line.content[..bytes]);
    line.content = line.content[bytes..].to_owned();
    line
}

fn set_default_tail(blocks: &mut [Block], original_prefix: &str, removed: usize, spaces: usize) {
    let Some(first) = blocks.first_mut() else {
        return;
    };
    let BlockKind::Wrap { default_tail } = &mut first.kind else {
        return;
    };
    if first.lines.len() != 1 {
        return;
    }
    if let Some(tail) = default_tail {
        let replace_start = original_prefix.len().min(tail.len());
        let replace_end = (replace_start + removed).min(tail.len());
        tail.replace_range(replace_start..replace_end, &" ".repeat(spaces));
        return;
    }
    let prefix = &first.lines[0].prefix;
    let suffix_start = (original_prefix.len() + removed).min(prefix.len());
    *default_tail = Some(format!(
        "{original_prefix}{}{}",
        " ".repeat(spaces),
        &prefix[suffix_start..]
    ));
}

fn parse_indented_item(
    lines: &[ParsedLine],
    start: usize,
    marker_end: usize,
    default_spaces: usize,
) -> Vec<Block> {
    let mut parsed = lines.to_vec();
    parsed[0] = consume(parsed[0].clone(), marker_end);
    for line in &mut parsed {
        let indent = indent_bytes(line);
        line.prefix.push_str(&line.content[..indent]);
        line.content = line.content[indent..].to_owned();
    }
    let first_is_list =
        bullet_marker(&parsed[0]).is_some() || numbered_marker(&parsed[0]).is_some();
    let tail_is_list = parsed
        .get(1)
        .is_some_and(|line| bullet_marker(line).is_some() || numbered_marker(line).is_some());
    if first_is_list && tail_is_list {
        return parsed
            .into_iter()
            .enumerate()
            .map(|(offset, line)| Block::no_wrap(start + offset, vec![line]))
            .collect();
    }
    let default_tail =
        (parsed.len() == 1).then(|| format!("{}{}", lines[0].prefix, " ".repeat(default_spaces)));
    vec![Block::wrap(start, parsed, default_tail)]
}

fn parse_container(
    lines: &[ParsedLine],
    start: usize,
    settings: Settings,
    marker_end: usize,
    continuation_spaces: usize,
) -> (Vec<Block>, usize) {
    let base_indent = indent_width(&lines[0], settings);
    let mut inner = vec![consume(lines[0].clone(), marker_end)];
    let mut end = 1;
    while end < lines.len() {
        if lines[end].content.trim().is_empty() {
            inner.push(lines[end].clone());
            end += 1;
            continue;
        }
        let indent = indent_width(&lines[end], settings);
        if indent <= base_indent
            || bullet_marker(&lines[end]).is_some_and(|_| indent <= base_indent + marker_end)
            || numbered_marker(&lines[end]).is_some_and(|_| indent <= base_indent + marker_end)
        {
            break;
        }
        inner.push(lines[end].clone());
        end += 1;
    }
    let mut blocks = parse_rst(&inner, start, settings);
    set_default_tail(
        &mut blocks,
        &lines[0].prefix,
        marker_end,
        continuation_spaces,
    );
    (blocks, end)
}

fn explicit_end(lines: &[ParsedLine], index: usize, settings: Settings) -> usize {
    let indent = indent_width(&lines[index], settings);
    let mut end = index + 1;
    while end < lines.len()
        && !lines[end].content.trim().is_empty()
        && indent_width(&lines[end], settings) > indent
    {
        end += 1;
    }
    end
}

fn line_block(lines: &[ParsedLine], index: usize) -> Option<(Vec<ParsedLine>, usize)> {
    let marker = regex(r"^(\s*)\|\s+").find(&lines[index].content)?;
    let mut parsed = vec![consume(lines[index].clone(), marker.end())];
    let mut end = index + 1;
    while end < lines.len() {
        if regex(r"^\s*\|\s+").is_match(&lines[end].content) || lines[end].content.trim().is_empty()
        {
            break;
        }
        parsed.push(consume(lines[end].clone(), indent_bytes(&lines[end])));
        end += 1;
    }
    Some((parsed, end))
}

fn doctest_end(lines: &[ParsedLine], index: usize, settings: Settings) -> Option<usize> {
    if !regex(r"^\s*>>>(?:\s|$)").is_match(&lines[index].content) {
        return None;
    }
    let indent = indent_width(&lines[index], settings);
    let mut end = index + 1;
    while end < lines.len()
        && !lines[end].content.trim().is_empty()
        && indent_width(&lines[end], settings) >= indent
    {
        end += 1;
    }
    Some(end)
}

fn title_end(lines: &[ParsedLine], index: usize, settings: Settings) -> Option<usize> {
    if bullet_marker(&lines[index]).is_some() || numbered_marker(&lines[index]).is_some() {
        return None;
    }
    if let Some((indent, character, length)) = punctuation_line(&lines[index]) {
        if indent == 0 {
            if let (Some(middle), Some((end_indent, end_character, end_length))) = (
                lines.get(index + 1),
                lines.get(index + 2).and_then(punctuation_line),
            ) {
                let middle_end = str_width(settings.tab_width, middle.content.trim_end());
                if !middle.content.trim().is_empty()
                    && end_indent == 0
                    && character == end_character
                    && length == end_length
                    && middle_end <= length
                {
                    return Some(index + 3);
                }
            }
        }
        if indent == 0 && length >= 4 {
            if let Some((next_indent, _, _)) = lines.get(index + 1).and_then(punctuation_line) {
                return Some((index + if next_indent == 0 { 2 } else { 3 }).min(lines.len()));
            }
            if lines
                .get(index + 1)
                .is_some_and(|line| !line.content.trim().is_empty())
            {
                return Some((index + 3).min(lines.len()));
            }
            return Some((index + 1).min(lines.len()));
        }
        if indent > 0 && length >= 4 {
            return Some(index + 1);
        }
    }
    let underline = lines.get(index + 1).and_then(punctuation_line)?;
    if underline.0 != indent_bytes(&lines[index]) {
        return None;
    }
    let text_width = str_width(settings.tab_width, lines[index].content.trim_end());
    if index > 0 && punctuation_line(&lines[index - 1]).is_some_and(|(indent, _, _)| indent == 0) {
        return Some(index + 2);
    }
    (underline.2 >= 4 || underline.2 >= text_width).then_some(index + 2)
}

fn paragraph_end(lines: &[ParsedLine], index: usize, settings: Settings) -> usize {
    let indent = indent_width(&lines[index], settings);
    let mut end = index + 1;
    while end < lines.len() && !lines[end].content.trim().is_empty() {
        if indent_width(&lines[end], settings) != indent {
            break;
        }
        if end > index + 1 && punctuation_line(&lines[end]).is_some() {
            end += 1;
            continue;
        }
        end += 1;
    }
    end
}

fn push_paragraph(blocks: &mut Vec<Block>, lines: &[ParsedLine], start: usize, settings: Settings) {
    let mut paragraph = lines.to_vec();
    for line in &mut paragraph {
        let indent = indent_bytes(line);
        line.prefix.push_str(&line.content[..indent]);
        line.content = line.content[indent..].to_owned();
    }
    let _ = settings;
    blocks.push(Block::wrap(start, paragraph, None));
}

fn parse_rst_paragraph(
    lines: &[ParsedLine],
    start: usize,
    settings: Settings,
) -> (Vec<Block>, usize) {
    let mut blocks = Vec::new();
    let end = paragraph_end(lines, 0, settings);
    push_paragraph(&mut blocks, &lines[..end], start, settings);
    if !lines[end - 1].content.trim_end().ends_with("::")
        || end >= lines.len()
        || !lines[end].content.trim().is_empty()
    {
        return (blocks, end);
    }

    let base_indent = indent_width(&lines[end - 1], settings);
    let mut index = end;
    while index < lines.len() && lines[index].content.trim().is_empty() {
        blocks.push(Block::no_wrap(start + index, vec![lines[index].clone()]));
        index += 1;
    }
    let Some(first_literal) = lines.get(index) else {
        return (blocks, index);
    };
    let literal_indent = indent_width(first_literal, settings);
    let quoted = first_literal
        .content
        .chars()
        .next()
        .filter(char::is_ascii_punctuation);
    if literal_indent <= base_indent && (literal_indent != base_indent || quoted.is_none()) {
        return (blocks, index);
    }

    let mut literal_end = index + 1;
    while literal_end < lines.len() {
        if lines[literal_end].content.trim().is_empty() {
            literal_end += 1;
            continue;
        }
        let remains_literal = if literal_indent > base_indent {
            indent_width(&lines[literal_end], settings) > base_indent
        } else {
            lines[literal_end]
                .content
                .starts_with(quoted.expect("quoted literal"))
        };
        if !remains_literal {
            break;
        }
        literal_end += 1;
    }
    blocks.push(Block::no_wrap(
        start + index,
        lines[index..literal_end].to_vec(),
    ));
    (blocks, literal_end)
}

pub(crate) fn parse_rst(lines: &[ParsedLine], start: usize, settings: Settings) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if lines[index].content.trim().is_empty() {
            blocks.push(Block::no_wrap(start + index, vec![lines[index].clone()]));
            index += 1;
            continue;
        }
        if let Some(end) = grid_table_end(lines, index).or_else(|| simple_table_end(lines, index)) {
            blocks.push(Block::no_wrap(start + index, lines[index..end].to_vec()));
            index = end;
            continue;
        }
        if let Some(end) = doctest_end(lines, index, settings) {
            blocks.push(Block::no_wrap(start + index, lines[index..end].to_vec()));
            index = end;
            continue;
        }
        if let Some(end) = title_end(lines, index, settings) {
            blocks.push(Block::no_wrap(start + index, lines[index..end].to_vec()));
            index = end;
            continue;
        }
        if let Some((parsed, end)) = line_block(lines, index) {
            let tail = parsed.first().map(|line| " ".repeat(line.prefix.len()));
            blocks.push(Block::wrap(start + index, parsed, tail));
            index = end;
            continue;
        }
        if let Some(marker) =
            bullet_marker(&lines[index]).or_else(|| numbered_marker(&lines[index]))
        {
            let continuation = lines[index].content[..marker].encode_utf16().count();
            let (mut parsed, consumed) = parse_container(
                &lines[index..],
                start + index,
                settings,
                marker,
                continuation,
            );
            blocks.append(&mut parsed);
            index += consumed;
            continue;
        }
        if let Some((marker, name)) = field_marker(&lines[index]) {
            let end = explicit_end(lines, index, settings);
            if name == "Address" {
                blocks.push(Block::no_wrap(start + index, lines[index..end].to_vec()));
            } else {
                blocks.extend(parse_indented_item(
                    &lines[index..end],
                    start + index,
                    marker,
                    4,
                ));
            }
            index = end;
            continue;
        }
        if let Some(marker) = footnote_marker(&lines[index]) {
            let end = explicit_end(lines, index, settings);
            blocks.extend(parse_indented_item(
                &lines[index..end],
                start + index,
                marker,
                4,
            ));
            index = end;
            continue;
        }
        if regex(r"^\s*(?:\.\.(?:\s|$)|__\s\S)").is_match(&lines[index].content) {
            let end = explicit_end(lines, index, settings);
            blocks.push(Block::no_wrap(start + index, lines[index..end].to_vec()));
            index = end;
            continue;
        }
        if regex(r"^\s*\+-{3}[-+]*\+\s*$").is_match(&lines[index].content) {
            blocks.push(Block::no_wrap(start + index, vec![lines[index].clone()]));
            index += 1;
            continue;
        }

        let (mut parsed, consumed) = parse_rst_paragraph(&lines[index..], start + index, settings);
        blocks.append(&mut parsed);
        index += consumed;
    }
    blocks
}
