use regex::Regex;

use super::comments::parse_source;
use crate::model::{Block, ParsedLine, protect_spaces_in_ranges};
use crate::{CustomMarkers, File, RewrapRequest};

const PRESERVE_ENVIRONMENTS: &[&str] = &[
    "align",
    "align*",
    "alltt",
    "alltt*",
    "displaymath",
    "displaymath*",
    "equation",
    "equation*",
    "gather",
    "gather*",
    "listing",
    "listing*",
    "lstlisting",
    "lstlisting*",
    "math",
    "math*",
    "multline",
    "multline*",
    "verbatim",
    "verbatim*",
];

#[derive(Debug)]
struct Command<'a> {
    name: &'a str,
    argument: &'a str,
    whole_line: bool,
}

fn command(value: &str) -> Option<Command<'_>> {
    let trimmed = value.trim();
    let captures = Regex::new(r"^\\(\[|[a-z]+)\*?\s*(?:(?:\[.*?\]|\{(.*?)\})\s*)*")
        .expect("valid LaTeX command regex")
        .captures(trimmed)?;
    let full = captures.get(0)?;
    Some(Command {
        name: captures.get(1)?.as_str(),
        argument: captures.get(2).map_or("", |group| group.as_str()),
        whole_line: full.end() == trimmed.len(),
    })
}

fn preserve_marker(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if matches!(trimmed, r"\(" | r"\[" | "$" | "$$") {
        Some(trimmed.to_owned())
    } else {
        let command = command(value)?;
        (command.name == "begin" && PRESERVE_ENVIRONMENTS.contains(&command.argument))
            .then(|| command.argument.to_owned())
    }
}

fn preserve_end(marker: &str) -> String {
    match marker {
        "$" | "$$" => marker.to_owned(),
        r"\(" => r"\)".to_owned(),
        r"\[" => r"\]".to_owned(),
        environment => format!(r"\end{{{environment}}}"),
    }
}

fn contains_unescaped(value: &str, marker: &str) -> bool {
    let mut offset = 0;
    while let Some(found) = value[offset..].find(marker) {
        let index = offset + found;
        if index == 0 || value.as_bytes()[index - 1] != b'\\' {
            return true;
        }
        offset = index + marker.len();
    }
    false
}

fn is_comment(value: &str) -> bool {
    Regex::new(r"^\s*%")
        .expect("valid LaTeX comment regex")
        .is_match(value)
}

fn ends_paragraph(value: &str) -> bool {
    value.ends_with("  ")
        || Regex::new(
            r"(\\(?:\\\*?|hline|newline|break|linebreak)(?:\[.*?\])?(?:\{.*?\})?\s*$)|(?:[^\\]%)",
        )
        .expect("valid LaTeX line-break regex")
        .is_match(value)
}

fn is_block_command(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.starts_with("$$")
        || command(value).is_some_and(|command| matches!(command.name, "[" | "begin" | "item"))
}

fn parse_comments(request: &RewrapRequest, lines: &[ParsedLine], start: usize) -> Vec<Block> {
    let comment_request = RewrapRequest {
        file: File {
            language: "latex-comment".to_owned(),
            path: String::new(),
            custom_markers: CustomMarkers {
                line: "%".to_owned(),
                block: (String::new(), String::new()),
            },
        },
        settings: request.settings,
        selections: request.selections.clone(),
        lines: lines.iter().map(ParsedLine::original).collect(),
    };
    let raw = comment_request
        .lines
        .iter()
        .cloned()
        .map(|line| ParsedLine::new("", line))
        .collect::<Vec<_>>();
    let mut blocks = parse_source(&comment_request, &raw);
    for block in &mut blocks {
        block.start += start;
    }
    blocks
}

pub(crate) fn parse_latex(request: &RewrapRequest, lines: &[ParsedLine]) -> Vec<Block> {
    let end_of_line_comment = Regex::new(r"[^\\]%").expect("valid LaTeX comment regex");
    let mut blocks = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if lines[index].content.trim().is_empty() {
            blocks.push(Block::no_wrap(index, vec![lines[index].clone()]));
            index += 1;
            continue;
        }
        if is_comment(&lines[index].content) {
            let mut end = index + 1;
            while end < lines.len() && is_comment(&lines[end].content) {
                end += 1;
            }
            blocks.extend(parse_comments(request, &lines[index..end], index));
            index = end;
            continue;
        }
        if let Some(marker) = preserve_marker(&lines[index].content) {
            let end_marker = preserve_end(&marker);
            let mut end = index + 1;
            while end < lines.len() {
                let finished = contains_unescaped(&lines[end].content, &end_marker);
                end += 1;
                if finished {
                    break;
                }
            }
            blocks.push(Block::no_wrap(index, lines[index..end].to_vec()));
            index = end;
            continue;
        }
        if command(&lines[index].content).is_some_and(|command| command.whole_line) {
            blocks.push(Block::no_wrap(index, vec![lines[index].clone()]));
            index += 1;
            continue;
        }

        let mut end = index + 1;
        while end < lines.len() {
            if lines[end].content.trim().is_empty()
                || is_comment(&lines[end].content)
                || preserve_marker(&lines[end].content).is_some()
                || command(&lines[end].content).is_some_and(|command| command.whole_line)
                || is_block_command(&lines[end].content)
            {
                break;
            }
            if ends_paragraph(&lines[end - 1].content) {
                break;
            }
            end += 1;
        }
        let mut paragraph = lines[index..end].to_vec();
        for line in paragraph.iter_mut().skip(1) {
            if let Some(found) = end_of_line_comment.find(&line.content) {
                let freeze_from = found.end();
                let protected = freeze_from..line.content.len();
                line.content =
                    protect_spaces_in_ranges(&line.content, std::slice::from_ref(&protected));
                line.protected_spaces = true;
            }
        }
        for line in &mut paragraph {
            let indent = line.content.len() - line.content.trim_start().len();
            line.prefix.push_str(&line.content[..indent]);
            line.content = line.content[indent..].to_owned();
        }
        blocks.push(Block::wrap(index, paragraph, None));
        index = end;
    }
    blocks
}
