#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CustomMarkers {
    pub line: String,
    pub block: (String, String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct File {
    pub language: String,
    pub path: String,
    pub custom_markers: CustomMarkers,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Settings {
    pub column: usize,
    pub tab_width: usize,
    pub double_sentence_spacing: bool,
    pub reformat: bool,
    pub whole_comment: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Position {
    pub line: usize,
    /// UTF-16 code units, matching editor protocol positions.
    pub character: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Selection {
    pub anchor: Position,
    pub active: Position,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Edit {
    pub start_line: usize,
    pub end_line: isize,
    pub lines: Vec<String>,
    pub selections: Vec<Selection>,
}

impl Edit {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            start_line: 0,
            end_line: -1,
            lines: Vec::new(),
            selections: Vec::new(),
        }
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        (self.end_line < 0 || self.end_line.unsigned_abs() < self.start_line)
            && self.lines.is_empty()
    }

    fn empty_with_selections(selections: &[Selection]) -> Self {
        Self {
            selections: selections.to_vec(),
            ..Self::empty()
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WrapRequest {
    pub file: File,
    pub settings: Settings,
    pub selections: Vec<Selection>,
    pub lines: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocState {
    pub file_path: String,
    pub version: i64,
    pub selections: Vec<Selection>,
}

/// Wrap text according to the reference behavior.
#[must_use]
pub fn wrap(request: &WrapRequest) -> Edit {
    if request.lines.is_empty() {
        return Edit::empty_with_selections(&request.selections);
    }

    let blocks = parse_document(request);
    render_selected(request, &blocks)
}

#[must_use]
pub fn maybe_auto_wrap(request: &WrapRequest, new_text: &str, position: Position) -> Edit {
    if new_text.is_empty()
        || request.settings.column < 1
        || !new_text.chars().all(char::is_whitespace)
    {
        return Edit::empty();
    }

    let (enter_pressed, indent_units) = if let Some(rest) = new_text.strip_prefix("\r\n") {
        (true, rest.encode_utf16().count())
    } else if let Some(rest) = new_text.strip_prefix('\n') {
        (true, rest.encode_utf16().count())
    } else {
        (false, 0)
    };
    if !enter_pressed && new_text.encode_utf16().count() > 1 {
        return Edit::empty();
    }

    let Some(line_text) = request.lines.get(position.line) else {
        return Edit::empty();
    };
    let character =
        position.character + usize::from(!enter_pressed) * new_text.encode_utf16().count();
    let prefix = width::utf16_prefix(line_text, character);
    if str_width(request.settings.tab_width, prefix) <= request.settings.column {
        return Edit::empty();
    }

    let mut auto_request = request.clone();
    auto_request.lines.truncate(position.line + 1);
    let line_end = line_text.encode_utf16().count();
    auto_request.selections = vec![Selection {
        anchor: Position {
            line: position.line,
            character: 0,
        },
        active: Position {
            line: position.line,
            character: line_end,
        },
    }];
    let mut edit = wrap(&auto_request);
    edit.selections = vec![Selection {
        anchor: Position {
            line: position.line + usize::from(enter_pressed),
            character: if enter_pressed {
                indent_units
            } else {
                character
            },
        },
        active: Position {
            line: position.line + usize::from(enter_pressed),
            character: if enter_pressed {
                indent_units
            } else {
                character
            },
        },
    }];
    edit
}
mod columns;
mod language;
mod model;
mod parser;
mod selections;
mod width;
mod wrapping;

use parser::parse_document;
use selections::render_selected;

pub use columns::ColumnState;
pub use language::{language_name_for_file, languages};
pub use width::str_width;
