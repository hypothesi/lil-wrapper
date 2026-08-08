use rewrap_core::{
    ColumnState, CustomMarkers, DocState, File, Position, RewrapRequest, Selection, Settings,
    language_name_for_file, maybe_auto_wrap, str_width,
};

fn position(line: usize, character: usize) -> Position {
    Position { line, character }
}

fn cursor(line: usize, character: usize) -> Selection {
    Selection {
        anchor: position(line, character),
        active: position(line, character),
    }
}

fn file(language: &str, path: &str) -> File {
    File {
        language: language.to_owned(),
        path: path.to_owned(),
        custom_markers: CustomMarkers::default(),
    }
}

fn request(column: usize, line: &str) -> RewrapRequest {
    RewrapRequest {
        file: file("plaintext", ""),
        settings: Settings {
            column,
            tab_width: 4,
            double_sentence_spacing: false,
            reformat: false,
            whole_comment: true,
        },
        selections: Vec::new(),
        lines: vec![line.to_owned()],
    }
}

#[test]
fn computes_the_reference_utf16_visual_width() {
    assert_eq!(str_width(4, "abc"), 3);
    assert_eq!(str_width(4, "a\tb"), 5);
    assert_eq!(str_width(2, "a\tb"), 3);
    assert_eq!(str_width(4, "\0\u{0001}"), 1);
    assert_eq!(str_width(4, "界"), 2);
    assert_eq!(str_width(4, "Ａ"), 2);
    assert_eq!(str_width(4, "😀"), 2);
}

#[test]
fn resolves_languages_by_editor_id_or_plaintext_file_name() {
    assert_eq!(
        language_name_for_file(&file("markdown", "/tmp/notes.txt")),
        Some("Markdown")
    );
    assert_eq!(
        language_name_for_file(&file("plaintext", "/tmp/README.md")),
        Some("Markdown")
    );
    assert_eq!(
        language_name_for_file(&file("", r"C:\work\Dockerfile")),
        Some("Dockerfile")
    );
    assert_eq!(
        language_name_for_file(&file("unknown-language", "/tmp/README.md")),
        None,
        "an explicit unknown editor language must not fall back to the extension"
    );
}

#[test]
fn tracks_and_cycles_rulers_only_after_identical_consecutive_wraps() {
    let mut state = ColumnState::default();
    let first = DocState {
        file_path: "/tmp/a.md".to_owned(),
        version: 1,
        selections: vec![cursor(0, 0)],
    };

    assert_eq!(state.wrapping_column(&first.file_path, &[72, 88]), 72);
    assert_eq!(state.maybe_change_wrapping_column(&first, &[72, 88]), 72);
    state.save_document(first.clone());
    assert_eq!(state.maybe_change_wrapping_column(&first, &[72, 88]), 88);

    let moved = DocState {
        selections: vec![cursor(0, 1)],
        ..first.clone()
    };
    state.save_document(first);
    assert_eq!(state.maybe_change_wrapping_column(&moved, &[72, 88]), 88);
    assert_eq!(state.wrapping_column(&moved.file_path, &[100, 120]), 100);
}

#[test]
fn keeps_ruler_state_separate_for_each_file() {
    let mut state = ColumnState::default();
    assert_eq!(state.wrapping_column("/tmp/a", &[72, 88]), 72);
    assert_eq!(state.wrapping_column("/tmp/b", &[100, 120]), 100);
    assert_eq!(state.wrapping_column("/tmp/a", &[72, 88]), 72);
}

#[test]
fn auto_wrap_rejects_non_typing_and_ineligible_changes() {
    let over_column = request(8, "one two three ");
    let empty = maybe_auto_wrap(&over_column, "", position(0, 13));
    assert!(
        empty.is_empty(),
        "deletions and empty edits do not trigger auto-wrap"
    );
    assert!(empty.selections.is_empty());
    assert!(
        maybe_auto_wrap(&over_column, "x", position(0, 13)).is_empty(),
        "non-whitespace insertion does not trigger auto-wrap"
    );
    assert!(
        maybe_auto_wrap(&over_column, "  ", position(0, 12)).is_empty(),
        "multi-character non-newline insertion does not trigger auto-wrap"
    );
    assert!(
        maybe_auto_wrap(&request(0, "one two three "), " ", position(0, 13)).is_empty(),
        "nonpositive wrapping columns disable auto-wrap"
    );
    assert!(
        maybe_auto_wrap(&request(20, "one two three "), " ", position(0, 13)).is_empty(),
        "typing at or before the column does not trigger auto-wrap"
    );
}

#[test]
fn auto_wrap_wraps_single_whitespace_insertions_past_the_column() {
    let edit = maybe_auto_wrap(&request(8, "one two three "), " ", position(0, 13));

    assert!(!edit.is_empty());
    assert_eq!(edit.lines, ["one two", "three "]);
    assert_eq!(edit.selections, [cursor(0, 14)]);
}

#[test]
fn custom_line_and_block_markers_are_used_for_unknown_languages() {
    let mut line_request = request(12, "@@ one two three four");
    line_request.file = File {
        language: "custom".to_owned(),
        path: "/tmp/file.custom".to_owned(),
        custom_markers: CustomMarkers {
            line: "@@".to_owned(),
            block: (String::new(), String::new()),
        },
    };
    let line_edit = rewrap_core::rewrap(&line_request);
    assert_eq!(line_edit.lines, ["@@ one two", "@@ three", "@@ four"]);

    let mut block_request = request(12, "<# one two three four #>");
    block_request.file = File {
        language: "custom-block".to_owned(),
        path: "/tmp/file.custom".to_owned(),
        custom_markers: CustomMarkers {
            line: String::new(),
            block: ("<#".to_owned(), "#>".to_owned()),
        },
    };
    let block_edit = rewrap_core::rewrap(&block_request);
    assert_eq!(block_edit.lines, ["<# one two", "three four", "#>"]);
}
