mod support;

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};
use std::sync::Mutex;

use lil_wrapper_core::{
    ColumnState, CustomMarkers, DocState, File, Position, Selection, Settings, WrapRequest,
    language_name_for_file, languages, maybe_auto_wrap, str_width, wrap,
};
use serde_json::{Value, json};
use support::specs::{SpecCase, load_corpus};
use walkdir::WalkDir;

static DOTNET_LOCK: Mutex<()> = Mutex::new(());
const PINNED_REFERENCE_HASH: &str = "9c8a3e05764cdddff6606ecbdfbb0a33fb7d880a";

#[test]
fn pinned_reference_files_match_the_verified_upstream_tree() {
    let root = project_root();
    let vendor = env::var_os("REWRAP_REFERENCE_ROOT")
        .map_or_else(|| root.join("vendor/rewrap"), PathBuf::from);
    let mut paths = vec![vendor.join("LICENSE")];
    for directory in ["docs/specs", "vscode/src"] {
        paths.extend(
            WalkDir::new(vendor.join(directory))
                .into_iter()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().is_file())
                .map(walkdir::DirEntry::into_path)
                .filter(|path| !path.components().any(|part| part.as_os_str() == ".obj")),
        );
    }
    paths.extend(
        WalkDir::new(vendor.join("core"))
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .map(walkdir::DirEntry::into_path)
            .filter(|path| {
                matches!(
                    path.extension().and_then(|value| value.to_str()),
                    Some("fs" | "fsproj")
                )
            }),
    );
    paths.extend([
        vendor.join("vscode/package.json"),
        vendor.join("vscode/package-lock.json"),
    ]);
    paths.sort();

    let mut input = Vec::new();
    for path in paths {
        let relative = path.strip_prefix(&vendor).expect("vendored reference path");
        let bytes = fs::read(&path).expect("read vendored reference file");
        input.extend_from_slice(relative.to_string_lossy().as_bytes());
        input.push(0);
        input.extend_from_slice(
            &u64::try_from(bytes.len())
                .expect("file length fits u64")
                .to_be_bytes(),
        );
        input.extend_from_slice(&bytes);
    }

    let mut child = Command::new("git")
        .args(["hash-object", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("start git hash-object");
    child
        .stdin
        .take()
        .expect("git hash stdin")
        .write_all(&input)
        .expect("hash vendored reference bytes");
    let output = child.wait_with_output().expect("finish git hash-object");
    assert!(output.status.success(), "git hash-object failed");
    assert_eq!(
        String::from_utf8(output.stdout)
            .expect("ASCII git hash")
            .trim(),
        PINNED_REFERENCE_HASH
    );
}

#[test]
fn pinned_original_runner_passes_its_executable_specs() {
    let _lock = DOTNET_LOCK.lock().expect(".NET test lock");
    let root = project_root();
    let project = root.join("tests/reference-original/Rewrap.OriginalSpecs.fsproj");
    let working_directory = copy_original_specs_to_lowercase_path(&root);
    let output = Command::new(dotnet())
        .args([
            "run",
            "--project",
            project.to_str().expect("UTF-8 original test project path"),
            "--configuration",
            "Release",
            "--verbosity",
            "quiet",
        ])
        .current_dir(&working_directory)
        .output()
        .expect("run the pinned original Rewrap specs with .NET 8");
    fs::remove_dir_all(working_directory).expect("remove copied original spec corpus");
    let diagnostics = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.status.success(),
        "original runner failed:\n{diagnostics}"
    );
    assert!(
        diagnostics.contains("Passed: 470; Failed: 0; Errored: 0"),
        "unexpected original runner result:\n{diagnostics}"
    );
}

#[test]
fn rust_matches_the_original_core_for_every_reference_expectation() {
    let corpus = load_corpus().expect("valid reference corpus");
    let selected = corpus.cases.iter().filter(|case| case.only).count();
    let cases = corpus
        .cases
        .iter()
        .filter(|case| selected == 0 || case.only)
        .collect::<Vec<_>>();
    let requests = cases.iter().map(|case| oracle_request(case)).collect();
    let original = run_oracle(&Value::Array(requests));
    let original = original.as_array().expect("oracle response array");
    assert_eq!(original.len(), cases.len(), "oracle response count");

    let mut failures = Vec::new();
    for (case, original_edit) in cases.into_iter().zip(original) {
        let original_edit = normalize_empty_edit(original_edit.clone());
        let rust_edit = wrap(&WrapRequest {
            file: File {
                language: case.language.clone(),
                path: String::new(),
                custom_markers: CustomMarkers::default(),
            },
            settings: case.settings,
            selections: case.selections.clone(),
            lines: case.input.clone(),
        });
        let rust_edit = edit_json(&case.id, &rust_edit);
        let rust_edit = normalize_empty_edit(rust_edit);
        if rust_edit != original_edit {
            failures.push(format!(
                "{}\noriginal: {}\nrust:     {}",
                case.id, original_edit, rust_edit
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} original/Rust differentials failed (showing up to 20):\n\n{}",
        failures.len(),
        failures
            .into_iter()
            .take(20)
            .collect::<Vec<_>>()
            .join("\n\n")
    );
}

#[test]
fn rust_spec_parser_matches_the_original_parser_case_for_case() {
    let corpus = load_corpus().expect("valid reference corpus");
    let response = run_oracle(&json!([{"id": "corpus", "operation": "corpus"}]));
    let original = response
        .pointer("/0/value")
        .and_then(Value::as_array)
        .expect("original parser corpus response");
    assert_eq!(original.len(), corpus.cases.len(), "parsed case count");

    let failures = corpus
        .cases
        .iter()
        .zip(original)
        .filter_map(|(case, original_case)| {
            let rust_case = corpus_case_json(case);
            (rust_case != *original_case).then(|| {
                format!(
                    "{}\noriginal: {}\nrust:     {}",
                    case.id, original_case, rust_case
                )
            })
        })
        .collect::<Vec<_>>();
    assert!(
        failures.is_empty(),
        "{} original/Rust parser differences (showing up to 20):\n\n{}",
        failures.len(),
        failures
            .into_iter()
            .take(20)
            .collect::<Vec<_>>()
            .join("\n\n")
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn exported_core_contracts_match_the_original_runtime() {
    let mut requests = vec![json!({"id": "languages", "operation": "languages"})];
    let mut expected = vec![json!({"id": "languages", "value": languages()})];

    for (id, language, path) in [
        ("language-explicit", "markdown", "/tmp/notes.txt"),
        ("language-path", "plaintext", "/tmp/README.md"),
        ("language-special-path", "", r"C:\work\Dockerfile"),
        ("language-unknown", "unknown-language", "/tmp/README.md"),
    ] {
        let file = File {
            language: language.to_owned(),
            path: path.to_owned(),
            custom_markers: CustomMarkers::default(),
        };
        requests.push(json!({
            "id": id,
            "operation": "languageName",
            "language": language,
            "path": path,
        }));
        expected.push(json!({"id": id, "value": language_name_for_file(&file)}));
    }

    for (id, tab_width, text) in [
        ("width-ascii", 4, "abc"),
        ("width-tab-four", 4, "a\tb"),
        ("width-tab-two", 2, "a\tb"),
        ("width-control", 4, "\0\u{0001}"),
        ("width-cjk", 4, "界"),
        ("width-full", 4, "Ａ"),
        ("width-astral", 4, "😀"),
    ] {
        requests.push(json!({
            "id": id,
            "operation": "strWidth",
            "tabWidth": tab_width,
            "text": text,
        }));
        expected.push(json!({"id": id, "value": str_width(tab_width, text)}));
    }

    let column_path = "/tmp/lil-wrapper-reference-differential-columns";
    requests.push(json!({
        "id": "columns",
        "operation": "columnScenario",
        "path": column_path,
    }));
    let mut columns = ColumnState::default();
    let rulers = [72, 88];
    let initial = columns.wrapping_column(column_path, &rulers);
    let state = DocState {
        file_path: column_path.to_owned(),
        version: 1,
        selections: vec![cursor(0, 0)],
    };
    let before_save = columns.maybe_change_wrapping_column(&state, &rulers);
    columns.save_document(state.clone());
    let cycled = columns.maybe_change_wrapping_column(&state, &rulers);
    columns.save_document(state.clone());
    let moved = DocState {
        selections: vec![cursor(0, 1)],
        ..state
    };
    let after_move = columns.maybe_change_wrapping_column(&moved, &rulers);
    let changed_rulers = columns.wrapping_column(column_path, &[100, 120]);
    expected.push(json!({
        "id": "columns",
        "value": [initial, before_save, cycled, after_move, changed_rulers],
    }));

    for (id, auto_request, new_text, position) in [
        (
            "auto-wrap-space",
            request("plaintext", 8, &["one two three "]),
            " ",
            Position {
                line: 0,
                character: 13,
            },
        ),
        (
            "auto-wrap-tab",
            request("plaintext", 8, &["one two three\t"]),
            "\t",
            Position {
                line: 0,
                character: 13,
            },
        ),
        (
            "auto-wrap-empty",
            request("plaintext", 8, &["one two three"]),
            "",
            Position {
                line: 0,
                character: 13,
            },
        ),
        (
            "auto-wrap-non-whitespace",
            request("plaintext", 8, &["one two threex"]),
            "x",
            Position {
                line: 0,
                character: 13,
            },
        ),
        (
            "auto-wrap-two-spaces",
            request("plaintext", 8, &["one two three  "]),
            "  ",
            Position {
                line: 0,
                character: 13,
            },
        ),
        (
            "auto-wrap-disabled-column",
            request("plaintext", 0, &["one two three "]),
            " ",
            Position {
                line: 0,
                character: 13,
            },
        ),
        (
            "auto-wrap-below-column",
            request("plaintext", 80, &["one two three "]),
            " ",
            Position {
                line: 0,
                character: 13,
            },
        ),
        (
            "auto-wrap-newline-indent",
            request("plaintext", 8, &["one two three", "  "]),
            "\n  ",
            Position {
                line: 0,
                character: 13,
            },
        ),
        (
            "auto-wrap-crlf-indent",
            request("plaintext", 8, &["one two three", "  "]),
            "\r\n  ",
            Position {
                line: 0,
                character: 13,
            },
        ),
        (
            "auto-wrap-unicode-space",
            request("plaintext", 8, &["one two three\u{a0}"]),
            "\u{a0}",
            Position {
                line: 0,
                character: 13,
            },
        ),
    ] {
        requests.push(json!({
            "id": id,
            "operation": "autoWrap",
            "language": "plaintext",
            "settings": settings_json(auto_request.settings),
            "lines": auto_request.lines,
            "newText": new_text,
            "position": {"line": position.line, "character": position.character},
        }));
        expected.push(edit_json(
            id,
            &maybe_auto_wrap(&auto_request, new_text, position),
        ));
    }

    push_custom_marker_contracts(&mut requests, &mut expected);
    push_source_parser_contracts(&mut requests, &mut expected);
    for (id, parser_request) in [
        (
            "autohotkey-block-comment",
            request("autohotkey", 14, &["/* one two three four */"]),
        ),
        (
            "go-godoc-block-comment",
            request("go", 14, &["/** one two three four */"]),
        ),
        (
            "julia-line-before-triple-quote",
            request("julia", 14, &["# \"\"\" one two three four \"\"\""]),
        ),
        (
            "graphql-line-before-triple-quote",
            request("graphql", 14, &["# \"\"\" one two three four \"\"\""]),
        ),
        (
            "python-line-before-triple-quote",
            request("python", 14, &["# \"\"\" one two three four \"\"\""]),
        ),
        (
            "prolog-javadoc-before-c-block",
            request("prolog", 14, &["/** @example one two three four */"]),
        ),
        (
            "octave-line-consumes-first-character",
            request("octave", 14, &["%one two three four five"]),
        ),
        (
            "pascal-spaced-directive-is-comment",
            request("pascal", 14, &["{ $one two three four five }"]),
        ),
    ] {
        requests.push(request_json(id, &parser_request));
        expected.push(edit_json(id, &wrap(&parser_request)));
    }

    let original = run_oracle(&Value::Array(requests));
    assert_eq!(original, Value::Array(expected));
}

fn push_custom_marker_contracts(requests: &mut Vec<Value>, expected: &mut Vec<Value>) {
    for (id, custom_request) in [
        {
            let mut request = request("custom-line", 12, &["@@ one two three four"]);
            "@@".clone_into(&mut request.file.custom_markers.line);
            ("custom-line", request)
        },
        {
            let mut request = request("custom-block", 12, &["<# one two three four #>"]);
            request.file.custom_markers.block = ("<#".to_owned(), "#>".to_owned());
            ("custom-block", request)
        },
    ] {
        requests.push(request_json(id, &custom_request));
        expected.push(edit_json(id, &wrap(&custom_request)));
    }
}

#[allow(clippy::too_many_lines)]
fn push_source_parser_contracts(requests: &mut Vec<Value>, expected: &mut Vec<Value>) {
    for (id, language, source) in [
        ("autohotkey-line", "autohotkey", "; one two three four five"),
        (
            "autohotkey-block",
            "autohotkey",
            "/* one two three four five */",
        ),
        ("basic-xmldoc", "basic", "''' one two three four five"),
        ("basic-line", "basic", "' one two three four five"),
        ("batch-rem", "batch file", "rem one two three four five"),
        ("batch-colons", "batch file", ":: one two three four five"),
        ("c-xmldoc", "c/c++", "/// one two three four five"),
        ("c-javadoc-line", "c/c++", "//! one two three four five"),
        ("c-line", "c/c++", "// one two three four five"),
        ("c-javadoc-block", "c/c++", "/** one two three four five */"),
        ("c-block", "c/c++", "/* one two three four five */"),
        ("csharp-xmldoc", "c#", "/// one two three four five"),
        ("csharp-line", "c#", "// one two three four five"),
        (
            "csharp-javadoc-block",
            "c#",
            "/** one two three four five */",
        ),
        ("csharp-block", "c#", "/* one two three four five */"),
        ("lisp-line", "common lisp", ";;; one two three four five"),
        ("lisp-block", "common lisp", "#| one two three four five |#"),
        (
            "coffee-javadoc",
            "coffeescript",
            "###* one two three four five ###",
        ),
        (
            "coffee-block",
            "coffeescript",
            "### one two three four five ###",
        ),
        ("coffee-line", "coffeescript", "# one two three four five"),
        ("config-line", "configuration", "# one two three four five"),
        ("java-javadoc", "java", "/** one two three four five */"),
        ("java-block", "java", "/* one two three four five */"),
        ("java-doc-line", "java", "/// one two three four five"),
        ("java-bang-line", "java", "//! one two three four five"),
        ("java-line", "java", "// one two three four five"),
        ("fidl-doc-line", "fidl", "/// one two three four five"),
        ("fidl-line", "fidl", "// one two three four five"),
        ("d-doc-line", "d", "/// one two three four five"),
        ("d-line", "d", "// one two three four five"),
        ("d-javadoc", "d", "/** one two three four five */"),
        ("d-nested-doc", "d", "/++ one two three four five +/"),
        ("d-c-block", "d", "/* one two three four five */"),
        ("d-nested-block", "d", "/+ one two three four five +/"),
        ("dart-doc-line", "dart", "/// one two three four five"),
        ("dart-line", "dart", "// one two three four five"),
        ("dart-doc-block", "dart", "/** one two three four five */"),
        ("dart-block", "dart", "/* one two three four five */"),
        ("elixir-line", "elixir", "# one two three four five"),
        (
            "elixir-doc-block",
            "elixir",
            "@doc \"\"\" one two three four five \"\"\"",
        ),
        ("elm-line", "elm", "-- one two three four five"),
        ("elm-block", "elm", "{-| one two three four five -}"),
        ("haskell-line", "haskell", "-- one two three four five"),
        (
            "haskell-block",
            "haskell",
            "{- | one two three four five -}",
        ),
        (
            "purescript-doc-line",
            "purescript",
            "-- | one two three four five",
        ),
        (
            "purescript-line",
            "purescript",
            "-- one two three four five",
        ),
        (
            "purescript-block",
            "purescript",
            "{- | one two three four five -}",
        ),
        ("fsharp-xmldoc", "f#", "/// one two three four five"),
        ("fsharp-line", "f#", "// one two three four five"),
        ("fsharp-block", "f#", "(* one two three four five *)"),
        ("go-line", "go", "// one two three four five"),
        ("go-doc-block", "go", "/** one two three four five */"),
        ("go-block", "go", "/* one two three four five */"),
        ("graphql-line", "graphql", "# one two three four five"),
        (
            "graphql-block",
            "graphql",
            "\"\"\" one two three four five \"\"\"",
        ),
        (
            "handlebars-long",
            "handlebars",
            "{{!-- one two three four five --}}",
        ),
        (
            "handlebars-short",
            "handlebars",
            "{{! one two three four five }}",
        ),
        (
            "handlebars-html",
            "handlebars",
            "<!-- one two three four five -->",
        ),
        ("hcl-javadoc", "hcl", "/** one two three four five */"),
        ("hcl-block", "hcl", "/* one two three four five */"),
        ("hcl-doc-line", "hcl", "/// one two three four five"),
        ("hcl-line", "hcl", "// one two three four five"),
        ("hcl-hash", "hcl", "# one two three four five"),
        ("ini-line", "ini", "; one two three four five"),
        ("j-line", "j", "NB. one two three four five"),
        ("julia-block", "julia", "#= one two three four five =#"),
        ("julia-line", "julia", "# one two three four five"),
        (
            "julia-triple",
            "julia",
            "\"\"\" one two three four five \"\"\"",
        ),
        ("lean-line", "lean", "-- one two three four five"),
        ("lean-block", "lean", "/-! one two three four five -/"),
        ("lua-block", "lua", "--[=[ one two three four five ]=]"),
        ("lua-line", "lua", "-- one two three four five"),
        ("matlab-line", "matlab", "% one two three four five"),
        ("matlab-block", "matlab", "%{ one two three four five %}"),
        (
            "octave-hash-block",
            "octave",
            "#{ one two three four five #}",
        ),
        (
            "octave-percent-block",
            "octave",
            "%{ one two three four five %}",
        ),
        ("octave-doc-line", "octave", "## one two three four five"),
        ("octave-line", "octave", "%one two three four five"),
        (
            "pascal-star-block",
            "pascal",
            "(* one two three four five *)",
        ),
        (
            "pascal-brace-block",
            "pascal",
            "{ one two three four five }",
        ),
        ("pascal-doc-line", "pascal", "/// one two three four five"),
        ("php-javadoc", "php", "/** one two three four five */"),
        ("php-block", "php", "/* one two three four five */"),
        ("php-line", "php", "# one two three four five"),
        ("powershell-line", "powershell", "# one two three four five"),
        (
            "powershell-block",
            "powershell",
            "<# one two three four five #>",
        ),
        ("prolog-javadoc", "prolog", "/** one two three four five */"),
        ("prolog-block", "prolog", "/* one two three four five */"),
        ("prolog-line", "prolog", "%! one two three four five"),
        ("protobuf-line", "protobuf", "// one two three four five"),
        ("r-doc-line", "r", "#' one two three four five"),
        ("r-line", "r", "# one two three four five"),
        ("python-line", "python", "# one two three four five"),
        (
            "python-double-block",
            "python",
            "\"\"\" one two three four five \"\"\"",
        ),
        (
            "python-single-block",
            "python",
            "''' one two three four five '''",
        ),
        ("ruby-line", "ruby", "# one two three four five"),
        ("ruby-block", "ruby", "=begin one two three four five =end"),
        ("rust-doc-line", "rust", "/// one two three four five"),
        ("rust-bang-line", "rust", "//! one two three four five"),
        ("rust-line", "rust", "// one two three four five"),
        ("shell-line", "shell script", "# one two three four five"),
        ("sql-line", "sql", "-- one two three four five"),
        ("sql-block", "sql", "/* one two three four five */"),
        ("yaml-line", "yaml", "### one two three four five"),
    ] {
        let parser_request = request(language, 14, &[source]);
        assert!(
            !wrap(&parser_request).is_empty(),
            "inactive parser fixture: {id}"
        );
        requests.push(request_json(id, &parser_request));
        expected.push(edit_json(id, &wrap(&parser_request)));
    }
}

fn normalize_empty_edit(mut edit: Value) -> Value {
    let start = edit.get("startLine").and_then(Value::as_i64);
    let end = edit.get("endLine").and_then(Value::as_i64);
    let lines_empty = edit
        .get("lines")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty);
    if lines_empty && start.zip(end).is_some_and(|(start, end)| end < start) {
        edit["startLine"] = json!(0);
        edit["endLine"] = json!(-1);
    }
    edit
}

fn oracle_request(case: &SpecCase) -> Value {
    json!({
        "id": case.id,
        "language": case.language,
        "path": "",
        "customMarkers": {"line": "", "block": ["", ""]},
        "settings": {
            "column": case.settings.column,
            "tabWidth": case.settings.tab_width,
            "doubleSentenceSpacing": case.settings.double_sentence_spacing,
            "reformat": case.settings.reformat,
            "wholeComment": case.settings.whole_comment,
        },
        "selections": case.selections.iter().map(|selection| json!({
            "anchor": {
                "line": selection.anchor.line,
                "character": selection.anchor.character,
            },
            "active": {
                "line": selection.active.line,
                "character": selection.active.character,
            },
        })).collect::<Vec<_>>(),
        "lines": case.input,
    })
}

fn corpus_case_json(case: &SpecCase) -> Value {
    json!({
        "id": case.id,
        "language": case.language,
        "settings": settings_json(case.settings),
        "input": case.input,
        "expected": case.expected,
        "selections": case.selections.iter().map(selection_json).collect::<Vec<_>>(),
        "only": case.only,
        "reformatAlternative": case.reformat_alternative,
    })
}

fn request(language: &str, column: usize, lines: &[&str]) -> WrapRequest {
    WrapRequest {
        file: File {
            language: language.to_owned(),
            path: String::new(),
            custom_markers: CustomMarkers::default(),
        },
        settings: Settings {
            column,
            tab_width: 4,
            double_sentence_spacing: false,
            reformat: false,
            whole_comment: true,
        },
        selections: Vec::new(),
        lines: lines.iter().map(|line| (*line).to_owned()).collect(),
    }
}

fn cursor(line: usize, character: usize) -> Selection {
    Selection {
        anchor: Position { line, character },
        active: Position { line, character },
    }
}

fn request_json(id: &str, request: &WrapRequest) -> Value {
    json!({
        "id": id,
        "language": request.file.language,
        "path": request.file.path,
        "customMarkers": {
            "line": request.file.custom_markers.line,
            "block": [
                request.file.custom_markers.block.0,
                request.file.custom_markers.block.1,
            ],
        },
        "settings": settings_json(request.settings),
        "selections": request.selections.iter().map(selection_json).collect::<Vec<_>>(),
        "lines": request.lines,
    })
}

fn settings_json(settings: Settings) -> Value {
    json!({
        "column": settings.column,
        "tabWidth": settings.tab_width,
        "doubleSentenceSpacing": settings.double_sentence_spacing,
        "reformat": settings.reformat,
        "wholeComment": settings.whole_comment,
    })
}

fn edit_json(id: &str, edit: &lil_wrapper_core::Edit) -> Value {
    json!({
        "id": id,
        "startLine": edit.start_line,
        "endLine": edit.end_line,
        "lines": edit.lines,
        "selections": edit.selections.iter().map(selection_json).collect::<Vec<_>>(),
        "isEmpty": edit.is_empty(),
    })
}

fn selection_json(selection: &Selection) -> Value {
    json!({
        "anchor": {
            "line": selection.anchor.line,
            "character": selection.anchor.character,
        },
        "active": {
            "line": selection.active.line,
            "character": selection.active.character,
        },
    })
}

fn run_oracle(requests: &Value) -> Value {
    let _lock = DOTNET_LOCK.lock().expect(".NET test lock");
    let root = project_root();
    let project = root.join("tests/reference-oracle/Rewrap.Oracle.fsproj");
    let mut child = Command::new(dotnet())
        .args([
            "run",
            "--project",
            project.to_str().expect("UTF-8 oracle project path"),
            "--configuration",
            "Release",
            "--verbosity",
            "quiet",
        ])
        .current_dir(root.join("vendor/rewrap"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start the original Rewrap oracle with .NET 8");
    child
        .stdin
        .take()
        .expect("oracle stdin")
        .write_all(requests.to_string().as_bytes())
        .expect("write oracle requests");
    let output = child.wait_with_output().expect("wait for original oracle");
    assert!(
        output.status.success(),
        "original oracle failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "original oracle returned invalid JSON: {error}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn copy_original_specs_to_lowercase_path(root: &Path) -> PathBuf {
    let source = root.join("vendor/rewrap/docs/specs");
    let temp_root = Path::new("/tmp");
    let temp_root = temp_root
        .is_dir()
        .then_some(temp_root)
        .map_or_else(env::temp_dir, Path::to_path_buf);
    let working_directory = temp_root.join(format!("rewrap-original-specs-{}", process::id()));
    let destination = working_directory.join("docs/specs");
    let _ = fs::remove_dir_all(&working_directory);

    for entry in WalkDir::new(&source) {
        let entry = entry.expect("read original spec corpus");
        let relative = entry
            .path()
            .strip_prefix(&source)
            .expect("spec path below corpus root")
            .to_string_lossy()
            .to_ascii_lowercase();
        let target = destination.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(target).expect("create copied spec directory");
        } else {
            fs::copy(entry.path(), target).expect("copy original spec file");
        }
    }

    working_directory
}

fn dotnet() -> PathBuf {
    if let Some(path) = env::var_os("DOTNET") {
        return path.into();
    }
    for homebrew in [
        "/opt/homebrew/opt/dotnet@8/bin/dotnet",
        "/usr/local/opt/dotnet@8/bin/dotnet",
    ] {
        let homebrew = PathBuf::from(homebrew);
        if homebrew.is_file() {
            return homebrew;
        }
    }
    PathBuf::from("dotnet")
}
