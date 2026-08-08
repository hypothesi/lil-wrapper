use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use rewrap_core::{Position, Selection, Settings};
use rewrap_lsp::{Configuration, LanguageServer, remap_selections};
use serde_json::{Map, Value, json};

static ORACLE: OnceLock<Value> = OnceLock::new();

#[test]
fn production_settings_match_the_original_vscode_adapter() {
    for case in oracle()["settings"].as_array().expect("settings cases") {
        let id = case["id"].as_str().expect("settings case id");
        if id == "scope-origin" {
            assert_eq!(case["columns"]["value"], json!([55]));
            assert_eq!(case["autoWrap"]["enabled"]["value"], false);
            continue;
        }

        let values = case["input"]["values"]
            .as_object()
            .expect("settings input values");
        let (rewrap, editor) = settings_sections(values);
        let configuration = Configuration::from_sections(&rewrap, &editor);
        let tab_size = case["input"]["tabSize"]
            .as_f64()
            .expect("settings tab size");

        if case.get("error").is_some() {
            assert!(
                configuration
                    .settings(configuration.columns()[0], Some(tab_size))
                    .is_err(),
                "{id}: the original adapter rejects an invalid tab size"
            );
            continue;
        }

        let expected_columns = case["editor"]["columns"]["value"]
            .as_array()
            .expect("original columns")
            .iter()
            .map(|column| usize::try_from(column.as_i64().expect("integer column")).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(configuration.columns(), expected_columns, "{id}: columns");

        let actual = configuration
            .settings(expected_columns[0], Some(tab_size))
            .expect("valid original core settings");
        assert_settings(&actual, &case["core"], id);
        assert_eq!(
            configuration.auto_wrap_enabled(),
            case["editor"]["autoWrap"]["enabled"]["value"]
                .as_bool()
                .expect("original auto-wrap value"),
            "{id}: auto-wrap"
        );
    }
}

#[test]
fn selection_remapping_matches_the_original_fast_diff_adapter() {
    for case in oracle()["selections"].as_array().expect("selection cases") {
        let old_lines = strings(&case["oldLines"]);
        let new_lines = strings(&case["newLines"]);
        let input = selections(&case["selectionsInput"]);
        let actual = remap_selections(
            &old_lines,
            &new_lines,
            json_usize(&case["startLine"]),
            json_usize(&case["endLine"]),
            &input,
        );
        assert_eq!(
            selections_json(&actual),
            case["selections"],
            "{}: remapped selections",
            case["id"]
        );
    }
}

#[test]
fn commands_and_manifest_match_the_original_vscode_adapter() {
    let original_commands = oracle()["commands"]["registered"]
        .as_array()
        .expect("registered commands");
    let manifest_commands = oracle()["manifest"]["contributes"]["commands"]
        .as_array()
        .expect("manifest commands")
        .iter()
        .map(|command| command["command"].clone())
        .collect::<Vec<_>>();
    assert_eq!(manifest_commands.len(), 3);

    let mut server = LanguageServer::new();
    let initialized = initialize(&mut server);
    let mut advertised = initialized["capabilities"]["executeCommandProvider"]["commands"]
        .as_array()
        .expect("advertised commands")
        .clone();
    advertised.sort_by_key(Value::to_string);
    assert_eq!(advertised, *original_commands);

    let calls = oracle()["commands"]["rewrapCalls"]
        .as_array()
        .expect("original command calls");
    let expected_calls = json!([8, 12, 0])
        .as_array()
        .unwrap()
        .iter()
        .map(|column| {
            json!({
                "file": {
                    "path": "/tmp/commands.txt",
                    "language": "plaintext",
                    "markers": {"line": "", "block": ["", ""]}
                },
                "settings": {
                    "column": column,
                    "doubleSentenceSpacing": false,
                    "wholeComment": true,
                    "reformat": false,
                    "tabWidth": 4
                },
                "selections": [{
                    "anchor": {"line": 2, "character": 4},
                    "active": {"line": 2, "character": 4}
                }],
                "lines": ["first paragraph", "", "one two three four"]
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(calls, &expected_calls);
    assert_eq!(oracle()["commands"]["cancelled"], true);
}

#[test]
fn custom_marker_normalization_matches_the_original_vscode_adapter() {
    for result in oracle()["customLanguages"]["results"]
        .as_array()
        .expect("custom language results")
    {
        let markers = &result["markers"];
        let configuration = Configuration::from_sections(
            &json!({"customMarkers": {
                "lineComment": markers["line"],
                "blockComment": markers["block"],
            }}),
            &json!({}),
        );
        let expected_line = markers["line"].as_str().unwrap_or_default();
        let expected_block = markers["block"]
            .as_array()
            .and_then(|parts| Some((parts.first()?.as_str()?, parts.get(1)?.as_str()?)));
        assert_eq!(configuration.custom_markers.line, expected_line);
        assert_eq!(
            configuration.custom_markers.block,
            expected_block
                .map(|(start, end)| (start.to_owned(), end.to_owned()))
                .unwrap_or_default()
        );
        if result["language"] == "nonstring-block" {
            assert_eq!(markers["block"], json!([1, 2]));
            assert_eq!(
                configuration.custom_markers.block,
                (String::new(), String::new())
            );
        }
    }
    assert_eq!(
        oracle()["customLanguages"]["reads"],
        json!({
            "/one/line.json": 1,
            "/two/block.json": 1,
            "/two/new.json": 1,
            "/two/invalid.json": 1,
            "/two/parse-error.json": 1,
            "/two/missing-comments.json": 1,
            "/two/nonstring-block.json": 1,
        }),
        "the original adapter caches each language configuration"
    );
}

#[test]
fn auto_wrap_event_gates_match_the_original_vscode_adapter() {
    let original = oracle()["autoWrap"]["eligibility"]
        .as_array()
        .expect("auto-wrap eligibility cases");
    for case in original {
        let id = case["id"].as_str().expect("auto-wrap case id");
        if matches!(
            id,
            "multiple-selections" | "nonempty-selection" | "negative-range-length"
        ) {
            continue;
        }
        assert_eq!(
            rust_auto_wrap_called(id),
            case["called"].as_bool().expect("original call result"),
            "{id}: auto-wrap event eligibility"
        );
    }
}

#[test]
fn auto_wrap_toggle_state_matches_the_original_vscode_adapter() {
    let uri = "file:///tmp/auto-toggle.txt";
    let key = "file:///tmp/auto.txt:autoWrap.enabled";
    assert_eq!(oracle()["autoWrap"]["afterOn"][&key], true);
    assert!(
        oracle()["autoWrap"]["afterOff"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    assert_eq!(oracle()["autoWrap"]["afterConfigurationFlip"][&key], false);
    assert!(
        oracle()["autoWrap"]["afterConfigurationFlipReset"]
            .as_object()
            .unwrap()
            .is_empty()
    );

    let mut server = LanguageServer::new();
    initialize(&mut server);
    server
        .notify(
            "textDocument/didOpen",
            json!({"textDocument": {
                "uri": uri,
                "languageId": "plaintext",
                "version": 1,
                "text": "one two three"
            }}),
        )
        .expect("open toggle document");
    configure_auto_wrap(&mut server, false);
    assert!(toggle_auto_wrap(&mut server, uri));
    assert!(!toggle_auto_wrap(&mut server, uri));
    assert!(toggle_auto_wrap(&mut server, uri));
    configure_auto_wrap(&mut server, true);
    assert!(!toggle_auto_wrap(&mut server, uri));
    assert!(toggle_auto_wrap(&mut server, uri));
}

#[test]
fn common_edit_application_matches_the_original_vscode_adapter() {
    let common = &oracle()["common"];
    assert_eq!(common["docLine"], json!(["first", "last", null]));
    assert_eq!(common["docType"]["path"], "/tmp/doc-line.txt");
    assert_eq!(common["docType"]["language"], "plaintext");

    let input = vec![
        Selection {
            anchor: Position {
                line: 1,
                character: 8,
            },
            active: Position {
                line: 1,
                character: 8,
            },
        },
        Selection {
            anchor: Position {
                line: 2,
                character: 3,
            },
            active: Position {
                line: 0,
                character: 2,
            },
        },
    ];
    let remapped = remap_selections(
        &["one two three".to_owned()],
        &["one two".to_owned(), "three".to_owned()],
        1,
        1,
        &input,
    );
    assert_eq!(selections_json(&remapped), common["applied"]["selections"]);
    assert_eq!(
        common["applied"]["lines"],
        json!(["before", "one two", "three", "after"])
    );
    assert_eq!(common["stale"]["lines"], json!(["one two three"]));
}

#[test]
fn direct_command_paths_match_the_original_core_requests() {
    let uri = "file:///tmp/command-range.txt";
    let mut server = LanguageServer::new();
    initialize(&mut server);
    server
        .notify(
            "textDocument/didOpen",
            json!({"textDocument": {
                "uri": uri,
                "languageId": "plaintext",
                "version": 1,
                "text": "first paragraph\n\none two three four"
            }}),
        )
        .expect("open command document");

    let missing = server
        .request(
            "workspace/executeCommand",
            json!({
                "command": "rewrap.rewrapComment",
                "arguments": [{"uri": uri}]
            }),
        )
        .expect_err("command without editor range");
    assert_eq!(missing.code, rewrap_lsp::INVALID_PARAMS);

    let range = json!({
        "start": {"line": 2, "character": 4},
        "end": {"line": 2, "character": 4}
    });
    server
        .notify(
            "workspace/didChangeConfiguration",
            json!({"settings": {
                "rewrap": {"wrappingColumn": 8},
                "editor": {"tabSize": 4}
            }}),
        )
        .expect("configure standard command");
    let standard = server
        .request(
            "workspace/executeCommand",
            json!({
                "command": "rewrap.rewrapComment",
                "arguments": [{"uri": uri, "range": range}]
            }),
        )
        .expect("standard command");
    assert_eq!(
        standard["changes"][uri],
        json!([{
            "range": {
                "start": {"line": 2, "character": 0},
                "end": {"line": 2, "character": 18}
            },
            "newText": "one two\nthree\nfour"
        }])
    );

    let custom = execute_direct_command("rewrap.rewrapCommentAt", Some(12));
    assert_eq!(custom["changes"][uri][0]["newText"], "one two\nthree four");
    assert_eq!(custom["changes"][uri][0]["range"]["start"]["line"], 2);
    assert_eq!(custom["changes"][uri][0]["range"]["end"]["character"], 18);

    let unwrap = execute_direct_command_with_text(
        "rewrap.rewrapCommentAt",
        Some(0),
        "first paragraph\n\none two\nthree four",
    );
    assert_eq!(
        unwrap["changes"][uri],
        json!([{
            "range": {
                "start": {"line": 2, "character": 0},
                "end": {"line": 3, "character": 10}
            },
            "newText": "one two three four"
        }])
    );
}

#[test]
fn failed_edits_save_the_original_cycle_state() {
    assert_eq!(oracle()["commands"]["failedEditSavedState"], true);

    let uri = "file:///tmp/failed-cycle.txt";
    let mut server = LanguageServer::new();
    initialize(&mut server);
    server
        .notify(
            "textDocument/didOpen",
            json!({"textDocument": {
                "uri": uri,
                "languageId": "plaintext",
                "version": 1,
                "text": "one two three four"
            }}),
        )
        .expect("open failed-cycle document");
    server
        .notify(
            "workspace/didChangeConfiguration",
            json!({"settings": {
                "rewrap": {"wrappingColumn": 0},
                "editor": {"rulers": [8, 12], "wordWrapColumn": 80}
            }}),
        )
        .expect("configure cycle rulers");
    let params = json!({
        "command": "rewrap.rewrapComment",
        "arguments": [{
            "uri": uri,
            "range": {
                "start": {"line": 0, "character": 4},
                "end": {"line": 0, "character": 4}
            }
        }]
    });

    let first = server
        .request("workspace/executeCommand", params.clone())
        .expect("first cycle command");
    let outbound = server.take_outbound_requests();
    server
        .client_response(&outbound[0]["id"], Some(json!({"applied": false})), None)
        .expect("failed edit response");
    let second = server
        .request("workspace/executeCommand", params)
        .expect("cycle after failed edit");

    assert_eq!(first["changes"][uri][0]["newText"], "one two\nthree\nfour");
    assert_eq!(second["changes"][uri][0]["newText"], "one two\nthree four");
}

fn oracle() -> &'static Value {
    ORACLE.get_or_init(|| {
        let root = project_root();
        let output = Command::new("deno")
            .args([
                "run",
                "--quiet",
                "--allow-read=.",
                "--node-modules-dir=none",
                "--lock=tests/reference-vscode/deno.lock",
                "--frozen",
                "--unstable-sloppy-imports",
                "--import-map=tests/reference-vscode/import-map.json",
                root.join("tests/reference-vscode/oracle.mjs")
                    .to_str()
                    .expect("UTF-8 oracle path"),
            ])
            .current_dir(&root)
            .output()
            .expect("run the pinned VS Code adapter oracle");
        assert!(
            output.status.success(),
            "VS Code adapter oracle failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "VS Code adapter oracle returned invalid JSON: {error}\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        })
    })
}

fn settings_sections(values: &Map<String, Value>) -> (Value, Value) {
    let mut rewrap = Map::new();
    let mut editor = Map::new();
    for (name, value) in values {
        if let Some(name) = name.strip_prefix("rewrap.") {
            if name == "autoWrap.enabled" {
                rewrap.insert("autoWrap".to_owned(), json!({"enabled": value}));
            } else if name != "autoWrap.notification" {
                rewrap.insert(name.to_owned(), value.clone());
            }
        } else if let Some(name) = name.strip_prefix("editor.") {
            editor.insert(name.to_owned(), value.clone());
        }
    }
    (Value::Object(rewrap), Value::Object(editor))
}

fn assert_settings(actual: &Settings, expected: &Value, id: &str) {
    assert_eq!(
        actual.column,
        json_usize(&expected["column"]),
        "{id}: column"
    );
    assert_eq!(
        actual.tab_width,
        json_usize(&expected["tabWidth"]),
        "{id}: tab width"
    );
    assert_eq!(
        actual.double_sentence_spacing,
        expected["doubleSentenceSpacing"].as_bool().unwrap(),
        "{id}: double spacing"
    );
    assert_eq!(
        actual.reformat,
        expected["reformat"].as_bool().unwrap(),
        "{id}: reformat"
    );
    assert_eq!(
        actual.whole_comment,
        expected["wholeComment"].as_bool().unwrap(),
        "{id}: whole comment"
    );
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("string array")
        .iter()
        .map(|value| value.as_str().expect("string").to_owned())
        .collect()
}

fn selections(value: &Value) -> Vec<Selection> {
    value
        .as_array()
        .expect("selection array")
        .iter()
        .map(|selection| Selection {
            anchor: position(&selection["anchor"]),
            active: position(&selection["active"]),
        })
        .collect()
}

fn position(value: &Value) -> Position {
    Position {
        line: json_usize(&value["line"]),
        character: json_usize(&value["character"]),
    }
}

fn json_usize(value: &Value) -> usize {
    usize::try_from(value.as_u64().expect("unsigned integer")).expect("integer fits usize")
}

fn selections_json(selections: &[Selection]) -> Value {
    Value::Array(
        selections
            .iter()
            .map(|selection| {
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
            })
            .collect(),
    )
}

fn initialize(server: &mut LanguageServer) -> Value {
    server
        .request(
            "initialize",
            json!({
                "processId": null,
                "capabilities": {
                    "workspace": {"applyEdit": true},
                    "textDocument": {"codeAction": {
                        "codeActionLiteralSupport": {
                            "codeActionKind": {"valueSet": ["refactor.rewrite"]}
                        }
                    }}
                }
            }),
        )
        .expect("initialize server")
}

fn execute_direct_command(command: &str, column: Option<usize>) -> Value {
    execute_direct_command_with_text(command, column, "first paragraph\n\none two three four")
}

fn execute_direct_command_with_text(command: &str, column: Option<usize>, text: &str) -> Value {
    let uri = "file:///tmp/command-range.txt";
    let mut server = LanguageServer::new();
    initialize(&mut server);
    server
        .notify(
            "textDocument/didOpen",
            json!({"textDocument": {
                "uri": uri,
                "languageId": "plaintext",
                "version": 1,
                "text": text
            }}),
        )
        .expect("open direct-command document");
    server
        .request(
            "workspace/executeCommand",
            json!({
                "command": command,
                "arguments": [{
                    "uri": uri,
                    "column": column,
                    "range": {
                        "start": {"line": 2, "character": 4},
                        "end": {"line": 2, "character": 4}
                    }
                }]
            }),
        )
        .expect("execute direct command")
}

#[allow(clippy::too_many_lines)]
fn rust_auto_wrap_called(id: &str) -> bool {
    let uri = "file:///tmp/auto-differential.txt";
    let mut server = LanguageServer::new();
    initialize(&mut server);
    server
        .notify(
            "textDocument/didOpen",
            json!({"textDocument": {
                "uri": uri,
                "languageId": "plaintext",
                "version": 1,
                "text": if matches!(id, "replacement" | "ranged-newline") {
                    "one two threex"
                } else {
                    "one two three"
                }
            }}),
        )
        .expect("open auto-wrap document");
    server
        .notify(
            "workspace/didChangeConfiguration",
            json!({"settings": {"rewrap": {
                "wrappingColumn": 8,
                "autoWrap": {"enabled": true}
            }}}),
        )
        .expect("enable auto-wrap");

    if id == "wrong-document" {
        return server
            .request(
                "textDocument/onTypeFormatting",
                on_type_params("file:///tmp/not-open.txt"),
            )
            .ok()
            .and_then(|value| value.as_array().cloned())
            .is_some_and(|edits| !edits.is_empty());
    }

    let mut change = json!({
        "range": {
            "start": {"line": 0, "character": 13},
            "end": {"line": 0, "character": 13}
        },
        "rangeLength": 0,
        "text": " "
    });
    if id == "replacement" {
        change["range"]["end"]["character"] = json!(14);
        change["rangeLength"] = json!(1);
    } else if id == "ranged-newline" {
        change["range"]["end"]["character"] = json!(14);
        change["rangeLength"] = json!(1);
        change["text"] = json!("\n");
    } else if id == "missing-range-length" {
        change.as_object_mut().unwrap().remove("rangeLength");
    } else if id == "negative-range-length" {
        change["rangeLength"] = json!(-1);
    }
    let mut changes = vec![change];
    if id == "multiple-changes" {
        changes.push(json!({
            "range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 0, "character": 0}
            },
            "rangeLength": 0,
            "text": ""
        }));
    } else if id == "multi-change-newline" {
        changes = vec![
            json!({
                "range": {
                    "start": {"line": 0, "character": 13},
                    "end": {"line": 0, "character": 13}
                },
                "rangeLength": 0,
                "text": "\n"
            }),
            json!({
                "range": {
                    "start": {"line": 1, "character": 0},
                    "end": {"line": 1, "character": 0}
                },
                "rangeLength": 0,
                "text": "  "
            }),
        ];
    }
    server
        .notify(
            "textDocument/didChange",
            json!({
                "textDocument": {"uri": uri, "version": 2},
                "contentChanges": changes
            }),
        )
        .expect("synchronize auto-wrap typing");
    server
        .request(
            "textDocument/onTypeFormatting",
            if matches!(id, "ranged-newline" | "multi-change-newline") {
                json!({
                    "textDocument": {"uri": uri},
                    "position": {"line": 1, "character": if id == "multi-change-newline" {2} else {0}},
                    "ch": "\n",
                    "options": {"tabSize": 4, "insertSpaces": true}
                })
            } else {
                on_type_params(uri)
            },
        )
        .expect("auto-wrap response")
        .as_array()
        .is_some_and(|edits| !edits.is_empty())
}

fn on_type_params(uri: &str) -> Value {
    json!({
        "textDocument": {"uri": uri},
        "position": {"line": 0, "character": 14},
        "ch": " ",
        "options": {"tabSize": 4, "insertSpaces": true}
    })
}

fn configure_auto_wrap(server: &mut LanguageServer, enabled: bool) {
    server
        .notify(
            "workspace/didChangeConfiguration",
            json!({"settings": {"rewrap": {
                "wrappingColumn": 8,
                "autoWrap": {"enabled": enabled}
            }}}),
        )
        .expect("configure auto-wrap");
}

fn toggle_auto_wrap(server: &mut LanguageServer, uri: &str) -> bool {
    server
        .request(
            "workspace/executeCommand",
            json!({
                "command": "rewrap.toggleAutoWrap",
                "arguments": [{"uri": uri}]
            }),
        )
        .expect("toggle auto-wrap")["enabled"]
        .as_bool()
        .expect("toggle state")
}

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
