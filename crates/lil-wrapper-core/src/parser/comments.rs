use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use regex::Regex;

use super::markdown::parse_markdown;
use super::plain::parse_plain;
use super::rst::parse_rst;
use super::sgml::parse_xmldoc;
use crate::language::canonical_language_name;
use crate::model::{Block, ParsedLine, protect_spaces_in_ranges};
use crate::width::{leading_width, split_at_visual_width, str_width, tabs_to_spaces};
use crate::{CustomMarkers, RewrapRequest, Settings};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Flavor {
    Markdown,
    Javadoc,
    Dartdoc,
    Godoc,
    XmlDoc,
    Rst,
    PsDoc,
}

#[derive(Clone, Debug)]
struct LineDef {
    pattern: String,
    flavor: Flavor,
    forbidden_next: &'static str,
}

#[derive(Clone, Debug)]
struct BlockDef {
    start: String,
    end: String,
    body_marker: &'static str,
    default_body_marker: &'static str,
    flavor: Flavor,
    lua_equals: bool,
    forbidden_next: &'static str,
}

#[derive(Clone, Debug)]
enum CommentDef {
    Line(LineDef),
    Block(BlockDef),
}

fn line(pattern: &str, flavor: Flavor) -> CommentDef {
    CommentDef::Line(LineDef {
        pattern: pattern.to_owned(),
        flavor,
        forbidden_next: "",
    })
}

fn line_except(pattern: &str, forbidden_next: &'static str, flavor: Flavor) -> CommentDef {
    CommentDef::Line(LineDef {
        pattern: pattern.to_owned(),
        flavor,
        forbidden_next,
    })
}

fn block(
    start: &str,
    end: &str,
    body_marker: &'static str,
    default_body_marker: &'static str,
    flavor: Flavor,
) -> CommentDef {
    CommentDef::Block(BlockDef {
        start: start.to_owned(),
        end: end.to_owned(),
        body_marker,
        default_body_marker,
        flavor,
        lua_equals: false,
        forbidden_next: "",
    })
}

fn block_except(
    start: &str,
    end: &str,
    forbidden_next: &'static str,
    flavor: Flavor,
) -> CommentDef {
    let mut definition = block(start, end, "", "", flavor);
    if let CommentDef::Block(block) = &mut definition {
        block.forbidden_next = forbidden_next;
    }
    definition
}

fn c_block() -> CommentDef {
    block(r"/\*", r"\*/", "*", "", Flavor::Markdown)
}

fn javadoc_block(flavor: Flavor) -> CommentDef {
    block(r"/\*[*!]", r"\*/", "*", " * ", flavor)
}

fn definitions(request: &RewrapRequest) -> Vec<CommentDef> {
    let markdown = Flavor::Markdown;
    if let Some(language) = canonical_language_name(&request.file) {
        return language_definitions(&language.to_ascii_lowercase());
    }

    let markers = cached_custom_markers(request);
    let mut definitions = Vec::new();
    if !markers.block.0.is_empty() && !markers.block.1.is_empty() {
        let custom = block(
            &regex::escape(&markers.block.0),
            &regex::escape(&markers.block.1),
            "",
            "",
            markdown,
        );
        definitions.push(custom);
    }
    if !markers.line.is_empty() {
        definitions.push(line(&regex::escape(&markers.line), markdown));
    }
    definitions
}

fn cached_custom_markers(request: &RewrapRequest) -> CustomMarkers {
    static CUSTOM_LANGUAGES: OnceLock<Mutex<HashMap<String, CustomMarkers>>> = OnceLock::new();

    let markers = &request.file.custom_markers;
    let valid =
        !markers.line.is_empty() || (!markers.block.0.is_empty() && !markers.block.1.is_empty());
    let key = request.file.language.to_ascii_lowercase();
    if !valid || key.trim().is_empty() || key == "plaintext" {
        return markers.clone();
    }

    let cache = CUSTOM_LANGUAGES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    cache.entry(key).or_insert_with(|| markers.clone()).clone()
}

#[expect(
    clippy::too_many_lines,
    reason = "the match mirrors the canonical reference language registry"
)]
fn language_definitions(language: &str) -> Vec<CommentDef> {
    let markdown = Flavor::Markdown;
    match language {
        "autohotkey" => vec![line(";", markdown), c_block()],
        "basic" => vec![line("'''", Flavor::XmlDoc), line("'", markdown)],
        "batch file" => vec![line(r"(?:@?rem|::)", markdown)],
        "c/c++" => vec![
            line("///", Flavor::XmlDoc),
            line(r"//!?", Flavor::Javadoc),
            line("//", markdown),
            javadoc_block(Flavor::Javadoc),
            c_block(),
        ],
        "c#" => vec![
            line("///", Flavor::XmlDoc),
            line("//", markdown),
            javadoc_block(Flavor::Javadoc),
            c_block(),
        ],
        "clojure" | "common lisp" | "emacs lisp" | "scheme" => {
            let mut definitions = vec![line(";+", markdown)];
            if matches!(language, "common lisp" | "scheme") {
                definitions.push(block(r"#\|", r"\|#", "", "", markdown));
            }
            definitions
        }
        "coffeescript" => vec![
            block(r"###\*", "###", "*#", " * ", Flavor::Javadoc),
            block("###", "###", "", "", markdown),
            line("#", markdown),
        ],
        "cmake" | "configuration" | "crystal" | "dockerfile" | "makefile" | "perl" | "tcl"
        | "toml" => {
            vec![line("#", markdown)]
        }
        "css"
        | "groovy"
        | "java"
        | "javascript"
        | "json"
        | "less"
        | "objective-c"
        | "scala"
        | "scss"
        | "shaderlab"
        | "swift"
        | "typescript"
        | "verilog/systemverilog" => vec![
            javadoc_block(Flavor::Javadoc),
            c_block(),
            line(r"//[/!]", Flavor::Javadoc),
            line("//", markdown),
        ],
        "fidl" | "prisma" => vec![line("///?", markdown)],
        "d" => vec![
            line("///", markdown),
            line("//", markdown),
            javadoc_block(markdown),
            block(r"/\+\+", r"\+/", "+", " + ", markdown),
            c_block(),
            block(r"/\+", r"\+/", "", "", markdown),
        ],
        "dart" => vec![
            line("///", Flavor::Dartdoc),
            line("//", markdown),
            javadoc_block(Flavor::Dartdoc),
            c_block(),
        ],
        "elixir" => vec![
            line("#", markdown),
            block(
                r#"@(?:module|type|)doc\s+\"\"\""#,
                r#"\"\"\""#,
                "",
                "",
                markdown,
            ),
        ],
        "elm" => vec![
            line("--", markdown),
            block(r"\{-\|?", r"-\}", "", "", markdown),
        ],
        "haskell" => vec![
            line("--", markdown),
            block(r"\{-\s*\|?", r"-\}", "", "", markdown),
        ],
        "purescript" => vec![
            line(r"--\s*\|", markdown),
            line("--", markdown),
            block(r"\{-\s*\|?", r"-\}", "", "", markdown),
        ],
        "f#" => vec![
            line("///", Flavor::XmlDoc),
            line("//", markdown),
            block(r"\(\*", r"\*\)", "", "", markdown),
        ],
        "go" => vec![
            line("//", Flavor::Godoc),
            block(r"/\*[*!]", r"\*/", "", "", Flavor::Godoc),
            c_block(),
        ],
        "graphql" => vec![
            line("#", markdown),
            block(r#".*?\"\"\""#, r#"\"\"\""#, "", "", markdown),
        ],
        "handlebars" => vec![
            block(r"\{\{!--", r"--\}\}", "", "", markdown),
            block(r"\{\{!", r"\}\}", "", "", markdown),
            block("<!--", "-->", "", "", markdown),
        ],
        "hcl" => vec![
            javadoc_block(Flavor::Javadoc),
            c_block(),
            line(r"//[/!]", Flavor::Javadoc),
            line("//", markdown),
            line("#", markdown),
        ],
        "ini" => vec![line("[#;]", markdown)],
        "j" => vec![line(r"NB\.", markdown)],
        "julia" => vec![
            block("#=", "=#", "", "", markdown),
            line("#", markdown),
            block(r#".*?\"\"\""#, r#"\"\"\""#, "", "", markdown),
        ],
        "lean" => vec![
            line("--", markdown),
            block(r"/-[-!]?", "-/", "", "", markdown),
        ],
        "lua" => {
            let mut lua = block(r"--\[(=*)\[", r"\]$1\]", "", "", markdown);
            if let CommentDef::Block(definition) = &mut lua {
                definition.lua_equals = true;
            }
            vec![lua, line("--", markdown)]
        }
        "matlab" => vec![
            line_except("%", "%{}", markdown),
            block(r"%\{", r"%\}", "", "", markdown),
        ],
        "octave" => vec![
            block(r"#\{", r"#\}", "", "", markdown),
            block(r"%\{", r"%\}", "", "", markdown),
            line("##?", markdown),
            line(r"%[^!]", markdown),
        ],
        "pascal" => vec![
            block(r"\(\*", r"\*\)", "", "", markdown),
            block_except(r"\{", r"\}", "$", markdown),
            line("///?", markdown),
        ],
        "php" => vec![
            javadoc_block(Flavor::Javadoc),
            c_block(),
            line(r"(?://|#)", markdown),
        ],
        "powershell" => vec![
            line("#", Flavor::PsDoc),
            block("<#", "#>", "", "", Flavor::PsDoc),
        ],
        "prolog" => vec![
            javadoc_block(Flavor::Javadoc),
            c_block(),
            line(r"%[%!]?", markdown),
        ],
        "protobuf" | "pug" => vec![line("//", markdown)],
        "r" => vec![line("#'?", markdown)],
        "python" => vec![
            line("#", markdown),
            block(r#".*?\"\"\""#, r#"\"\"\""#, "", "", Flavor::Rst),
            block(".*?'''", "'''", "", "", Flavor::Rst),
        ],
        "ruby" => vec![
            line("#", markdown),
            block("=begin", "=end", "", "", markdown),
        ],
        "rust" => vec![line(r"//[/!]?", markdown)],
        "shell script" => vec![line_except("#", "!", markdown)],
        "sql" => vec![line("--", markdown), c_block()],
        "yaml" => vec![line("#{1,3}", markdown)],
        _ => Vec::new(),
    }
}

fn match_line(definition: &LineDef, value: &str) -> Option<usize> {
    let found = Regex::new(&format!(r"(?i)^\s*{}", definition.pattern))
        .expect("valid comment marker")
        .find(value)?;
    if value[found.end()..]
        .chars()
        .next()
        .is_some_and(|next| definition.forbidden_next.contains(next))
    {
        None
    } else {
        Some(found.end())
    }
}

fn match_block_start(definition: &BlockDef, value: &str) -> Option<(usize, String)> {
    if !definition.forbidden_next.is_empty() {
        let marker = Regex::new(&format!(r"(?i)^\s*{}", definition.start))
            .expect("valid block comment marker")
            .find(value)?;
        if value[marker.end()..]
            .chars()
            .next()
            .is_some_and(|next| definition.forbidden_next.contains(next))
        {
            return None;
        }
    }
    let captures = Regex::new(&format!(r"(?i)^\s*{}\s*", definition.start))
        .expect("valid block comment marker")
        .captures(value)?;
    let full = captures.get(0)?;
    let end = if definition.lua_equals {
        format!("]{}]", captures.get(1).map_or("", |group| group.as_str()))
    } else {
        definition.end.clone()
    };
    Some((full.end(), end))
}

fn contains_text(value: &str) -> bool {
    let trimmed = value.trim();
    if matches!(trimmed, "=begin" | "=end") {
        return false;
    }
    value
        .chars()
        .any(|character| character.is_ascii_alphanumeric() || u32::from(character) >= 0x00c0)
}

fn reformat_prefix(prefix: &str, settings: Settings) -> String {
    if !settings.reformat {
        return prefix.to_owned();
    }
    let trimmed = prefix.trim_end();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed} ")
    }
}

fn parse_content(
    lines: &[ParsedLine],
    start: usize,
    settings: Settings,
    flavor: Flavor,
) -> Vec<Block> {
    match flavor {
        Flavor::XmlDoc => parse_xmldoc(lines, start, settings),
        Flavor::Rst => parse_rst(lines, start, settings),
        Flavor::PsDoc => parse_psdoc(lines, start, settings),
        Flavor::Markdown => parse_markdown(lines, start, settings, false),
        Flavor::Godoc => {
            let mut blocks = Vec::new();
            let mut index = 0;
            while index < lines.len() {
                if lines[index].content.trim().is_empty()
                    || lines[index].content.starts_with([' ', '\t'])
                {
                    blocks.push(Block::no_wrap(start + index, vec![lines[index].clone()]));
                    index += 1;
                } else {
                    let mut end = index + 1;
                    while end < lines.len()
                        && !lines[end].content.trim().is_empty()
                        && !lines[end].content.starts_with([' ', '\t'])
                        && !lines[end - 1].content.ends_with("  ")
                    {
                        end += 1;
                    }
                    blocks.push(Block::wrap(start + index, lines[index..end].to_vec(), None));
                    index = end;
                }
            }
            blocks
        }
        Flavor::Dartdoc => parse_tagged(
            lines,
            start,
            settings,
            r"^\s*(?:@nodoc|\{@template|\{@endtemplate|\{@macro)",
            false,
        ),
        Flavor::Javadoc => parse_tagged(lines, start, settings, r"^\s*@\w+", true),
    }
}

fn parse_psdoc(lines: &[ParsedLine], start: usize, settings: Settings) -> Vec<Block> {
    let tag = Regex::new(r"^\s*\.([A-Z]+)").expect("valid PowerShell help tag regex");
    let prompt = Regex::new(r"^\s*PS C:\\>").expect("valid PowerShell prompt regex");
    let mut blocks = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let Some(captures) = tag.captures(&lines[index].content) else {
            let mut end = index + 1;
            while end < lines.len() && !tag.is_match(&lines[end].content) {
                end += 1;
            }
            let body = psdoc_body(&lines[index..end]);
            blocks.extend(parse_markdown(&body, start + index, settings, false));
            index = end;
            continue;
        };
        let name = captures.get(1).map_or("", |group| group.as_str());
        blocks.push(Block::no_wrap(start + index, vec![lines[index].clone()]));
        index += 1;
        let section_end = (index..lines.len())
            .find(|candidate| tag.is_match(&lines[*candidate].content))
            .unwrap_or(lines.len());
        if name != "EXAMPLE" {
            let body = psdoc_body(&lines[index..section_end]);
            blocks.extend(parse_markdown(&body, start + index, settings, false));
            index = section_end;
            continue;
        }

        let mut first_content = true;
        while index < section_end {
            if lines[index].content.trim().is_empty() || prompt.is_match(&lines[index].content) {
                blocks.push(Block::no_wrap(start + index, vec![lines[index].clone()]));
                if prompt.is_match(&lines[index].content) {
                    first_content = false;
                }
                index += 1;
            } else if first_content {
                blocks.push(Block::no_wrap(start + index, vec![lines[index].clone()]));
                first_content = false;
                index += 1;
            } else {
                let mut end = index + 1;
                while end < section_end
                    && !lines[end].content.trim().is_empty()
                    && !prompt.is_match(&lines[end].content)
                {
                    end += 1;
                }
                blocks.extend(parse_markdown(
                    &lines[index..end],
                    start + index,
                    settings,
                    false,
                ));
                index = end;
            }
        }
    }
    blocks
}

fn psdoc_body(lines: &[ParsedLine]) -> Vec<ParsedLine> {
    let whitespace = lines
        .iter()
        .find(|line| !line.content.trim().is_empty())
        .map_or("", |line| {
            &line.content[..line.content.len() - line.content.trim_start().len()]
        });
    let whitespace_chars = whitespace.chars().count();
    lines
        .iter()
        .cloned()
        .map(|mut line| {
            if line.content.trim().is_empty() {
                return line;
            }
            let remove_chars = whitespace_chars.min(
                line.content
                    .chars()
                    .take_while(|character| character.is_whitespace())
                    .count(),
            );
            let remove_bytes = line
                .content
                .char_indices()
                .nth(remove_chars)
                .map_or(line.content.len(), |(index, _)| index);
            line.prefix.push_str(whitespace);
            line.content = line.content[remove_bytes..].to_owned();
            line
        })
        .collect()
}

fn parse_tagged(
    lines: &[ParsedLine],
    start: usize,
    settings: Settings,
    pattern: &str,
    wrap_tag_with_text: bool,
) -> Vec<Block> {
    let tag = Regex::new(&format!("(?i){pattern}")).expect("valid tag regex");
    let example = Regex::new(r"(?i)^\s*@example(?:\s*$)").expect("valid example tag regex");
    let mut blocks = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if tag.is_match(&lines[index].content) {
            if wrap_tag_with_text && example.is_match(&lines[index].content) {
                let mut end = index + 1;
                while end < lines.len() && !tag.is_match(&lines[end].content) {
                    end += 1;
                }
                blocks.push(Block::no_wrap(start + index, lines[index..end].to_vec()));
                index = end;
                continue;
            }
            let has_tag_text = lines[index]
                .content
                .split_once(char::is_whitespace)
                .is_some_and(|(_, rest)| !rest.trim().is_empty());
            if wrap_tag_with_text && has_tag_text {
                let mut end = index + 1;
                while end < lines.len() && !tag.is_match(&lines[end].content) {
                    end += 1;
                }
                let tagged = freeze_inline_tags(&lines[index..end]);
                blocks.extend(parse_markdown(&tagged, start + index, settings, false));
                index = end;
            } else {
                blocks.push(Block::no_wrap(start + index, vec![lines[index].clone()]));
                index += 1;
            }
        } else {
            let mut end = index + 1;
            while end < lines.len() && !tag.is_match(&lines[end].content) {
                end += 1;
            }
            let tagged = if wrap_tag_with_text {
                freeze_inline_tags(&lines[index..end])
            } else {
                lines[index..end].to_vec()
            };
            blocks.extend(parse_markdown(&tagged, start + index, settings, false));
            index = end;
        }
    }
    blocks
}

fn freeze_inline_tags(lines: &[ParsedLine]) -> Vec<ParsedLine> {
    let inline = Regex::new(r"(?i)\{@[a-z]+.*?[^\\]\}").expect("valid inline tag regex");
    lines
        .iter()
        .cloned()
        .map(|mut line| {
            let ranges = inline
                .find_iter(&line.content)
                .map(|found| found.range())
                .collect::<Vec<_>>();
            if !ranges.is_empty() {
                line.content = protect_spaces_in_ranges(&line.content, &ranges);
                line.protected_spaces = true;
            }
            line
        })
        .collect()
}

fn process_comment_lines(
    raw_lines: &[ParsedLine],
    decorations: &[bool],
    start: usize,
    settings: Settings,
    flavor: Flavor,
    comment_id: usize,
) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut index = 0;
    while index < raw_lines.len() {
        if decorations[index] {
            let mut block = Block::no_wrap(start + index, vec![raw_lines[index].clone()]);
            block.comment = Some(comment_id);
            blocks.push(block);
            index += 1;
        } else {
            let mut end = index + 1;
            while end < raw_lines.len() && !decorations[end] {
                end += 1;
            }
            let mut parsed = parse_content(&raw_lines[index..end], start + index, settings, flavor);
            for block in &mut parsed {
                block.comment = Some(comment_id);
            }
            blocks.append(&mut parsed);
            index = end;
        }
    }
    blocks
}

fn parse_line_comment(
    lines: &[ParsedLine],
    start: usize,
    definition: &LineDef,
    settings: Settings,
    comment_id: usize,
) -> (Vec<Block>, usize) {
    let first_end = match_line(definition, &lines[0].content).expect("matched line comment");
    let first_width = str_width(settings.tab_width, &lines[0].content[..first_end]);
    let mut ends = vec![first_end];
    let mut consumed = 1;
    while consumed < lines.len() {
        let Some(end) = match_line(definition, &lines[consumed].content) else {
            break;
        };
        if str_width(settings.tab_width, &lines[consumed].content[..end]) != first_width {
            break;
        }
        ends.push(end);
        consumed += 1;
    }

    let indent = (0..consumed)
        .filter(|index| contains_text(&lines[*index].content[ends[*index]..]))
        .map(|index| {
            leading_width(
                &lines[index].content[ends[index]..],
                settings.tab_width,
                first_width,
            )
        })
        .min()
        .unwrap_or(usize::MAX);
    let mut parsed = Vec::new();
    let mut decorations = Vec::new();
    for index in 0..consumed {
        let original = &lines[index].content;
        let marker = &original[..ends[index]];
        let rest = &original[ends[index]..];
        let rest_indent = leading_width(rest, settings.tab_width, first_width);
        let decoration = !rest.trim().is_empty() && !contains_text(rest) && rest_indent < indent;
        if decoration {
            parsed.push(lines[index].clone());
        } else {
            let width = if indent == usize::MAX {
                rest_indent
            } else {
                indent
            };
            let (left, right) = split_at_visual_width(rest, width, settings.tab_width, first_width);
            let mut prefix = reformat_prefix(&format!("{marker}{left}"), settings);
            let content = tabs_to_spaces(
                &right,
                settings.tab_width,
                str_width(settings.tab_width, &prefix),
            );
            if content.is_empty() {
                prefix.truncate(prefix.trim_end().len());
            }
            parsed.push(ParsedLine::new(prefix, content));
        }
        decorations.push(decoration);
    }
    (
        process_comment_lines(
            &parsed,
            &decorations,
            start,
            settings,
            definition.flavor,
            comment_id,
        ),
        consumed,
    )
}

struct BlockExtent {
    start_end: usize,
    end_line: usize,
    end_match_start: Option<usize>,
    consumed: usize,
}

fn block_extent(lines: &[ParsedLine], definition: &BlockDef) -> BlockExtent {
    let (start_end, end_pattern) =
        match_block_start(definition, &lines[0].content).expect("matched block comment");
    let end_regex = Regex::new(&format!("(?i){end_pattern}")).expect("valid end marker");
    let mut consumed = 1;
    let mut end_line = 0;
    let mut end_match_start = end_regex
        .find(&lines[0].content[start_end..])
        .map(|found| found.start());
    while end_match_start.is_none() && consumed < lines.len() {
        if let Some(found) = end_regex.find(&lines[consumed].content) {
            end_line = consumed;
            end_match_start = Some(found.start());
            consumed += 1;
            break;
        }
        consumed += 1;
    }
    BlockExtent {
        start_end,
        end_line,
        end_match_start,
        consumed,
    }
}

fn single_block_line(
    line: &ParsedLine,
    start_end: usize,
    end_match_start: usize,
    settings: Settings,
) -> (Vec<ParsedLine>, Vec<bool>) {
    let decoration = !contains_text(&line.content[start_end..start_end + end_match_start]);
    let parsed = if decoration {
        line.clone()
    } else {
        consume_block_prefix(line, start_end, settings)
    };
    (vec![parsed], vec![decoration])
}

fn multiline_block_lines(
    lines: &[ParsedLine],
    definition: &BlockDef,
    extent: &BlockExtent,
    settings: Settings,
) -> (Vec<ParsedLine>, Vec<bool>) {
    let first_rest = &lines[0].content[extent.start_end..];
    let first_decoration = !contains_text(first_rest);
    let mut parsed = vec![if first_decoration {
        lines[0].clone()
    } else {
        consume_block_prefix(&lines[0], extent.start_end, settings)
    }];
    let mut decorations = vec![first_decoration];
    let body_end = if extent.end_match_start.is_some() {
        extent.end_line + 1
    } else {
        extent.consumed
    };
    let prefix_regex = if definition.body_marker.is_empty() {
        Regex::new(r"^\s*").expect("valid whitespace regex")
    } else {
        Regex::new(&format!(
            r"^\s*[{}]?\s*",
            regex::escape(definition.body_marker)
        ))
        .expect("valid body marker regex")
    };
    let indent = (1..body_end)
        .filter_map(|index| {
            let line = &lines[index].content;
            let end = prefix_regex.find(line)?.end();
            contains_text(line).then(|| str_width(settings.tab_width, &line[..end]))
        })
        .min()
        .unwrap_or(0);
    for (index, line) in lines.iter().enumerate().take(body_end).skip(1) {
        let standalone_end = index == extent.end_line
            && extent
                .end_match_start
                .is_some_and(|end| !contains_text(&line.content[..end]));
        let prefix_match = prefix_regex
            .find(&line.content)
            .map_or(0, |found| found.end());
        let prefix_width = str_width(settings.tab_width, &line.content[..prefix_match]);
        let decoration = standalone_end
            || (prefix_match < line.content.len()
                && !line.content.trim().is_empty()
                && !contains_text(&line.content)
                && prefix_width < indent);
        if decoration {
            parsed.push(line.clone());
            decorations.push(true);
            continue;
        }
        let (left, right) = split_at_visual_width(&line.content, indent, settings.tab_width, 0);
        let prefix = reformat_prefix(&left, settings);
        parsed.push(ParsedLine::new(
            prefix.clone(),
            tabs_to_spaces(
                &right,
                settings.tab_width,
                str_width(settings.tab_width, &prefix),
            ),
        ));
        decorations.push(false);
    }
    (parsed, decorations)
}

fn set_single_block_tail(blocks: &mut [Block], lines: &[ParsedLine], definition: &BlockDef) {
    for block in blocks {
        if let crate::model::BlockKind::Wrap { default_tail } = &mut block.kind
            && block.lines.len() == 1
        {
            let indent =
                &lines[0].content[..lines[0].content.len() - lines[0].content.trim_start().len()];
            *default_tail = Some(if definition.flavor == Flavor::Rst {
                let marker = if definition.end.contains("'''") {
                    "'''"
                } else {
                    "\"\"\""
                };
                " ".repeat(lines[0].content.find(marker).unwrap_or_default())
            } else {
                format!("{indent}{}", definition.default_body_marker)
            });
        }
    }
}

pub(crate) fn parse_html_comment(
    lines: &[ParsedLine],
    start: usize,
    settings: Settings,
    comment_id: usize,
) -> (Vec<Block>, usize) {
    let definition = BlockDef {
        start: "<!--".to_owned(),
        end: "-->".to_owned(),
        body_marker: "",
        default_body_marker: "",
        flavor: Flavor::Markdown,
        lua_equals: false,
        forbidden_next: "",
    };
    parse_block_comment(lines, start, &definition, settings, comment_id)
}

fn parse_block_comment(
    lines: &[ParsedLine],
    start: usize,
    definition: &BlockDef,
    settings: Settings,
    comment_id: usize,
) -> (Vec<Block>, usize) {
    let extent = block_extent(lines, definition);
    let single_line = extent.end_line == 0 && extent.end_match_start.is_some();
    let (parsed, decorations) = if single_line {
        single_block_line(
            &lines[0],
            extent.start_end,
            extent.end_match_start.expect("single-line end marker"),
            settings,
        )
    } else {
        multiline_block_lines(lines, definition, &extent, settings)
    };

    let mut blocks = process_comment_lines(
        &parsed,
        &decorations,
        start,
        settings,
        definition.flavor,
        comment_id,
    );
    if single_line {
        set_single_block_tail(&mut blocks, lines, definition);
    }
    (blocks, extent.consumed)
}

fn consume_block_prefix(line: &ParsedLine, end: usize, settings: Settings) -> ParsedLine {
    let prefix = reformat_prefix(&line.content[..end], settings);
    ParsedLine::new(prefix, line.content[end..].to_owned())
}

pub(crate) fn parse_source(request: &RewrapRequest, lines: &[ParsedLine]) -> Vec<Block> {
    let definitions = definitions(request);
    let yaml = canonical_language_name(&request.file).is_some_and(|name| name == "YAML");
    let mut blocks = Vec::new();
    let mut index = 0;
    let mut comment_id = 0;
    while index < lines.len() {
        let mut matched = false;
        for definition in &definitions {
            match definition {
                CommentDef::Line(line_definition)
                    if match_line(line_definition, &lines[index].content).is_some() =>
                {
                    let (mut parsed, consumed) = parse_line_comment(
                        &lines[index..],
                        index,
                        line_definition,
                        request.settings,
                        comment_id,
                    );
                    blocks.append(&mut parsed);
                    index += consumed;
                    comment_id += 1;
                    matched = true;
                    break;
                }
                CommentDef::Block(block_definition)
                    if match_block_start(block_definition, &lines[index].content).is_some() =>
                {
                    let (mut parsed, consumed) = parse_block_comment(
                        &lines[index..],
                        index,
                        block_definition,
                        request.settings,
                        comment_id,
                    );
                    blocks.append(&mut parsed);
                    index += consumed;
                    comment_id += 1;
                    matched = true;
                    break;
                }
                _ => {}
            }
        }
        if matched {
            continue;
        }

        let mut end = index + 1;
        while end < lines.len()
            && !definitions.iter().any(|definition| match definition {
                CommentDef::Line(line_definition) => {
                    match_line(line_definition, &lines[end].content).is_some()
                }
                CommentDef::Block(block_definition) => {
                    match_block_start(block_definition, &lines[end].content).is_some()
                }
            })
        {
            end += 1;
        }
        if yaml {
            blocks.extend(parse_plain(&lines[index..end], index, request.settings));
        } else {
            blocks.push(Block::no_wrap(index, lines[index..end].to_vec()));
        }
        index = end;
    }
    blocks
}
