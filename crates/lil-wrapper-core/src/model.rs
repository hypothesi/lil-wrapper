#[derive(Clone, Debug)]
pub(crate) struct ParsedLine {
    pub prefix: String,
    pub content: String,
    pub protected_spaces: bool,
}

impl ParsedLine {
    pub fn new(prefix: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
            content: content.into(),
            protected_spaces: false,
        }
    }

    pub fn original(&self) -> String {
        let content = if self.protected_spaces {
            restore_protected_spaces(&self.content)
        } else {
            self.content.clone()
        };
        format!("{}{content}", self.prefix)
    }
}

pub(crate) fn escape_literal_nuls(value: &str) -> String {
    value.replace('\0', "\0\0")
}

pub(crate) fn protect_spaces_in_ranges(value: &str, ranges: &[std::ops::Range<usize>]) -> String {
    if ranges.is_empty() {
        return value.to_owned();
    }
    let mut output = String::with_capacity(value.len());
    let mut end = 0;
    for range in ranges {
        output.push_str(&escape_literal_nuls(&value[end..range.start]));
        for character in value[range.clone()].chars() {
            match character {
                '\0' => output.push_str("\0\0"),
                ' ' => output.push_str("\0s"),
                _ => output.push(character),
            }
        }
        end = range.end;
    }
    output.push_str(&escape_literal_nuls(&value[end..]));
    output
}

pub(crate) fn restore_protected_spaces(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\0' {
            match characters.peek() {
                Some('\0') => {
                    characters.next();
                    output.push('\0');
                }
                Some('s') => {
                    characters.next();
                    output.push(' ');
                }
                _ => output.push('\0'),
            }
        } else {
            output.push(character);
        }
    }
    output
}

#[derive(Clone, Debug)]
pub(crate) enum BlockKind {
    Wrap { default_tail: Option<String> },
    NoWrap,
}

#[derive(Clone, Debug)]
pub(crate) struct Block {
    pub start: usize,
    pub lines: Vec<ParsedLine>,
    pub kind: BlockKind,
    pub comment: Option<usize>,
}

impl Block {
    pub fn end(&self) -> usize {
        self.start + self.lines.len()
    }

    pub fn no_wrap(start: usize, lines: Vec<ParsedLine>) -> Self {
        Self {
            start,
            lines,
            kind: BlockKind::NoWrap,
            comment: None,
        }
    }

    pub fn wrap(start: usize, lines: Vec<ParsedLine>, default_tail: Option<String>) -> Self {
        Self {
            start,
            lines,
            kind: BlockKind::Wrap { default_tail },
            comment: None,
        }
    }
}
