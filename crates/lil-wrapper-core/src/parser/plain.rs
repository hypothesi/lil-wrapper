use crate::Settings;
use crate::model::{Block, ParsedLine};
use crate::width::leading_width;

pub(crate) fn parse_plain(lines: &[ParsedLine], start: usize, settings: Settings) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if lines[index].content.trim().is_empty() {
            blocks.push(Block::no_wrap(start + index, vec![lines[index].clone()]));
            index += 1;
            continue;
        }
        let first_indent = leading_width(&lines[index].content, settings.tab_width, 0);
        let mut end = index + 1;
        while end < lines.len() && !lines[end].content.trim().is_empty() {
            let indent = leading_width(&lines[end].content, settings.tab_width, 0);
            if indent.abs_diff(first_indent) >= 2 || lines[end - 1].content.ends_with("  ") {
                break;
            }
            end += 1;
        }
        let mut parsed = lines[index..end].to_vec();
        let first_whitespace = &lines[index].content
            [..lines[index].content.len() - lines[index].content.trim_start().len()];
        for line in &mut parsed {
            line.prefix.push_str(first_whitespace);
            line.content = line.content.trim_start().to_owned();
        }
        blocks.push(Block::wrap(start + index, parsed, None));
        index = end;
    }
    blocks
}
