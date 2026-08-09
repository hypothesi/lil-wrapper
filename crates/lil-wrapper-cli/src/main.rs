//! Command line interface for the Lil Wrapper text wrapping library.
//!
//! Wraps a text file to a configured column. The binary exists so that Zed
//! tasks can surface Lil Wrapper operations in the command palette via
//! `task: spawn`.

use lil_wrapper_core::{
    CustomMarkers, Edit, File, Position, Selection, Settings, WrapRequest, language_name_for_file,
    wrap,
};
use std::env;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

const DEFAULT_COLUMN: usize = 80;
const DEFAULT_TAB_WIDTH: usize = 4;
const USAGE: &str = "\
Usage: lil-wrapper-cli wrap <file> [--column N] [--tab-width N] [--write] [--help]

Wraps <file> to the configured column using the Lil Wrapper core library.

Options:
  --column N      Wrap at column N (default: 80)
  --tab-width N   Tab expansion width (default: 4)
  --write         Rewrite <file> in place instead of printing to stdout
  --help          Print this help and exit
";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lil-wrapper-cli: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let arguments: Vec<String> = env::args().skip(1).collect();
    let mut path: Option<PathBuf> = None;
    let mut column = DEFAULT_COLUMN;
    let mut tab_width = DEFAULT_TAB_WIDTH;
    let mut write = false;

    let mut index = 0;
    while index < arguments.len() {
        let argument = &arguments[index];
        match argument.as_str() {
            "--help" | "-h" => {
                print!("{USAGE}");
                return Ok(());
            }
            "--column" => {
                index += 1;
                column = arguments
                    .get(index)
                    .ok_or("--column requires a value")?
                    .parse()?;
            }
            "--tab-width" => {
                index += 1;
                tab_width = arguments
                    .get(index)
                    .ok_or("--tab-width requires a value")?
                    .parse()?;
            }
            "--write" => write = true,
            value if value.starts_with("--") => {
                return Err(format!("unknown option: {value}").into());
            }
            value => path = Some(PathBuf::from(value)),
        }
        index += 1;
    }

    let path = path.ok_or("missing <file> argument; run with --help for usage")?;
    let text = fs::read_to_string(&path)?;
    let lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let mut file = File {
        language: String::new(),
        path: path.to_string_lossy().into_owned(),
        custom_markers: CustomMarkers::default(),
    };
    let language = language_name_for_file(&file).unwrap_or("plaintext");
    language.clone_into(&mut file.language);
    let request = WrapRequest {
        file,
        settings: Settings {
            column,
            tab_width,
            double_sentence_spacing: false,
            reformat: false,
            whole_comment: true,
        },
        selections: vec![whole_document_selection(&lines)],
        lines,
    };
    let edit = wrap(&request);
    let output = apply(&request, &edit);

    if write {
        fs::write(&path, output.join("\n") + "\n")?;
    } else {
        println!("{}", output.join("\n"));
    }
    Ok(())
}

fn whole_document_selection(lines: &[String]) -> Selection {
    let last = lines.len().saturating_sub(1);
    let character = lines
        .get(last)
        .map_or(0, |line| line.encode_utf16().count());
    Selection {
        anchor: Position {
            line: 0,
            character: 0,
        },
        active: Position {
            line: last,
            character,
        },
    }
}

#[must_use]
fn apply(request: &WrapRequest, edit: &Edit) -> Vec<String> {
    if edit.is_empty() {
        return request.lines.clone();
    }
    let end = usize::try_from(edit.end_line).expect("nonempty edit has an end line");
    let mut output = request.lines[..edit.start_line].to_vec();
    output.extend(edit.lines.clone());
    output.extend_from_slice(&request.lines[end + 1..]);
    output
}
