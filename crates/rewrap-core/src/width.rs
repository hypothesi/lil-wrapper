#[must_use]
pub fn str_width(tab_width: usize, text: &str) -> usize {
    str_width_from(0, tab_width, text)
}

#[must_use]
pub(crate) fn str_width_from(offset: usize, tab_width: usize, text: &str) -> usize {
    let tab_width = tab_width.max(1);
    text.encode_utf16().fold(offset, |column, unit| {
        column + code_unit_width(tab_width, column, unit)
    }) - offset
}

#[must_use]
pub(crate) const fn code_unit_width(tab_width: usize, column: usize, unit: u16) -> usize {
    match unit {
        0x0009 => tab_width - column % tab_width,
        0x0001..=0x001f => 0,
        0x2e80..=0xd7af | 0xf900..=0xfaff | 0xff01..=0xff5e => 2,
        _ => 1,
    }
}

pub(crate) fn utf16_prefix(text: &str, units: usize) -> &str {
    let byte = text
        .char_indices()
        .scan(0, |count, (byte, character)| {
            let before = *count;
            *count += character.len_utf16();
            Some((byte, before))
        })
        .find_map(|(byte, count)| (count == units).then_some(byte))
        .unwrap_or(text.len());
    &text[..byte]
}

pub(crate) fn leading_width(text: &str, tab_width: usize, offset: usize) -> usize {
    let whitespace = &text[..text.len() - text.trim_start().len()];
    str_width_from(offset, tab_width, whitespace)
}

pub(crate) fn split_at_visual_width(
    text: &str,
    width: usize,
    tab_width: usize,
    offset: usize,
) -> (String, String) {
    if width == 0 {
        return (String::new(), text.to_owned());
    }

    let mut consumed_width = 0;
    let mut byte = 0;
    for (index, character) in text.char_indices() {
        let character_width = character
            .encode_utf16(&mut [0; 2])
            .iter()
            .fold(0, |sum, unit| {
                sum + code_unit_width(tab_width.max(1), offset + consumed_width + sum, *unit)
            });
        if consumed_width + character_width > width {
            let left_spaces = width - consumed_width;
            let right_spaces = character_width - left_spaces;
            return (
                format!("{}{}", &text[..index], " ".repeat(left_spaces)),
                format!(
                    "{}{}",
                    " ".repeat(right_spaces),
                    &text[index + character.len_utf8()..]
                ),
            );
        }
        consumed_width += character_width;
        byte = index + character.len_utf8();
        if consumed_width == width {
            break;
        }
    }
    (text[..byte].to_owned(), text[byte..].to_owned())
}

pub(crate) fn tabs_to_spaces(text: &str, tab_width: usize, offset: usize) -> String {
    let mut result = String::with_capacity(text.len());
    let mut column = offset;
    for character in text.chars() {
        if character == '\t' {
            let spaces = tab_width.max(1) - column % tab_width.max(1);
            result.push_str(&" ".repeat(spaces));
            column += spaces;
        } else {
            result.push(character);
            column += character
                .encode_utf16(&mut [0; 2])
                .iter()
                .map(|unit| code_unit_width(tab_width.max(1), column, *unit))
                .sum::<usize>();
        }
    }
    result
}
