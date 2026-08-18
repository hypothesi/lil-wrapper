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

/// A document already parsed into blocks.
///
/// Parsing depends only on the text, file, and settings of a request, never on its selections, so
/// one parse serves every selection in the same document. Hold this across requests that share a
/// document version to avoid reparsing, and pass it to [`wrap_with`].
#[derive(Clone, Debug)]
pub struct ParsedDocument {
    blocks: Vec<Block>,
    file: File,
    settings: Settings,
    fingerprint: u64,
}

fn fingerprint(lines: &[String]) -> u64 {
    let mut hasher = DefaultHasher::new();
    lines.len().hash(&mut hasher);
    for line in lines {
        line.hash(&mut hasher);
    }
    hasher.finish()
}

impl ParsedDocument {
    /// Returns whether this parse describes the given request's document.
    #[must_use]
    pub fn matches(&self, request: &WrapRequest) -> bool {
        self.file == request.file
            && self.settings == request.settings
            && self.fingerprint == fingerprint(&request.lines)
    }
}

/// Parses a document so that its blocks can be reused across selections.
#[must_use]
pub fn parse(request: &WrapRequest) -> ParsedDocument {
    ParsedDocument {
        blocks: parse_document(request),
        file: request.file.clone(),
        settings: request.settings,
        fingerprint: fingerprint(&request.lines),
    }
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

/// Wrap text reusing an existing parse.
///
/// Reparses when `parsed` does not describe `request`, so a stale parse yields the same result as
/// [`wrap`] rather than a wrong one.
#[must_use]
pub fn wrap_with(parsed: &ParsedDocument, request: &WrapRequest) -> Edit {
    if request.lines.is_empty() {
        return Edit::empty_with_selections(&request.selections);
    }
    if !parsed.matches(request) {
        return wrap(request);
    }
    render_selected(request, &parsed.blocks)
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
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

mod columns;
mod language;
mod model;
mod parser;
mod selections;
mod width;
mod wrapping;

use model::Block;
use parser::parse_document;
use selections::render_selected;

pub use columns::ColumnState;
pub use language::{language_name_for_file, languages};
pub use width::str_width;
