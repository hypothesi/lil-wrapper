use rewrap_lsp::{INVALID_REQUEST, LanguageServer, SERVER_NOT_INITIALIZED};
use serde_json::{Value, json};

fn initialize(server: &mut LanguageServer) -> Value {
    server
        .request(
            "initialize",
            json!({
                "processId": null,
                "capabilities": {
                    "workspace": {
                        "configuration": true,
                        "applyEdit": true,
                        "workspaceEdit": {"documentChanges": true}
                    },
                    "textDocument": {"codeAction": {
                        "dataSupport": true,
                        "codeActionLiteralSupport": {
                            "codeActionKind": {"valueSet": ["refactor.rewrite"]}
                        }
                    }}
                },
                "rootUri": "file:///tmp/project"
            }),
        )
        .expect("initialize response")
}

fn open(server: &mut LanguageServer, uri: &str, language: &str, version: i64, text: &str) {
    server
        .notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": language,
                    "version": version,
                    "text": text
                }
            }),
        )
        .expect("didOpen accepted");
}

#[test]
fn advertises_every_zed_compatible_rewrap_operation() {
    let result = initialize(&mut LanguageServer::new());
    let capabilities = &result["capabilities"];

    assert_eq!(capabilities["positionEncoding"], "utf-16");
    assert_eq!(capabilities["textDocumentSync"]["change"], 2);
    assert_eq!(capabilities["documentFormattingProvider"], true);
    assert_eq!(capabilities["documentRangeFormattingProvider"], true);
    assert_eq!(
        capabilities["documentOnTypeFormattingProvider"],
        json!({"firstTriggerCharacter": " ", "moreTriggerCharacter": ["\t", "\n"]})
    );
    assert_eq!(capabilities["codeActionProvider"], true);
    assert_eq!(
        capabilities["executeCommandProvider"]["commands"],
        json!([
            "rewrap.rewrapComment",
            "rewrap.rewrapCommentAt",
            "rewrap.toggleAutoWrap"
        ])
    );
}

#[test]
fn enforces_the_json_rpc_lifecycle() {
    let mut server = LanguageServer::new();
    assert_eq!(
        server
            .request("textDocument/formatting", json!({}))
            .expect_err("request before initialize")
            .code,
        SERVER_NOT_INITIALIZED
    );
    initialize(&mut server);
    assert_eq!(
        server
            .request("initialize", json!({}))
            .expect_err("duplicate initialize")
            .code,
        INVALID_REQUEST
    );
    server
        .request("shutdown", Value::Null)
        .expect("shutdown response");
    assert_eq!(
        server
            .request("textDocument/formatting", json!({}))
            .expect_err("request after shutdown")
            .code,
        INVALID_REQUEST
    );
    server.notify("exit", Value::Null).expect("exit accepted");
}

#[test]
fn invalid_initialize_params_do_not_advance_the_lifecycle() {
    let mut server = LanguageServer::new();

    let missing = server
        .request("initialize", json!({}))
        .expect_err("missing capabilities");
    let null = server
        .request("initialize", json!({"capabilities": null}))
        .expect_err("null capabilities");
    let valid = initialize(&mut server);

    assert_eq!(missing.code, rewrap_lsp::INVALID_PARAMS);
    assert_eq!(null.code, rewrap_lsp::INVALID_PARAMS);
    assert_eq!(valid["capabilities"]["positionEncoding"], "utf-16");
}

#[test]
fn formats_a_document_with_runtime_settings_and_preserves_crlf() {
    let mut server = LanguageServer::new();
    initialize(&mut server);
    open(
        &mut server,
        "file:///tmp/a.txt",
        "plaintext",
        1,
        "one two three\r\nfour five\r\n",
    );
    server
        .notify(
            "workspace/didChangeConfiguration",
            json!({"settings": {"rewrap": {"wrappingColumn": 8}}}),
        )
        .expect("configuration accepted");

    let edits = server
        .request(
            "textDocument/formatting",
            json!({
                "textDocument": {"uri": "file:///tmp/a.txt"},
                "options": {"tabSize": 4, "insertSpaces": true}
            }),
        )
        .expect("formatting response");

    assert_eq!(edits[0]["newText"], "one two\r\nthree\r\nfour\r\nfive");
}

#[test]
fn applies_incremental_utf16_changes_before_formatting() {
    let mut server = LanguageServer::new();
    initialize(&mut server);
    open(
        &mut server,
        "file:///tmp/unicode.txt",
        "plaintext",
        1,
        "a😀 c",
    );
    server
        .notify(
            "textDocument/didChange",
            json!({
                "textDocument": {"uri": "file:///tmp/unicode.txt", "version": 2},
                "contentChanges": [{
                    "range": {
                        "start": {"line": 0, "character": 3},
                        "end": {"line": 0, "character": 3}
                    },
                    "rangeLength": 0,
                    "text": " b"
                }]
            }),
        )
        .expect("incremental UTF-16 edit accepted");

    let actions = server
        .request(
            "textDocument/codeAction",
            json!({
                "textDocument": {"uri": "file:///tmp/unicode.txt"},
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 6}},
                "context": {"diagnostics": []}
            }),
        )
        .expect("code action response");

    assert_eq!(actions[0]["title"], "Rewrap Comment / Text");
    assert_eq!(
        actions[0]["edit"]["documentChanges"][0]["textDocument"]["version"],
        2
    );
}

#[test]
fn exposes_direct_configured_column_unwrap_and_auto_wrap_actions() {
    let mut server = LanguageServer::new();
    initialize(&mut server);
    open(
        &mut server,
        "file:///tmp/a.txt",
        "plaintext",
        1,
        "one two three four",
    );
    server
        .notify(
            "workspace/didChangeConfiguration",
            json!({"settings": {
                "rewrap": {"wrappingColumn": 0},
                "editor": {"rulers": [8, {"column": 12}], "wordWrapColumn": 80}
            }}),
        )
        .expect("configuration");

    let actions = server
        .request(
            "textDocument/codeAction",
            json!({
                "textDocument": {"uri": "file:///tmp/a.txt"},
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 18}},
                "context": {"diagnostics": []}
            }),
        )
        .expect("code actions");
    let titles = actions
        .as_array()
        .expect("action array")
        .iter()
        .map(|action| action["title"].as_str().expect("action title"))
        .collect::<Vec<_>>();

    assert!(titles.contains(&"Rewrap Comment / Text"));
    assert!(titles.contains(&"Rewrap at Column 8"));
    assert!(titles.contains(&"Rewrap at Column 12"));
    assert!(!titles.contains(&"Rewrap at Column..."));
    assert!(titles.contains(&"Unwrap Comment / Text"));
    assert!(titles.contains(&"Toggle Auto-Wrap for Current Document"));
    let column_twelve = actions
        .as_array()
        .expect("action array")
        .iter()
        .find(|action| action["title"] == "Rewrap at Column 12")
        .expect("column 12 action");
    assert_eq!(
        column_twelve["edit"]["documentChanges"][0]["edits"][0]["newText"],
        "one two\nthree four"
    );
}

#[test]
fn filters_code_actions_by_requested_kind() {
    let uri = "file:///tmp/only.txt";
    let mut server = LanguageServer::new();
    initialize(&mut server);
    open(&mut server, uri, "plaintext", 1, "one two three");
    let request = |only: &str| {
        json!({
            "textDocument": {"uri": uri},
            "range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 0, "character": 13}
            },
            "context": {"diagnostics": [], "only": [only]}
        })
    };

    let quick_fixes = server
        .request("textDocument/codeAction", request("quickfix"))
        .expect("quick-fix request");
    let refactors = server
        .request("textDocument/codeAction", request("refactor"))
        .expect("refactor request");

    assert_eq!(quick_fixes, json!([]));
    assert!(!refactors.as_array().expect("refactor actions").is_empty());
}

#[test]
fn adapts_or_rejects_edit_paths_from_client_capabilities() {
    let uri = "file:///tmp/capabilities.txt";
    let mut legacy = LanguageServer::new();
    legacy
        .request("initialize", json!({"capabilities": {}}))
        .expect("legacy initialize");
    open(&mut legacy, uri, "plaintext", 1, "one two three");
    let action_params = json!({
        "textDocument": {"uri": uri},
        "range": {
            "start": {"line": 0, "character": 0},
            "end": {"line": 0, "character": 13}
        },
        "context": {"diagnostics": []}
    });

    let no_literals = legacy
        .request("textDocument/codeAction", action_params.clone())
        .expect("unsupported literal actions");
    let no_apply = legacy
        .request(
            "workspace/executeCommand",
            json!({
                "command": "rewrap.rewrapCommentAt",
                "arguments": [{"uri": uri, "column": 8}]
            }),
        )
        .expect_err("unsupported applyEdit");

    assert_eq!(no_literals, json!([]));
    assert_eq!(no_apply.code, -32_803);

    let mut changes_client = LanguageServer::new();
    changes_client
        .request(
            "initialize",
            json!({"capabilities": {"textDocument": {"codeAction": {
                "codeActionLiteralSupport": {
                    "codeActionKind": {"valueSet": ["refactor.rewrite"]}
                }
            }}}}),
        )
        .expect("literal initialize");
    open(&mut changes_client, uri, "plaintext", 1, "one two three");
    let actions = changes_client
        .request("textDocument/codeAction", action_params)
        .expect("literal actions");

    assert!(actions[0]["edit"].get("changes").is_some());
    assert!(actions[0]["edit"].get("documentChanges").is_none());
}

#[test]
fn requests_scoped_configuration_when_the_client_supports_it() {
    let mut server = LanguageServer::new();
    initialize(&mut server);
    server
        .notify("initialized", json!({}))
        .expect("initialized accepted");

    let outbound = server.take_outbound_requests();
    assert_eq!(outbound.len(), 1);
    assert_eq!(outbound[0]["method"], "workspace/configuration");
    assert_eq!(outbound[0]["params"]["items"][0]["section"], "rewrap");
}
