use rewrap_core::{Position, Selection};
use rewrap_lsp::remap_selections;

fn position(line: usize, character: usize) -> Position {
    Position { line, character }
}

fn selection(anchor: Position, active: Position) -> Selection {
    Selection { anchor, active }
}

#[test]
fn keeps_a_cursor_attached_to_its_word_when_a_line_splits() {
    let mapped = remap_selections(
        &["one two three".to_owned()],
        &["one two".to_owned(), "three".to_owned()],
        0,
        0,
        &[selection(position(0, 8), position(0, 8))],
    );

    assert_eq!(mapped, [selection(position(1, 0), position(1, 0))]);
}

#[test]
fn shifts_points_below_an_edit_by_the_line_count_delta() {
    let mapped = remap_selections(
        &["one two three".to_owned()],
        &["one two".to_owned(), "three".to_owned()],
        2,
        2,
        &[selection(position(4, 3), position(4, 3))],
    );

    assert_eq!(mapped, [selection(position(5, 3), position(5, 3))]);
}

#[test]
fn preserves_anchor_and_active_direction() {
    let mapped = remap_selections(
        &["one two three".to_owned()],
        &["one two".to_owned(), "three".to_owned()],
        0,
        0,
        &[selection(position(0, 13), position(0, 4))],
    );

    assert_eq!(mapped[0].anchor, position(1, 5));
    assert_eq!(mapped[0].active, position(0, 4));
}

#[test]
fn computes_offsets_in_utf16_code_units() {
    let mapped = remap_selections(
        &["a😀 b c".to_owned()],
        &["a😀 b".to_owned(), "c".to_owned()],
        0,
        0,
        &[selection(position(0, 7), position(0, 7))],
    );

    assert_eq!(mapped, [selection(position(1, 1), position(1, 1))]);
}
