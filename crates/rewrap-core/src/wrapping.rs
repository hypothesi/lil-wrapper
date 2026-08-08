use crate::Settings;
use crate::model::{ParsedLine, escape_literal_nuls, restore_protected_spaces};
use crate::width::{code_unit_width, str_width};

const CJ_NO_START: &str = "})]?,;¢°′″‰℃、。｡､￠，．：；？！％・･ゝゞヽヾーァィゥェォッャュョヮヵヶぁぃぅぇぉっゃゅょゎゕゖㇰㇱㇲㇳㇴㇵㇶㇷㇸㇹㇺㇻㇼㇽㇾㇿ々〻ｧｨｩｪｫｬｭｮｯｰ”〉》」』】〕）］｝｣";
const CJ_NO_END: &str = "([{‘“〈《「『【〔（［｛｢£¥＄￡￥＋";

fn is_whitespace(unit: u16) -> bool {
    (unit != 0 && unit <= 0x20) || unit == 0x3000
}

fn is_cj(unit: u16) -> bool {
    (0x3040..=0x30ff).contains(&unit)
        || (0x3400..=0x4dbf).contains(&unit)
        || (0x4e00..=0x9fff).contains(&unit)
}

fn contains_unit(text: &str, unit: u16) -> bool {
    text.encode_utf16().any(|candidate| candidate == unit)
}

fn can_break_between(left: u16, right: u16) -> bool {
    if is_whitespace(left) || is_whitespace(right) {
        true
    } else if contains_unit(CJ_NO_END, left) || contains_unit(CJ_NO_START, right) {
        false
    } else {
        is_cj(left) || is_cj(right)
    }
}

fn concat_lines(lines: &[ParsedLine], double_sentence_spacing: bool) -> String {
    let content = |line: &ParsedLine| {
        if line.protected_spaces {
            line.content.clone()
        } else {
            escape_literal_nuls(&line.content)
        }
    };
    let mut result = lines.first().map_or_else(String::new, content);
    for line in &lines[1..] {
        if line.content.is_empty() || result.is_empty() {
            continue;
        }
        result.truncate(result.trim_end().len());
        let left = result.encode_utf16().last().unwrap_or_default();
        let right = line.content.encode_utf16().next().unwrap_or_default();
        if !is_cj(left) && !is_cj(right) {
            result.push(' ');
            if double_sentence_spacing && matches!(left, 0x2e | 0x3f | 0x21) {
                result.push(' ');
            }
        }
        result.push_str(&content(line));
    }
    result
}

fn decode(units: &[u16]) -> String {
    restore_protected_spaces(&String::from_utf16_lossy(units))
}

pub(crate) fn wrap_lines(
    lines: &[ParsedLine],
    default_tail: Option<&str>,
    settings: Settings,
) -> Vec<String> {
    if lines.is_empty() {
        return Vec::new();
    }
    let mut prefixes = lines
        .iter()
        .map(|line| line.prefix.clone())
        .collect::<Vec<_>>();
    if prefixes.len() == 1 {
        if let Some(tail) = default_tail {
            prefixes.push(tail.to_owned());
        }
    }
    let text = concat_lines(lines, settings.double_sentence_spacing);
    let units = text.encode_utf16().collect::<Vec<_>>();
    let max_width = if settings.column == 0 {
        usize::MAX
    } else {
        settings.column
    };
    let mut output = Vec::new();
    let mut prefix_index = 0;
    let mut line_start = 0;
    let mut position = 0;
    let mut current_width = str_width(settings.tab_width, &prefixes[0]);

    while position < units.len() {
        let unit = units[position];
        let protected_pair = unit == 0
            && units
                .get(position + 1)
                .is_some_and(|next| matches!(*next, 0 | 0x73));
        let next_width = current_width
            + if protected_pair {
                1
            } else {
                code_unit_width(settings.tab_width.max(1), current_width, unit)
            };
        if next_width <= max_width || is_whitespace(unit) {
            current_width = next_width;
            position += if protected_pair { 2 } else { 1 };
            continue;
        }

        let mut break_position = position;
        while break_position > line_start
            && !can_break_between(units[break_position - 1], units[break_position])
        {
            break_position -= 1;
        }
        if break_position <= line_start {
            current_width = next_width;
            position += 1;
            continue;
        }

        let content = decode(&units[line_start..break_position]);
        output.push(format!("{}{}", prefixes[prefix_index], content.trim()));
        prefix_index = (prefix_index + 1).min(prefixes.len() - 1);
        line_start = break_position;
        position = break_position;
        current_width = str_width(settings.tab_width, &prefixes[prefix_index]);
    }

    let content = decode(&units[line_start..]);
    output.push(format!(
        "{}{}",
        prefixes[prefix_index],
        content.trim_start()
    ));
    output
}
