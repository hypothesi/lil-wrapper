use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use lil_wrapper_core::{Position, Selection, Settings};
use regex::Regex;
use walkdir::WalkDir;

const SPEC_ROOT: &str = "../../vendor/rewrap/docs/specs";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpecCase {
    pub id: String,
    pub file: PathBuf,
    pub language: String,
    pub settings: Settings,
    pub input: Vec<String>,
    pub expected: Vec<String>,
    pub selections: Vec<Selection>,
    pub only: bool,
    pub reformat_alternative: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Corpus {
    pub files: Vec<PathBuf>,
    pub cases: Vec<SpecCase>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SpecErrorKind {
    InvalidSelection,
    InvalidSetting(String),
    InvalidUtf16Boundary,
    InvalidWrappingColumn,
    Io(String),
    NoOutput,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpecError {
    pub file: PathBuf,
    pub sample: usize,
    pub kind: SpecErrorKind,
}

impl fmt::Display for SpecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} sample {}: {:?}",
            self.file.display(),
            self.sample,
            self.kind
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TestSettings {
    language: String,
    tab_width: usize,
    double_sentence_spacing: bool,
    reformat: bool,
    whole_comment: bool,
}

impl Default for TestSettings {
    fn default() -> Self {
        Self {
            language: "plaintext".to_owned(),
            tab_width: 4,
            double_sentence_spacing: false,
            reformat: false,
            whole_comment: true,
        }
    }
}

pub fn load_corpus() -> Result<Corpus, Vec<SpecError>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(SPEC_ROOT);
    let mut files = WalkDir::new(&root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(walkdir::DirEntry::into_path)
        .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
        .collect::<Vec<_>>();
    files.sort();

    let mut cases = Vec::new();
    let mut errors = Vec::new();
    for file in &files {
        match parse_file(&root, file) {
            Ok(mut parsed) => cases.append(&mut parsed),
            Err(mut file_errors) => errors.append(&mut file_errors),
        }
    }

    if errors.is_empty() {
        Ok(Corpus { files, cases })
    } else {
        Err(errors)
    }
}

fn parse_file(root: &Path, file: &Path) -> Result<Vec<SpecCase>, Vec<SpecError>> {
    let text = fs::read_to_string(file).map_err(|error| {
        vec![SpecError {
            file: file.to_path_buf(),
            sample: 0,
            kind: SpecErrorKind::Io(error.to_string()),
        }]
    })?;
    let relative = file.strip_prefix(root).unwrap_or(file);
    let mut cases = Vec::new();
    let mut errors = Vec::new();
    let mut settings = TestSettings::default();
    let mut sample_lines = Some(Vec::new());
    let mut sample_number = 0;

    for line in text.lines() {
        if line.starts_with("    ") {
            if let Some(lines) = &mut sample_lines {
                lines.push(line.to_owned());
            }
        } else if line.starts_with("> ") {
            match parse_settings(line) {
                Ok(parsed) => settings = parsed,
                Err(kind) => errors.push(SpecError {
                    file: relative.to_path_buf(),
                    sample: sample_number,
                    kind,
                }),
            }
            sample_lines = None;
        } else {
            if let Some(lines) = sample_lines.take().filter(|lines| !lines.is_empty()) {
                sample_number += 1;
                match parse_sample(relative, sample_number, &settings, lines) {
                    Ok(mut parsed) => cases.append(&mut parsed),
                    Err(error) => errors.push(error),
                }
            }
            sample_lines = line.is_empty().then(Vec::new);
        }
    }

    if let Some(lines) = sample_lines.filter(|lines| !lines.is_empty()) {
        sample_number += 1;
        match parse_sample(relative, sample_number, &settings, lines) {
            Ok(mut parsed) => cases.append(&mut parsed),
            Err(error) => errors.push(error),
        }
    }

    if errors.is_empty() {
        Ok(cases)
    } else {
        Err(errors)
    }
}

fn parse_settings(line: &str) -> Result<TestSettings, SpecErrorKind> {
    let mut settings = TestSettings::default();
    for pair in line[1..].split(',') {
        let mut parts = pair.split(':').map(str::trim);
        let key = parts.next().unwrap_or_default();
        let value = parts
            .next()
            .ok_or_else(|| SpecErrorKind::InvalidSetting(format!("missing value for {key}")))?;
        match key {
            "language" => value.trim_matches('"').clone_into(&mut settings.language),
            "tabWidth" => {
                settings.tab_width = value.parse().map_err(|_| {
                    SpecErrorKind::InvalidSetting(format!("invalid tabWidth: {value}"))
                })?;
            }
            "doubleSentenceSpacing" => {
                settings.double_sentence_spacing = parse_bool(key, value)?;
            }
            "reformat" => settings.reformat = parse_bool(key, value)?,
            "wholeComment" => settings.whole_comment = parse_bool(key, value)?,
            _ => {}
        }
    }
    Ok(settings)
}

fn parse_bool(key: &str, value: &str) -> Result<bool, SpecErrorKind> {
    match value.to_ascii_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(SpecErrorKind::InvalidSetting(format!(
            "invalid {key}: {value}"
        ))),
    }
}

fn parse_sample(
    file: &Path,
    sample: usize,
    settings: &TestSettings,
    mut lines: Vec<String>,
) -> Result<Vec<SpecCase>, SpecError> {
    let mut only = false;
    for line in &mut lines {
        if line.ends_with("<only>") {
            *line = line.replace("<only>", "");
            only = true;
        }
    }

    let (input_lines, output_lines) = split_lines(" -> ", &lines).map_err(|kind| SpecError {
        file: file.to_path_buf(),
        sample,
        kind,
    })?;
    let Some(output_lines) = output_lines else {
        return Err(SpecError {
            file: file.to_path_buf(),
            sample,
            kind: SpecErrorKind::NoOutput,
        });
    };
    let (expected_lines, reformat_lines) =
        split_lines("-or-", &output_lines).map_err(|kind| SpecError {
            file: file.to_path_buf(),
            sample,
            kind,
        })?;

    let mut sections = Vec::new();
    if let Some(reformat) = &reformat_lines {
        sections.push(reformat.as_slice());
    }
    sections.push(input_lines.as_slice());
    sections.push(expected_lines.as_slice());
    let wrapping_columns = sections
        .into_iter()
        .map(wrapping_column)
        .collect::<Vec<_>>();
    let Some(Some(column)) = wrapping_columns.first().copied() else {
        return Err(SpecError {
            file: file.to_path_buf(),
            sample,
            kind: SpecErrorKind::InvalidWrappingColumn,
        });
    };
    if wrapping_columns
        .iter()
        .any(|candidate| *candidate != Some(column))
    {
        return Err(SpecError {
            file: file.to_path_buf(),
            sample,
            kind: SpecErrorKind::InvalidWrappingColumn,
        });
    }

    let selections = selections(&input_lines).map_err(|kind| SpecError {
        file: file.to_path_buf(),
        sample,
        kind,
    })?;
    let input = clean_up(input_lines);
    let expected = clean_up(expected_lines);
    let base_id = format!("{}#{sample}", file.display());
    let base_settings = Settings {
        column,
        tab_width: settings.tab_width,
        double_sentence_spacing: settings.double_sentence_spacing,
        reformat: settings.reformat,
        whole_comment: settings.whole_comment,
    };
    let mut cases = vec![SpecCase {
        id: base_id.clone(),
        file: file.to_path_buf(),
        language: settings.language.clone(),
        settings: base_settings,
        input: input.clone(),
        expected,
        selections: selections.clone(),
        only,
        reformat_alternative: false,
    }];

    if let Some(reformat) = reformat_lines {
        cases.push(SpecCase {
            id: format!("{base_id}:reformat"),
            file: file.to_path_buf(),
            language: settings.language.clone(),
            settings: Settings {
                reformat: true,
                ..base_settings
            },
            input,
            expected: clean_up(reformat),
            selections,
            only,
            reformat_alternative: true,
        });
    }

    Ok(cases)
}

fn split_lines(
    marker: &str,
    lines: &[String],
) -> Result<(Vec<String>, Option<Vec<String>>), SpecErrorKind> {
    let split_point = lines
        .iter()
        .map(|line| width_before(marker, line))
        .max()
        .unwrap_or(-1);
    if split_point < 0 {
        return Ok((remove_indent(lines.to_vec())?, None));
    }

    let split_width = usize::try_from(split_point).expect("nonnegative split point")
        + marker.encode_utf16().count();
    let mut left = Vec::with_capacity(lines.len());
    let mut right = Vec::with_capacity(lines.len());
    for line in lines {
        let (before, after) = split_at_width(split_width, line)?;
        left.push(before);
        right.push(after);
    }
    Ok((remove_indent(left)?, Some(remove_indent(right)?)))
}

fn remove_indent(lines: Vec<String>) -> Result<Vec<String>, SpecErrorKind> {
    let indent = lines
        .iter()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            (!trimmed.is_empty())
                .then(|| line.encode_utf16().count() - trimmed.encode_utf16().count())
        })
        .min()
        .unwrap_or(0);
    lines
        .into_iter()
        .map(|line| substring_from_utf16(&line, indent.min(line.encode_utf16().count())))
        .collect()
}

fn wrapping_column(lines: &[String]) -> Option<usize> {
    let positions = lines
        .iter()
        .map(|line| width_before("¦", line))
        .filter(|position| *position >= 0)
        .map(|position| usize::try_from(position).expect("nonnegative marker position"))
        .collect::<Vec<_>>();
    positions
        .first()
        .copied()
        .filter(|first| positions.iter().all(|position| position == first))
}

fn selections(lines: &[String]) -> Result<Vec<Selection>, SpecErrorKind> {
    #[derive(Clone, Copy, Eq, PartialEq)]
    enum Kind {
        Active,
        Anchor,
    }

    let marker = Regex::new("[«»]").expect("static selection regex");
    let mut result = Vec::new();
    let mut pending = None;
    for (line_number, source) in lines.iter().enumerate() {
        let mut line = source.clone();
        while let Some(found) = marker.find(&line) {
            let character = line[..found.start()].encode_utf16().count();
            let kind = if found.as_str() == "«" {
                Kind::Anchor
            } else {
                Kind::Active
            };
            line.replace_range(found.range(), "");
            let position = Position {
                line: line_number,
                character,
            };
            match (pending, kind) {
                (None, _) => pending = Some((kind, position)),
                (Some((Kind::Anchor, anchor)), Kind::Active) => {
                    result.push(Selection {
                        anchor,
                        active: position,
                    });
                    pending = None;
                }
                (Some((Kind::Active, active)), Kind::Anchor) => {
                    result.push(Selection {
                        anchor: position,
                        active,
                    });
                    pending = None;
                }
                _ => return Err(SpecErrorKind::InvalidSelection),
            }
        }
    }
    if pending.is_some() {
        Err(SpecErrorKind::InvalidSelection)
    } else {
        Ok(result)
    }
}

fn clean_up(lines: Vec<String>) -> Vec<String> {
    let tabs = Regex::new("-*→").expect("static tab regex");
    let mut result = lines
        .into_iter()
        .map(|mut line| {
            let trailing_start = line.trim_end().len();
            if line[..trailing_start].ends_with(" ->") {
                let marker_start = trailing_start - " ->".len();
                line.replace_range(marker_start..trailing_start, "   ");
            }
            line = line.replace("-or-", "    ");
            line = line.replace('¦', " ");
            line = line.replace(['«', '»'], "");
            line = line.trim_end().to_owned();
            line = line.replace('·', " ");
            tabs.replace_all(&line, "\t").into_owned()
        })
        .collect::<Vec<_>>();
    while result.last().is_some_and(String::is_empty) {
        result.pop();
    }
    result
}

fn width_before(marker: &str, line: &str) -> isize {
    line.find(marker).map_or(-1, |position| {
        isize::try_from(string_width(1, &line[..position])).expect("line width fits in isize")
    })
}

fn split_at_width(column: usize, value: &str) -> Result<(String, String), SpecErrorKind> {
    let units = value.encode_utf16().collect::<Vec<_>>();
    let mut index = 0;
    let mut width = 0;
    let split = loop {
        if index > units.len() || width > column {
            break index.saturating_sub(1);
        }
        if index >= units.len() {
            break index;
        }
        width += code_unit_width(1, width, units[index]);
        index += 1;
    };
    let byte = byte_index_for_utf16(value, split)?;
    Ok((value[..byte].to_owned(), value[byte..].to_owned()))
}

fn substring_from_utf16(value: &str, start: usize) -> Result<String, SpecErrorKind> {
    let byte = byte_index_for_utf16(value, start)?;
    Ok(value[byte..].to_owned())
}

fn byte_index_for_utf16(value: &str, target: usize) -> Result<usize, SpecErrorKind> {
    if target == value.encode_utf16().count() {
        return Ok(value.len());
    }
    let mut utf16 = 0;
    for (byte, character) in value.char_indices() {
        if utf16 == target {
            return Ok(byte);
        }
        utf16 += character.len_utf16();
        if utf16 > target {
            return Err(SpecErrorKind::InvalidUtf16Boundary);
        }
    }
    Err(SpecErrorKind::InvalidUtf16Boundary)
}

fn string_width(tab_width: usize, value: &str) -> usize {
    let tab_width = tab_width.max(1);
    value.encode_utf16().fold(0, |column, unit| {
        column + code_unit_width(tab_width, column, unit)
    })
}

fn code_unit_width(tab_width: usize, column: usize, unit: u16) -> usize {
    if unit == 0x0009 {
        tab_width - column % tab_width
    } else if (0x0001..=0x001f).contains(&unit) {
        0
    } else if (0x2e80..=0xd7af).contains(&unit)
        || (0xf900..=0xfaff).contains(&unit)
        || (0xff01..=0xff5e).contains(&unit)
    {
        2
    } else {
        1
    }
}
