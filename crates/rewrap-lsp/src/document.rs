use crate::{CONTENT_MODIFIED, INVALID_PARAMS, RpcError};
use rewrap_core::Position;
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct LspPosition {
    pub line: usize,
    pub character: usize,
}

impl From<LspPosition> for Position {
    fn from(position: LspPosition) -> Self {
        Self {
            line: position.line,
            character: position.character,
        }
    }
}

impl From<Position> for LspPosition {
    fn from(position: Position) -> Self {
        Self {
            line: position.line,
            character: position.character,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct LspRange {
    pub start: LspPosition,
    pub end: LspPosition,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ContentChange {
    pub range: Option<LspRange>,
    pub range_length: Option<usize>,
    pub text: String,
}

#[derive(Clone, Debug)]
pub(crate) struct LastChange {
    pub start: LspPosition,
    pub end: LspPosition,
    pub text: String,
}

#[derive(Clone, Debug)]
pub(crate) struct ChangeBatch {
    pub change_count: usize,
    pub insertion: Option<LastChange>,
    pub separate_newline_indent: Option<LastChange>,
}

#[derive(Clone, Debug)]
struct PendingNewline {
    change: LastChange,
    version: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct Document {
    pub uri: String,
    pub language: String,
    pub version: i64,
    text: String,
    preferred_eol: String,
    pub last_change_batch: Option<ChangeBatch>,
    pending_newline: Option<PendingNewline>,
}

impl Document {
    pub fn new(uri: String, language: String, version: i64, text: String) -> Self {
        let preferred_eol = detect_eol(&text).to_owned();
        Self {
            uri,
            language,
            version,
            text,
            preferred_eol,
            last_change_batch: None,
            pending_newline: None,
        }
    }

    pub fn lines(&self) -> Vec<String> {
        line_spans(&self.text)
            .into_iter()
            .map(|(start, end)| self.text[start..end].to_owned())
            .collect()
    }

    pub fn line_utf16_len(&self, line: usize) -> Option<usize> {
        let (start, end) = *line_spans(&self.text).get(line)?;
        Some(self.text[start..end].encode_utf16().count())
    }

    pub fn preferred_eol(&self) -> &str {
        &self.preferred_eol
    }

    pub fn apply_changes(
        &mut self,
        version: i64,
        changes: &[ContentChange],
    ) -> Result<(), RpcError> {
        if version <= self.version {
            return Err(RpcError::new(
                CONTENT_MODIFIED,
                format!(
                    "stale document version {version}; current version is {}",
                    self.version
                ),
            ));
        }
        if changes.is_empty() {
            return Err(RpcError::new(
                INVALID_PARAMS,
                "contentChanges must not be empty",
            ));
        }

        let mut text = self.text.clone();
        let mut insertion = None;
        for change in changes {
            let explicit_range = change.range;
            let range = change.range.unwrap_or_else(|| LspRange {
                start: LspPosition {
                    line: 0,
                    character: 0,
                },
                end: end_position(&text),
            });
            let start = position_to_byte(&text, range.start)?;
            let end = position_to_byte(&text, range.end)?;
            if start > end {
                return Err(RpcError::new(
                    INVALID_PARAMS,
                    "change range start is after its end",
                ));
            }
            if let Some(expected_length) = change.range_length {
                let actual_length = text[start..end].encode_utf16().count();
                if expected_length != actual_length {
                    return Err(RpcError::new(
                        INVALID_PARAMS,
                        format!(
                            "rangeLength is {expected_length}, but the range contains {actual_length} UTF-16 units"
                        ),
                    ));
                }
            }
            if changes.len() == 1 && explicit_range.is_some() && start == end {
                insertion = Some(LastChange {
                    start: range.start,
                    end: inserted_end(range.start, &change.text),
                    text: change.text.clone(),
                });
            }
            text.replace_range(start..end, &change.text);
        }

        let mut separate_newline_indent = None;
        let mut pending_newline = None;
        if let Some(change) = &insertion {
            if is_newline(&change.text) {
                pending_newline = Some(PendingNewline {
                    change: change.clone(),
                    version,
                });
            } else if is_horizontal_whitespace(&change.text)
                && self
                    .pending_newline
                    .as_ref()
                    .is_some_and(|pending| pending.version == self.version)
            {
                let pending = self
                    .pending_newline
                    .as_ref()
                    .expect("pending newline was checked");
                if change.start == pending.change.end {
                    separate_newline_indent = Some(LastChange {
                        start: pending.change.start,
                        end: change.end,
                        text: format!("{}{}", pending.change.text, change.text),
                    });
                }
            }
        }

        self.text = text;
        self.version = version;
        self.last_change_batch = Some(ChangeBatch {
            change_count: changes.len(),
            insertion,
            separate_newline_indent,
        });
        self.pending_newline = pending_newline;
        if let Some(eol) = detect_optional_eol(&self.text) {
            self.preferred_eol = eol.to_owned();
        }
        Ok(())
    }
}

fn detect_eol(text: &str) -> &'static str {
    detect_optional_eol(text).unwrap_or("\n")
}

fn detect_optional_eol(text: &str) -> Option<&'static str> {
    let bytes = text.as_bytes();
    for (index, byte) in bytes.iter().copied().enumerate() {
        match byte {
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => return Some("\r\n"),
            b'\r' => return Some("\r"),
            b'\n' => return Some("\n"),
            _ => {}
        }
    }
    None
}

fn line_spans(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\r' => {
                spans.push((start, index));
                index += usize::from(bytes.get(index + 1) == Some(&b'\n')) + 1;
                start = index;
            }
            b'\n' => {
                spans.push((start, index));
                index += 1;
                start = index;
            }
            _ => index += 1,
        }
    }
    spans.push((start, text.len()));
    spans
}

fn position_to_byte(text: &str, position: LspPosition) -> Result<usize, RpcError> {
    let spans = line_spans(text);
    let Some(&(start, end)) = spans.get(position.line) else {
        return Err(RpcError::new(
            INVALID_PARAMS,
            format!("line {} is outside the document", position.line),
        ));
    };
    let line = &text[start..end];
    let mut utf16_offset = 0;
    for (byte_offset, character) in line.char_indices() {
        if utf16_offset == position.character {
            return Ok(start + byte_offset);
        }
        let next_offset = utf16_offset + character.len_utf16();
        if position.character < next_offset {
            return Err(RpcError::new(
                INVALID_PARAMS,
                "position splits a UTF-16 surrogate pair",
            ));
        }
        utf16_offset = next_offset;
    }
    if utf16_offset == position.character {
        Ok(end)
    } else {
        Err(RpcError::new(
            INVALID_PARAMS,
            "position is past the end of the line",
        ))
    }
}

fn is_newline(text: &str) -> bool {
    matches!(text, "\n" | "\r" | "\r\n")
}

fn is_horizontal_whitespace(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|character| matches!(character, ' ' | '\t'))
}

fn end_position(text: &str) -> LspPosition {
    let spans = line_spans(text);
    let line = spans.len() - 1;
    let (start, end) = spans[line];
    LspPosition {
        line,
        character: text[start..end].encode_utf16().count(),
    }
}

fn inserted_end(start: LspPosition, text: &str) -> LspPosition {
    let spans = line_spans(text);
    if spans.len() == 1 {
        return LspPosition {
            line: start.line,
            character: start.character + text.encode_utf16().count(),
        };
    }
    let (last_start, last_end) = spans[spans.len() - 1];
    LspPosition {
        line: start.line + spans.len() - 1,
        character: text[last_start..last_end].encode_utf16().count(),
    }
}

#[cfg(test)]
mod tests {
    use super::{ContentChange, Document, LspPosition, LspRange, position_to_byte};

    #[test]
    fn applies_cr_only_changes_at_utf16_positions() {
        let mut document = Document::new(
            "file:///a".to_owned(),
            "plaintext".to_owned(),
            1,
            "a😀 c\rnext".to_owned(),
        );
        document
            .apply_changes(
                2,
                &[ContentChange {
                    range: Some(LspRange {
                        start: LspPosition {
                            line: 0,
                            character: 3,
                        },
                        end: LspPosition {
                            line: 0,
                            character: 3,
                        },
                    }),
                    range_length: Some(0),
                    text: " b".to_owned(),
                }],
            )
            .expect("valid change");

        assert_eq!(document.lines(), ["a😀 b c", "next"]);
        assert_eq!(document.preferred_eol(), "\r");
    }

    #[test]
    fn rejects_stale_versions_without_changing_text() {
        let mut document = Document::new(
            "file:///a".to_owned(),
            "plaintext".to_owned(),
            2,
            "unchanged".to_owned(),
        );
        let error = document
            .apply_changes(
                2,
                &[ContentChange {
                    range: None,
                    range_length: None,
                    text: "changed".to_owned(),
                }],
            )
            .expect_err("stale version");

        assert_eq!(error.code, crate::CONTENT_MODIFIED);
        assert_eq!(document.lines(), ["unchanged"]);
    }

    #[test]
    fn maps_bmp_and_astral_utf16_positions_to_byte_offsets() {
        let text = "aé😀b";

        assert_eq!(
            position_to_byte(
                text,
                LspPosition {
                    line: 0,
                    character: 1
                }
            )
            .expect("before BMP character"),
            1
        );
        assert_eq!(
            position_to_byte(
                text,
                LspPosition {
                    line: 0,
                    character: 2
                }
            )
            .expect("before astral character"),
            3
        );
        assert_eq!(
            position_to_byte(
                text,
                LspPosition {
                    line: 0,
                    character: 4
                }
            )
            .expect("after astral character"),
            7
        );
    }

    #[test]
    fn accepts_exact_eol_and_rejects_past_eol_or_surrogate_splits() {
        let text = "a😀";

        assert_eq!(
            position_to_byte(
                text,
                LspPosition {
                    line: 0,
                    character: 3
                }
            )
            .expect("exact EOL"),
            text.len()
        );
        assert!(
            position_to_byte(
                text,
                LspPosition {
                    line: 0,
                    character: 4
                }
            )
            .is_err()
        );
        assert!(
            position_to_byte(
                text,
                LspPosition {
                    line: 0,
                    character: 2
                }
            )
            .is_err()
        );
    }

    #[test]
    fn tracks_only_single_zero_length_insertions_as_direct_changes() {
        let mut document = Document::new(
            "file:///a".to_owned(),
            "plaintext".to_owned(),
            1,
            "abc".to_owned(),
        );
        document
            .apply_changes(
                2,
                &[ContentChange {
                    range: Some(LspRange {
                        start: LspPosition {
                            line: 0,
                            character: 1,
                        },
                        end: LspPosition {
                            line: 0,
                            character: 2,
                        },
                    }),
                    range_length: Some(1),
                    text: " ".to_owned(),
                }],
            )
            .expect("replacement");

        let batch = document.last_change_batch.expect("change metadata");
        assert_eq!(batch.change_count, 1);
        assert!(batch.insertion.is_none());
    }
}
