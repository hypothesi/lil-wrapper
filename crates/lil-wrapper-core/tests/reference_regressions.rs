use std::panic::{AssertUnwindSafe, catch_unwind};

use lil_wrapper_core::{
    ColumnState, CustomMarkers, DocState, File, Position, Selection, Settings, WrapRequest,
    language_name_for_file, languages, maybe_auto_wrap, str_width, wrap,
};

fn settings(column: usize) -> Settings {
    Settings {
        column,
        tab_width: 4,
        double_sentence_spacing: false,
        reformat: false,
        whole_comment: true,
    }
}

fn request(language: &str, path: &str, column: usize, lines: &[&str]) -> WrapRequest {
    WrapRequest {
        file: File {
            language: language.to_owned(),
            path: path.to_owned(),
            custom_markers: CustomMarkers::default(),
        },
        settings: settings(column),
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

fn apply(request: &WrapRequest) -> Vec<String> {
    let edit = wrap(request);
    if edit.is_empty() {
        return request.lines.clone();
    }
    let end = usize::try_from(edit.end_line).expect("nonempty edit end");
    let mut output = request.lines[..edit.start_line].to_vec();
    output.extend(edit.lines);
    output.extend_from_slice(&request.lines[end + 1..]);
    output
}

#[test]
fn javadoc_inline_tags_never_leak_protected_space_sentinels() {
    let input = "/** <p>{@link Foo} */";
    assert_eq!(
        apply(&request("java", "", str_width(4, input), &[input])),
        [input]
    );

    let with_nul = "/** <p>{@link Foo\0Bar} */";
    assert_eq!(apply(&request("java", "", 80, &[with_nul])), [with_nul]);
}

#[test]
fn python_docstring_continuations_do_not_repeat_the_opening_delimiter() {
    let mut case = request(
        "python",
        "",
        100,
        &[
            "def get_resendable_uuids(now, pushable_participant_pks):",
            "    \"\"\" Get the uuids of relevant archives. This includes a per-study timeout value for how frequently",
            "    to resend, and a filter by last updated time. \"\"\"",
        ],
    );
    case.selections = vec![cursor(1, 10)];

    assert_eq!(
        apply(&case),
        [
            "def get_resendable_uuids(now, pushable_participant_pks):",
            "    \"\"\" Get the uuids of relevant archives. This includes a per-study timeout value for how",
            "    frequently to resend, and a filter by last updated time. \"\"\"",
        ]
    );
}

#[test]
fn prefixed_python_docstring_continuations_do_not_repeat_the_opening_delimiter() {
    for delimiter in ["r\"\"\"", "fr\"\"\"", "b'''", "u'''"] {
        let first = format!("    {delimiter} one two three four five six seven eight nine ten");
        let closing = if delimiter.ends_with("\"\"\"") {
            "    eleven twelve \"\"\""
        } else {
            "    eleven twelve '''"
        };
        let marker = &delimiter[delimiter.len() - 3..];
        let mut case = request("python", "", 40, &[&first, closing]);
        case.selections = vec![cursor(0, 10)];

        let output = apply(&case);
        assert!(output.len() > 2, "{delimiter}: fixture must wrap");
        assert_eq!(
            output
                .iter()
                .map(|line| line.matches(marker).count())
                .sum::<usize>(),
            2,
            "{delimiter}: only the opening and closing delimiters remain"
        );
    }
}

#[test]
fn xmldoc_start_tags_are_kept_intact_when_wrapping() {
    let input = "/// Returned by the <see cref=\"OandaApiClient.GetTrades(string, TradeStateFilter, string?, int, string?, string[]?)\"/> method.";
    assert_eq!(
        apply(&request("c#", "", 54, &[input])),
        [
            "/// Returned by the",
            "/// <see cref=\"OandaApiClient.GetTrades(string, TradeStateFilter, string?, int, string?, string[]?)\"/>",
            "/// method.",
        ]
    );

    let quoted = "/// Compare <see cref=\"A > B\" langword=\"one two three\"/> safely.";
    assert_eq!(
        apply(&request("c#", "", 30, &[quoted])),
        [
            "/// Compare",
            "/// <see cref=\"A > B\" langword=\"one two three\"/>",
            "/// safely.",
        ]
    );
}

#[test]
fn exports_the_complete_reference_language_registry_in_order() {
    assert_eq!(
        languages(),
        [
            "AsciiDoc",
            "AutoHotkey",
            "Basic",
            "Batch file",
            "Bikeshed",
            "C/C++",
            "C#",
            "Clojure",
            "CMake",
            "CoffeeScript",
            "Common Lisp",
            "Configuration",
            "Crystal",
            "CSS",
            "D",
            "Dart",
            "Dockerfile",
            "Elixir",
            "Elm",
            "Emacs Lisp",
            "F#",
            "FIDL",
            "Go",
            "Git commit",
            "GraphQL",
            "Groovy",
            "Handlebars",
            "Haskell",
            "HCL",
            "HTML",
            "INI",
            "J",
            "Java",
            "JavaScript",
            "Julia",
            "JSON",
            "LaTeX",
            "Lean",
            "Less",
            "Lua",
            "Makefile",
            "Markdown",
            "MATLAB",
            "Objective-C",
            "Octave",
            "Pascal",
            "Perl",
            "PHP",
            "PowerShell",
            "Prisma",
            "Prolog",
            "Protobuf",
            "Pug",
            "PureScript",
            "Python",
            "R",
            "reStructuredText",
            "Ruby",
            "Rust",
            "SCSS",
            "Scala",
            "Scheme",
            "Shaderlab",
            "Shell script",
            "SQL",
            "Swift",
            "Tcl",
            "Textile",
            "TOML",
            "TypeScript",
            "Verilog/SystemVerilog",
            "XAML",
            "XML",
            "YAML",
        ]
    );
}

#[test]
fn path_inference_and_aliases_use_the_resolved_language_parser() {
    for mut case in [
        request("plaintext", "/tmp/main.rs", 12, &["// one two three four"]),
        request("typescript", "", 12, &["// one two three four"]),
        request("postcss", "", 12, &["// one two three four"]),
        request("terraform", "", 12, &["# one two three four"]),
    ] {
        assert_ne!(apply(&case), case.lines, "{}", case.file.language);
        case.lines.clear();
    }
}

#[test]
fn every_reference_source_language_has_its_comment_processor() {
    let cases = [
        ("autohotkey", ";"),
        ("basic", "'"),
        ("batch file", "rem"),
        ("c/c++", "//"),
        ("c#", "//"),
        ("clojure", ";"),
        ("cmake", "#"),
        ("coffeescript", "#"),
        ("common lisp", ";"),
        ("configuration", "#"),
        ("crystal", "#"),
        ("css", "//"),
        ("d", "//"),
        ("dart", "//"),
        ("dockerfile", "#"),
        ("elixir", "#"),
        ("elm", "--"),
        ("emacs lisp", ";"),
        ("f#", "//"),
        ("fidl", "//"),
        ("go", "//"),
        ("graphql", "#"),
        ("groovy", "//"),
        ("haskell", "--"),
        ("hcl", "#"),
        ("ini", ";"),
        ("j", "NB."),
        ("java", "//"),
        ("javascript", "//"),
        ("julia", "#"),
        ("json", "//"),
        ("lean", "--"),
        ("less", "//"),
        ("lua", "--"),
        ("makefile", "#"),
        ("matlab", "%"),
        ("objective-c", "//"),
        ("octave", "#"),
        ("pascal", "//"),
        ("perl", "#"),
        ("php", "#"),
        ("powershell", "#"),
        ("prisma", "//"),
        ("prolog", "%"),
        ("protobuf", "//"),
        ("pug", "//"),
        ("purescript", "--"),
        ("python", "#"),
        ("r", "#"),
        ("ruby", "#"),
        ("rust", "//"),
        ("scss", "//"),
        ("scala", "//"),
        ("scheme", ";"),
        ("shaderlab", "//"),
        ("shell script", "#"),
        ("sql", "--"),
        ("swift", "//"),
        ("tcl", "#"),
        ("toml", "#"),
        ("typescript", "//"),
        ("verilog/systemverilog", "//"),
        ("yaml", "#"),
    ];

    for (language, marker) in cases {
        let source = format!("{marker} one two three four five");
        let case = request(language, "", 12, &[&source]);
        assert_ne!(apply(&case), case.lines, "missing parser for {language}");
    }
}

#[test]
fn source_language_marker_sets_do_not_expand_beyond_the_reference() {
    assert!(wrap(&request("rust", "", 10, &["/* one two three */"])).is_empty());
    assert!(wrap(&request("protobuf", "", 10, &["/* one two three */"])).is_empty());
    assert!(wrap(&request("pascal", "", 10, &["{$one two three}"])).is_empty());
}

#[test]
fn explicit_language_ids_are_not_trimmed_before_lookup() {
    let file = File {
        language: " markdown ".to_owned(),
        path: "/tmp/notes.md".to_owned(),
        custom_markers: CustomMarkers::default(),
    };

    assert_eq!(language_name_for_file(&file), None);
}

#[test]
fn c_uses_the_reference_doc_comment_parser() {
    assert_eq!(
        apply(&request("c", "", 8, &["/// a b c"])),
        ["/// a b", "/// c"]
    );
}

#[test]
fn top_level_markdown_reformat_preserves_existing_list_item_indent() {
    let mut case = request("markdown", "", 8, &["* a b", "  c d", " * a b", "   c d"]);
    case.settings.reformat = true;

    assert_eq!(apply(&case), ["* a b c", "  d", " * a b c", "   d"]);
}

#[test]
fn top_level_markdown_does_not_apply_legacy_blockquote_reformatting() {
    let mut case = request("markdown", "", 6, &["* a", "  >b", " >c"]);
    case.settings.reformat = true;

    let edit = wrap(&case);
    assert_eq!(edit.start_line, 1);
    assert_eq!(edit.end_line, 2);
    assert_eq!(edit.lines, ["  >b c"]);
}

#[test]
fn powershell_unicode_whitespace_never_slices_inside_utf8() {
    let case = request(
        "powershell",
        "",
        20,
        &["# .DESCRIPTION", "#  alpha words", "# \u{a0}beta words"],
    );

    assert!(catch_unwind(AssertUnwindSafe(|| apply(&case))).is_ok());
}

#[test]
fn changed_rulers_reset_to_the_first_even_if_the_document_is_unchanged() {
    let mut state = ColumnState::default();
    let document = DocState {
        file_path: "/tmp/rulers".to_owned(),
        version: 1,
        selections: vec![cursor(0, 0)],
    };
    assert_eq!(state.wrapping_column(&document.file_path, &[72, 88]), 72);
    state.save_document(document.clone());

    assert_eq!(
        state.maybe_change_wrapping_column(&document, &[100, 120]),
        100
    );
}

#[test]
fn no_op_wraps_preserve_the_input_selections() {
    let mut case = request("plaintext", "", 80, &["short"]);
    case.selections = vec![cursor(0, 2)];

    let edit = wrap(&case);
    assert!(edit.is_empty());
    assert_eq!(edit.selections, case.selections);
}

#[test]
fn known_languages_ignore_custom_markers() {
    let mut case = request("rust", "", 12, &["# one two three four"]);
    case.file.custom_markers.line = "#".to_owned();

    assert!(wrap(&case).is_empty());
}

#[test]
fn custom_block_markers_take_precedence_over_overlapping_line_markers() {
    let mut case = request("custom-overlap", "", 12, &["/* one two three four */"]);
    case.file.custom_markers = CustomMarkers {
        line: "/".to_owned(),
        block: ("/*".to_owned(), "*/".to_owned()),
    };

    assert_eq!(apply(&case), ["/* one two", "three four", "*/"]);
}

#[test]
fn custom_languages_reuse_the_first_valid_marker_definition() {
    let mut first = request("custom-cache-contract", "", 12, &["# one two three four"]);
    first.file.custom_markers.line = "#".to_owned();
    assert_ne!(apply(&first), first.lines);

    let mut second = request("custom-cache-contract", "", 12, &["# one two three four"]);
    second.file.custom_markers.line = "//".to_owned();
    assert_ne!(apply(&second), second.lines);
}

#[test]
fn ineligible_auto_wrap_returns_the_reference_empty_edit() {
    let mut case = request("plaintext", "", 8, &["one two three"]);
    case.selections = vec![cursor(0, 3)];

    let edit = maybe_auto_wrap(
        &case,
        "x",
        Position {
            line: 0,
            character: 3,
        },
    );
    assert!(edit.is_empty());
    assert!(edit.selections.is_empty());
}

#[test]
fn whole_comment_wraps_every_paragraph_in_an_html_comment() {
    let mut case = request(
        "html",
        "",
        8,
        &["<!--", "one two three", "", "four five six", "-->"],
    );
    case.selections = vec![cursor(1, 2)];

    assert_eq!(
        apply(&case),
        ["<!--", "one two", "three", "", "four", "five six", "-->"]
    );
}

#[test]
fn invalid_rst_roman_numerals_are_plain_paragraphs() {
    assert_eq!(
        apply(&request("rst", "", 10, &["iiv. one two three"])),
        ["iiv. one", "two three"]
    );
}

#[test]
fn rst_simple_table_state_matches_the_reference() {
    let lines = ["= =", "a b", "= =", "", "normal text that is long"];
    assert_eq!(apply(&request("rst", "", 10, &lines)), lines);
}

#[test]
fn markdown_indentation_counts_utf16_units_not_utf8_bytes() {
    assert_eq!(
        apply(&request("markdown", "", 10, &["　　one two three"])),
        ["　　one", "　　two", "　　three"]
    );
}

#[test]
fn markdown_code_blocks_are_never_wrapped() {
    let cases = [
        ["```rust", "let result = one + two + three + four;", "```"].as_slice(),
        ["~~~text", "one two three four five six", "~~~"].as_slice(),
        [
            "- ```rust",
            "  let result = one + two + three + four;",
            "  ```",
        ]
        .as_slice(),
        [
            "> ```rust",
            "> let result = one + two + three + four;",
            "> ```",
        ]
        .as_slice(),
        ["\tlet result = one + two + three + four;"].as_slice(),
    ];

    for lines in cases {
        assert!(
            wrap(&request("markdown", "", 12, lines)).is_empty(),
            "{lines:?}"
        );
    }

    let mut selected = request(
        "markdown",
        "",
        12,
        &["```rust", "let result = one + two + three + four;", "```"],
    );
    selected.selections = vec![cursor(1, 4)];
    assert!(wrap(&selected).is_empty());
}

#[test]
fn markdown_tables_are_never_wrapped() {
    let cases = [
        ["| Name |", "| --- |", "| Alice |"].as_slice(),
        ["| A | B |", "| - | - |", "| 1 | 2 |"].as_slice(),
        ["| :---: |", "| --- |", "| c |"].as_slice(),
        ["Name | Age", "--- | ---", "Alice | 30"].as_slice(),
        ["| Escaped \\| pipe | Value |", "| --- | --- |", "| a | b |"].as_slice(),
        [
            "> | quoted | table |",
            "> | --- | --- |",
            "> | inside | blockquote |",
        ]
        .as_slice(),
        ["- | item | table |", "  | --- | --- |", "  | in | list |"].as_slice(),
    ];

    for lines in cases {
        assert!(
            wrap(&request("markdown", "", 12, lines)).is_empty(),
            "{lines:?}"
        );
    }
}

#[test]
fn plaintext_uses_the_first_line_indent_for_the_whole_paragraph() {
    assert_eq!(
        apply(&request("plaintext", "", 10, &["one two", " three four"])),
        ["one two", "three four"]
    );
}

#[test]
fn escaped_latex_end_markers_do_not_end_preserved_sections() {
    let lines = [r"\[", r"\\]", "one two three four"];
    assert_eq!(apply(&request("latex", "", 8, &lines)), lines);
}

#[test]
fn selection_normalization_preserves_the_reference_input_order() {
    let mut case = request("plaintext", "", 8, &["one two three", "", "four five six"]);
    case.selections = vec![cursor(2, 0), cursor(0, 0)];

    assert_eq!(apply(&case), ["one two", "three", "", "four five six"]);
}

#[test]
fn html_embedded_sections_use_unanchored_reference_markers() {
    assert_eq!(
        apply(&request(
            "html",
            "",
            12,
            &["prefix <script>", "// one two three", "</script>"]
        )),
        ["prefix <script>", "// one two", "// three", "</script>"]
    );
}
