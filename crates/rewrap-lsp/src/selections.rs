use diff_match_patch::Dmp;
use rewrap_core::{Position, Selection};

#[must_use]
pub fn remap_selections(
    old_lines: &[String],
    new_lines: &[String],
    start_line: usize,
    end_line: usize,
    selections: &[Selection],
) -> Vec<Selection> {
    if new_lines.is_empty() {
        return selections.to_vec();
    }

    let old_text = encode_utf16_units(&old_lines.join("\n"));
    let new_text = encode_utf16_units(&new_lines.join("\n"));
    let mut dmp = Dmp::new();
    // fast-diff always enables diff-match-patch's half-match path. This long
    // practical deadline enables the same path without constraining editor use.
    dmp.diff_timeout = Some(60.0 * 60.0 * 24.0 * 365.0);
    let diffs = dmp.diff_main(&old_text, &new_text, false);
    let new_line_count = isize::try_from(new_lines.len()).unwrap_or(isize::MAX);
    let old_line_count = isize::try_from(old_lines.len()).unwrap_or(isize::MAX);
    let line_growth = new_line_count.saturating_sub(old_line_count);

    selections
        .iter()
        .map(|selection| Selection {
            anchor: remap_position(
                selection.anchor,
                old_lines,
                new_lines,
                start_line,
                end_line,
                line_growth,
                &diffs,
            ),
            active: remap_position(
                selection.active,
                old_lines,
                new_lines,
                start_line,
                end_line,
                line_growth,
                &diffs,
            ),
        })
        .collect()
}

fn remap_position(
    position: Position,
    old_lines: &[String],
    new_lines: &[String],
    start_line: usize,
    end_line: usize,
    line_growth: isize,
    diffs: &[diff_match_patch::Diff],
) -> Position {
    if (start_line..=end_line).contains(&position.line) {
        let relative = Position {
            line: position.line - start_line,
            character: position.character,
        };
        let old_offset = offset_at(old_lines, relative);
        let new_offset = new_offset_from_old(old_offset, diffs);
        let mut mapped = position_at(new_lines, new_offset);
        mapped.line += start_line;
        mapped
    } else if position.line > end_line {
        Position {
            line: position.line.saturating_add_signed(line_growth),
            character: position.character,
        }
    } else {
        position
    }
}

fn encode_utf16_units(text: &str) -> String {
    text.encode_utf16()
        .map(|unit| {
            let scalar = if unit < 0xd800 {
                u32::from(unit)
            } else {
                u32::from(unit) + 0x800
            };
            char::from_u32(scalar).expect("UTF-16 unit encoding is a valid Unicode scalar")
        })
        .collect()
}

fn new_offset_from_old(offset: usize, diffs: &[diff_match_patch::Diff]) -> usize {
    let mut running_offset = 0;
    let mut delta = 0_isize;
    for diff in diffs {
        let length = diff.text.chars().count();
        if diff.operation != 1 {
            if running_offset + length > offset {
                break;
            }
            running_offset += length;
        }
        let operation = isize::try_from(diff.operation).unwrap_or_default();
        let length = isize::try_from(length).unwrap_or(isize::MAX);
        delta = delta.saturating_add(operation.saturating_mul(length));
    }
    offset.saturating_add_signed(delta)
}

fn offset_at(lines: &[String], position: Position) -> usize {
    let previous = lines
        .iter()
        .take(position.line)
        .map(|line| line.encode_utf16().count() + 1)
        .sum::<usize>();
    previous + position.character
}

fn position_at(lines: &[String], mut offset: usize) -> Position {
    for (line, text) in lines.iter().enumerate() {
        let line_length = text.encode_utf16().count() + 1;
        if offset < line_length {
            return Position {
                line,
                character: offset,
            };
        }
        offset -= line_length;
    }
    let line = lines.len().saturating_sub(1);
    Position {
        line,
        character: lines
            .get(line)
            .map_or(0, |text| text.encode_utf16().count()),
    }
}
