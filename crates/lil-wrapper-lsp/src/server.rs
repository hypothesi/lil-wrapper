use crate::document::{ContentChange, Document, LspPosition, LspRange};
use crate::remap_selections;
use crate::settings::Configuration;
use lil_wrapper_core::{DocState, Edit, File, Selection, WrapRequest, maybe_auto_wrap, wrap};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};
use std::mem;

pub const SERVER_NOT_INITIALIZED: i64 = -32_002;
pub const CONTENT_MODIFIED: i64 = -32_801;
pub const INVALID_REQUEST: i64 = -32_600;
pub const METHOD_NOT_FOUND: i64 = -32_601;
pub const INVALID_PARAMS: i64 = -32_602;
pub const INTERNAL_ERROR: i64 = -32_603;
const REQUEST_FAILED: i64 = -32_803;

const COMMAND_WRAP: &str = "lil-wrapper.wrapComment";
const COMMAND_WRAP_AT: &str = "lil-wrapper.wrapCommentAt";
const COMMAND_TOGGLE_AUTO_WRAP: &str = "lil-wrapper.toggleAutoWrap";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

impl RpcError {
    pub(crate) fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Lifecycle {
    #[default]
    PreInitialize,
    Running,
    Shutdown,
}

#[derive(Clone, Debug, Default)]
struct ClientCapabilities {
    workspace: WorkspaceCapabilities,
    code_action_literals: bool,
}

#[derive(Clone, Debug, Default)]
struct WorkspaceCapabilities {
    configuration: bool,
    apply_edit: bool,
    document_changes: bool,
}

#[derive(Clone, Debug)]
enum ConfigurationScope {
    Global,
    Document { uri: String, instance: u64 },
}

#[derive(Clone, Debug)]
struct DeferredResponse {
    id: Value,
    result: Value,
}

#[derive(Clone, Debug)]
struct CycleToken {
    uri: String,
    source_version: i64,
    instance: u64,
}

#[derive(Clone, Debug)]
enum PendingRequest {
    Configuration {
        generation: u64,
        scope: ConfigurationScope,
    },
    ApplyEdit {
        deferred: Option<DeferredResponse>,
        cycle: Option<CycleToken>,
    },
}

#[derive(Clone, Debug)]
struct ScopedConfiguration {
    generation: u64,
    instance: u64,
    configuration: Configuration,
}

#[derive(Clone, Debug)]
struct PendingCycle {
    instance: u64,
    source_version: i64,
    expected_range: LspRange,
    expected_text: String,
    post_selections: Vec<Selection>,
    source_selections: Vec<Selection>,
}

#[derive(Debug, Default)]
struct ColumnTracker {
    last_document: Option<DocState>,
    columns: HashMap<String, usize>,
}

impl ColumnTracker {
    fn current(&mut self, uri: &str, rulers: &[usize]) -> usize {
        let first = rulers[0];
        let current = self
            .columns
            .get(uri)
            .copied()
            .filter(|column| rulers.contains(column))
            .unwrap_or(first);
        self.columns.insert(uri.to_owned(), current);
        current
    }

    fn begin_cycle(
        &mut self,
        document: &DocState,
        rulers: &[usize],
        already_pending: bool,
    ) -> (usize, usize) {
        let previous = self.current(&document.file_path, rulers);
        if already_pending || self.last_document.as_ref() != Some(document) {
            return (previous, previous);
        }
        let index = rulers
            .iter()
            .position(|column| *column == previous)
            .unwrap_or_default();
        let selected = rulers[(index + 1) % rulers.len()];
        self.columns.insert(document.file_path.clone(), selected);
        (selected, previous)
    }

    fn commit(&mut self, document: DocState) {
        self.last_document = Some(document);
    }

    fn close(&mut self, uri: &str) {
        if self
            .last_document
            .as_ref()
            .is_some_and(|document| document.file_path == uri)
        {
            self.last_document = None;
        }
    }
}

#[derive(Debug, Default)]
pub struct LanguageServer {
    lifecycle: Lifecycle,
    client_initialized: bool,
    exited: bool,
    exit_code: i32,
    client_capabilities: ClientCapabilities,
    documents: HashMap<String, Document>,
    document_instances: HashMap<String, u64>,
    next_document_instance: u64,
    configuration: Configuration,
    configuration_generation: u64,
    document_configurations: HashMap<String, ScopedConfiguration>,
    auto_wrap_overrides: HashMap<String, bool>,
    column_state: ColumnTracker,
    pending_cycles: HashMap<String, PendingCycle>,
    outbound_requests: Vec<Value>,
    pending_requests: HashMap<String, PendingRequest>,
    last_apply_edit_request: Option<String>,
    next_request_id: i64,
}

impl LanguageServer {
    #[must_use]
    pub fn new() -> Self {
        Self {
            next_request_id: 1,
            next_document_instance: 1,
            ..Self::default()
        }
    }

    /// Handles a client-to-server JSON-RPC request.
    ///
    /// # Errors
    ///
    /// Returns an LSP/JSON-RPC error when the lifecycle, method, or parameters
    /// do not permit the request.
    pub fn request(&mut self, method: &str, params: Value) -> Result<Value, RpcError> {
        if method == "initialize" {
            let result = self.initialize(&params);
            drop(params);
            return result;
        }
        match self.lifecycle {
            Lifecycle::PreInitialize => {
                return Err(RpcError::new(
                    SERVER_NOT_INITIALIZED,
                    "server has not been initialized",
                ));
            }
            Lifecycle::Shutdown => {
                return Err(RpcError::new(INVALID_REQUEST, "server is shut down"));
            }
            Lifecycle::Running => {}
        }

        match method {
            "shutdown" => {
                self.lifecycle = Lifecycle::Shutdown;
                Ok(Value::Null)
            }
            "textDocument/formatting" => self.format_document(&params),
            "textDocument/rangeFormatting" => self.format_range(&params),
            "textDocument/onTypeFormatting" => self.format_on_type(params),
            "textDocument/codeAction" => self.code_actions(&params),
            "workspace/executeCommand" => self.execute_command(&params),
            _ => Err(RpcError::new(
                METHOD_NOT_FOUND,
                format!("method not found: {method}"),
            )),
        }
    }

    /// Handles a client-to-server JSON-RPC notification.
    ///
    /// # Errors
    ///
    /// Returns an LSP/JSON-RPC error when the lifecycle, method, or parameters
    /// do not permit the notification. A JSON-RPC transport must not send that
    /// error back for a notification.
    pub fn notify(&mut self, method: &str, params: Value) -> Result<(), RpcError> {
        if method == "exit" {
            self.exited = true;
            self.exit_code = i32::from(self.lifecycle != Lifecycle::Shutdown);
            return Ok(());
        }
        match self.lifecycle {
            Lifecycle::PreInitialize => {
                return Err(RpcError::new(
                    SERVER_NOT_INITIALIZED,
                    "server has not been initialized",
                ));
            }
            Lifecycle::Shutdown => {
                return Err(RpcError::new(INVALID_REQUEST, "server is shut down"));
            }
            Lifecycle::Running => {}
        }

        match method {
            "initialized" => self.initialized(),
            "textDocument/didOpen" => self.did_open(params),
            "textDocument/didChange" => self.did_change(params),
            "textDocument/didClose" => self.did_close(&params),
            "workspace/didChangeConfiguration" => self.did_change_configuration(&params),
            "$/cancelRequest" | "$/setTrace" => Ok(()),
            _ => Err(RpcError::new(
                METHOD_NOT_FOUND,
                format!("method not found: {method}"),
            )),
        }
    }

    /// Handles a client response to a request previously emitted by the server.
    ///
    /// # Errors
    ///
    /// Returns an invalid-parameters error when a configuration response has
    /// an unexpected result shape.
    pub fn client_response(
        &mut self,
        id: &Value,
        result: Option<Value>,
        error: Option<Value>,
    ) -> Result<(), RpcError> {
        let has_error = error.is_some();
        let Some(pending) = self.pending_requests.remove(&request_key(id)) else {
            return Ok(());
        };
        match pending {
            PendingRequest::Configuration { generation, scope } => {
                if generation != self.configuration_generation || has_error {
                    return Ok(());
                }
                let result = result.unwrap_or(Value::Null);
                let values = result.as_array().ok_or_else(|| {
                    RpcError::new(
                        INVALID_PARAMS,
                        "workspace/configuration result must be an array",
                    )
                })?;
                let empty = Value::Object(Map::new());
                let section = values.first().unwrap_or(&empty);
                let editor = values.get(1).unwrap_or(&empty);
                let configuration = Configuration::from_sections(section, editor);
                match scope {
                    ConfigurationScope::Global => self.configuration = configuration,
                    ConfigurationScope::Document { uri, instance } => {
                        if self.document_instances.get(&uri) == Some(&instance) {
                            self.document_configurations.insert(
                                uri,
                                ScopedConfiguration {
                                    generation,
                                    instance,
                                    configuration,
                                },
                            );
                        }
                    }
                }
            }
            PendingRequest::ApplyEdit { deferred, cycle } => {
                let failure = apply_edit_failure(result.as_ref(), error);
                if failure.is_some() {
                    if let Some(cycle) = cycle {
                        self.fail_cycle(&cycle);
                    }
                }
                if let Some(deferred) = deferred {
                    let response = if let Some(message) = failure {
                        json!({
                            "jsonrpc": "2.0",
                            "id": deferred.id,
                            "error": {"code": REQUEST_FAILED, "message": message}
                        })
                    } else {
                        json!({
                            "jsonrpc": "2.0",
                            "id": deferred.id,
                            "result": deferred.result
                        })
                    };
                    self.outbound_requests.push(response);
                }
            }
        }
        Ok(())
    }

    pub(crate) fn defer_last_apply_edit_response(&mut self, id: &Value, result: Value) -> bool {
        let Some(key) = self.last_apply_edit_request.take() else {
            return false;
        };
        let Some(PendingRequest::ApplyEdit { deferred, .. }) = self.pending_requests.get_mut(&key)
        else {
            return false;
        };
        *deferred = Some(DeferredResponse {
            id: id.clone(),
            result,
        });
        true
    }

    pub(crate) fn prepare_transport_request(&mut self) {
        self.last_apply_edit_request = None;
    }

    #[must_use]
    pub fn take_outbound_requests(&mut self) -> Vec<Value> {
        mem::take(&mut self.outbound_requests)
    }

    #[must_use]
    pub const fn should_exit(&self) -> bool {
        self.exited
    }

    #[must_use]
    pub const fn exit_code(&self) -> i32 {
        self.exit_code
    }

    fn initialize(&mut self, params: &Value) -> Result<Value, RpcError> {
        if self.lifecycle != Lifecycle::PreInitialize {
            return Err(RpcError::new(
                INVALID_REQUEST,
                "initialize may only be requested once",
            ));
        }
        let params: InitializeParams = parse_value(params.clone())?;
        let capabilities = Value::Object(params.capabilities);
        let code_action_literals = capabilities
            .pointer("/textDocument/codeAction/codeActionLiteralSupport/codeActionKind/valueSet")
            .and_then(Value::as_array)
            .is_some_and(|kinds| {
                kinds
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|kind| code_action_kind_matches(kind, "refactor.rewrite"))
            });
        self.client_capabilities = ClientCapabilities {
            workspace: WorkspaceCapabilities {
                configuration: capabilities
                    .pointer("/workspace/configuration")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                apply_edit: capabilities
                    .pointer("/workspace/applyEdit")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                document_changes: capabilities
                    .pointer("/workspace/workspaceEdit/documentChanges")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            },
            code_action_literals,
        };
        if let Some(options) = params.initialization_options {
            self.configuration = Configuration::from_value(&options);
        }
        self.lifecycle = Lifecycle::Running;

        Ok(json!({
            "capabilities": {
                "positionEncoding": "utf-16",
                "textDocumentSync": {"openClose": true, "change": 2},
                "documentFormattingProvider": true,
                "documentRangeFormattingProvider": true,
                "documentOnTypeFormattingProvider": {
                    "firstTriggerCharacter": " ",
                    "moreTriggerCharacter": ["\t", "\n"]
                },
                "codeActionProvider": true,
                "executeCommandProvider": {
                    "commands": [COMMAND_WRAP, COMMAND_WRAP_AT, COMMAND_TOGGLE_AUTO_WRAP]
                }
            },
            "serverInfo": {"name": "lil-wrapper-lsp", "version": env!("CARGO_PKG_VERSION")}
        }))
    }

    fn initialized(&mut self) -> Result<(), RpcError> {
        if self.client_initialized {
            return Err(RpcError::new(
                INVALID_REQUEST,
                "initialized notification was already received",
            ));
        }
        self.client_initialized = true;
        if self.client_capabilities.workspace.configuration {
            self.request_all_configurations();
        }
        Ok(())
    }

    fn did_open(&mut self, params: Value) -> Result<(), RpcError> {
        let params: DidOpenParams = parse_params(params)?;
        let item = params.text_document;
        if self.documents.contains_key(&item.uri) {
            return Err(RpcError::new(
                INVALID_REQUEST,
                format!("document is already open: {}", item.uri),
            ));
        }
        let uri = item.uri.clone();
        let instance = self.next_document_instance;
        self.next_document_instance += 1;
        self.document_instances.insert(uri.clone(), instance);
        self.documents.insert(
            uri.clone(),
            Document::new(item.uri, item.language_id, item.version, item.text),
        );
        if self.client_initialized && self.client_capabilities.workspace.configuration {
            self.request_configuration(ConfigurationScope::Document { uri, instance });
        }
        Ok(())
    }

    fn did_change(&mut self, params: Value) -> Result<(), RpcError> {
        let params: DidChangeParams = parse_params(params)?;
        let uri = params.text_document.uri.clone();
        let matching_cycle = self
            .pending_cycles
            .get(&uri)
            .filter(|pending| {
                self.document_instances.get(&uri) == Some(&pending.instance)
                    && change_matches_cycle(&params.content_changes, pending)
            })
            .cloned();
        let document = self.document_mut(&uri)?;
        document.apply_changes(params.text_document.version, &params.content_changes)?;
        if let Some(pending) = matching_cycle {
            self.pending_cycles.remove(&uri);
            self.column_state.commit(DocState {
                file_path: uri,
                version: params.text_document.version,
                selections: pending.post_selections,
            });
        }
        Ok(())
    }

    fn did_close(&mut self, params: &Value) -> Result<(), RpcError> {
        let uri = text_document_uri(params)?;
        self.documents.remove(uri);
        self.document_instances.remove(uri);
        self.document_configurations.remove(uri);
        self.pending_cycles.remove(uri);
        self.column_state.close(uri);
        self.pending_requests.retain(|_, pending| {
            !matches!(
                pending,
                PendingRequest::Configuration {
                    scope: ConfigurationScope::Document { uri: pending_uri, .. },
                    ..
                } if pending_uri == uri
            )
        });
        Ok(())
    }

    fn did_change_configuration(&mut self, params: &Value) -> Result<(), RpcError> {
        if !params.is_object() {
            return Err(RpcError::new(
                INVALID_PARAMS,
                "configuration notification params must be an object",
            ));
        }
        self.configuration_generation = self.configuration_generation.wrapping_add(1);
        self.configuration = Configuration::from_value(params);
        self.document_configurations.clear();
        self.pending_requests
            .retain(|_, pending| !matches!(pending, PendingRequest::Configuration { .. }));
        if self.client_initialized && self.client_capabilities.workspace.configuration {
            self.request_all_configurations();
        }
        Ok(())
    }

    fn format_document(&mut self, params: &Value) -> Result<Value, RpcError> {
        let uri = text_document_uri(params)?.to_owned();
        let tab_width = formatting_tab_width(params)?;
        let edit = self.wrap(&uri, None, ColumnChoice::Cycle, tab_width)?;
        Ok(Value::Array(
            edit.into_iter().map(ProtocolTextEdit::into_value).collect(),
        ))
    }

    fn format_range(&mut self, params: &Value) -> Result<Value, RpcError> {
        let uri = text_document_uri(params)?.to_owned();
        let range = parse_value::<LspRange>(
            params
                .get("range")
                .cloned()
                .ok_or_else(|| RpcError::new(INVALID_PARAMS, "range is required"))?,
        )?;
        let tab_width = formatting_tab_width(params)?;
        let edit = self.wrap(&uri, Some(range), ColumnChoice::Cycle, tab_width)?;
        Ok(Value::Array(
            edit.into_iter().map(ProtocolTextEdit::into_value).collect(),
        ))
    }

    fn format_on_type(&mut self, params: Value) -> Result<Value, RpcError> {
        let parsed: OnTypeParams = parse_value(params)?;
        if !self.auto_wrap_enabled(&parsed.text_document.uri) {
            return Ok(json!([]));
        }
        let document = self.document(&parsed.text_document.uri)?.clone();
        let configuration = self.configuration_for(&parsed.text_document.uri).clone();
        let columns = configuration.columns();
        let column = self.column_state.current(&document.uri, &columns);
        let settings = configuration
            .settings(column, Some(parsed.options.tab_size))
            .map_err(|message| RpcError::new(INVALID_PARAMS, message))?;
        let Some((new_text, insertion_start)) = on_type_change(&document, &parsed) else {
            return Ok(json!([]));
        };
        let request = WrapRequest {
            file: core_file(&document, &configuration),
            settings,
            selections: Vec::new(),
            lines: document.lines(),
        };
        let edit = maybe_auto_wrap(&request, &new_text, insertion_start.into());
        Ok(Value::Array(
            edit_to_text_edit(&document, &edit)
                .into_iter()
                .map(ProtocolTextEdit::into_value)
                .collect(),
        ))
    }

    fn code_actions(&mut self, params: &Value) -> Result<Value, RpcError> {
        let params: CodeActionParams = parse_value(params.clone())?;
        if !self.client_capabilities.code_action_literals
            || !code_action_kind_requested(params.context.only.as_deref(), "refactor.rewrite")
        {
            return Ok(json!([]));
        }
        let uri = params.text_document.uri;
        let range = params.range;
        let wrap_edit = self.wrap(&uri, Some(range), ColumnChoice::Current, None)?;
        let unwrap_edit = self.wrap(&uri, Some(range), ColumnChoice::Custom(0), None)?;
        let columns = self.configuration_for(&uri).columns();
        let mut actions = vec![json!({
            "title": "Lil Wrapper: Wrap Comment / Text",
            "kind": "refactor.rewrite",
            "edit": self.workspace_edit(
                &uri,
                wrap_edit.into_iter().map(ProtocolTextEdit::into_value).collect()
            )?
        })];
        let mut seen_columns = HashSet::new();
        for column in columns {
            if !seen_columns.insert(column) {
                continue;
            }
            let edit = self.wrap(&uri, Some(range), ColumnChoice::Custom(column), None)?;
            actions.push(json!({
                "title": format!("Lil Wrapper: Wrap at Column {column}"),
                "kind": "refactor.rewrite",
                "edit": self.workspace_edit(
                    &uri,
                    edit.into_iter().map(ProtocolTextEdit::into_value).collect()
                )?
            }));
        }
        actions.push(json!({
            "title": "Unwrap Comment / Text",
            "kind": "refactor.rewrite",
            "edit": self.workspace_edit(
                &uri,
                unwrap_edit.into_iter().map(ProtocolTextEdit::into_value).collect()
            )?
        }));
        actions.push(json!({
            "title": "Toggle Auto-Wrap for Current Document",
            "kind": "refactor.rewrite",
            "command": {
                "title": "Toggle Auto-Wrap for Current Document",
                "command": COMMAND_TOGGLE_AUTO_WRAP,
                "arguments": [{"uri": uri}]
            }
        }));
        Ok(Value::Array(actions))
    }

    fn execute_command(&mut self, params: &Value) -> Result<Value, RpcError> {
        let command = params
            .get("command")
            .and_then(Value::as_str)
            .ok_or_else(|| RpcError::new(INVALID_PARAMS, "command is required"))?;
        let arguments = match params.get("arguments") {
            None => Vec::new(),
            Some(arguments) => arguments
                .as_array()
                .cloned()
                .ok_or_else(|| RpcError::new(INVALID_PARAMS, "arguments must be an array"))?,
        };
        let target = self.command_target(&arguments)?;

        if command == COMMAND_TOGGLE_AUTO_WRAP {
            let configured = self.configuration_for(&target.uri).auto_wrap_enabled();
            let stored = self.auto_wrap_overrides.get(&target.uri).copied();
            if stored.is_none() || stored == Some(configured) {
                self.auto_wrap_overrides
                    .insert(target.uri.clone(), !configured);
            } else {
                self.auto_wrap_overrides.remove(&target.uri);
            }
            return Ok(json!({"enabled": self.auto_wrap_enabled(&target.uri)}));
        }
        if !self.client_capabilities.workspace.apply_edit {
            return Err(RpcError::new(
                REQUEST_FAILED,
                "client does not support workspace/applyEdit",
            ));
        }
        if target.range.is_none() {
            return Err(RpcError::new(
                INVALID_PARAMS,
                "lil-wrapper commands require an active editor range",
            ));
        }

        let choice = match command {
            COMMAND_WRAP => ColumnChoice::Cycle,
            COMMAND_WRAP_AT => ColumnChoice::Custom(target.column.ok_or_else(|| {
                RpcError::new(
                    INVALID_PARAMS,
                    "lil-wrapper.wrapCommentAt requires a numeric column argument",
                )
            })?),
            _ => {
                return Err(RpcError::new(
                    METHOD_NOT_FOUND,
                    format!("unknown command: {command}"),
                ));
            }
        };
        let edit = self.wrap(&target.uri, target.range, choice, None)?;
        let workspace_edit = self.workspace_edit(
            &target.uri,
            edit.into_iter().map(ProtocolTextEdit::into_value).collect(),
        )?;
        let cycle = (choice == ColumnChoice::Cycle)
            .then(|| self.cycle_token(&target.uri))
            .flatten();
        let key = self.queue_request(
            "workspace/applyEdit",
            json!({"label": "Lil Wrapper: Wrap Comment / Text", "edit": workspace_edit}),
            PendingRequest::ApplyEdit {
                deferred: None,
                cycle,
            },
        );
        self.last_apply_edit_request = Some(key);
        Ok(workspace_edit)
    }

    fn wrap(
        &mut self,
        uri: &str,
        range: Option<LspRange>,
        choice: ColumnChoice,
        tab_width: Option<f64>,
    ) -> Result<Option<ProtocolTextEdit>, RpcError> {
        let document = self.document(uri)?.clone();
        let configuration = self.configuration_for(uri).clone();
        let selections = range.map_or_else(Vec::new, |range| {
            vec![Selection {
                anchor: range.start.into(),
                active: range.end.into(),
            }]
        });
        let state = DocState {
            file_path: document.uri.clone(),
            version: document.version,
            selections: selections.clone(),
        };
        let columns = configuration.columns();
        let (column, _) = match choice {
            ColumnChoice::Current => {
                let column = self.column_state.current(uri, &columns);
                (column, column)
            }
            ColumnChoice::Cycle => self.column_state.begin_cycle(
                &state,
                &columns,
                self.pending_cycles.contains_key(uri),
            ),
            ColumnChoice::Custom(column) => (column, column),
        };
        let request = WrapRequest {
            file: core_file(&document, &configuration),
            settings: configuration
                .settings(column, tab_width)
                .map_err(|message| RpcError::new(INVALID_PARAMS, message))?,
            selections,
            lines: document.lines(),
        };
        let edit = wrap(&request);
        if choice == ColumnChoice::Cycle {
            if let Some(protocol_edit) = edit_to_text_edit(&document, &edit) {
                let end_line = usize::try_from(edit.end_line).map_err(|_| {
                    RpcError::new(INTERNAL_ERROR, "core returned an invalid edit range")
                })?;
                let old_lines = &request.lines[edit.start_line..=end_line];
                let post_selections = remap_selections(
                    old_lines,
                    &edit.lines,
                    edit.start_line,
                    end_line,
                    &request.selections,
                );
                let instance = *self.document_instances.get(uri).ok_or_else(|| {
                    RpcError::new(INVALID_PARAMS, format!("document is not open: {uri}"))
                })?;
                self.pending_cycles.insert(
                    uri.to_owned(),
                    PendingCycle {
                        instance,
                        source_version: document.version,
                        expected_range: protocol_edit.range,
                        expected_text: protocol_edit.new_text.clone(),
                        post_selections,
                        source_selections: request.selections.clone(),
                    },
                );
                return Ok(Some(protocol_edit));
            }
            self.column_state.commit(state);
        }
        Ok(edit_to_text_edit(&document, &edit))
    }

    fn workspace_edit(&self, uri: &str, edits: Vec<Value>) -> Result<Value, RpcError> {
        let document = self.document(uri)?;
        if self.client_capabilities.workspace.document_changes {
            let mut workspace_edit = json!({
                "documentChanges": [{
                    "textDocument": {"uri": uri, "version": document.version},
                    "edits": []
                }]
            });
            workspace_edit["documentChanges"][0]["edits"] = Value::Array(edits);
            Ok(workspace_edit)
        } else {
            let mut changes = Map::new();
            changes.insert(uri.to_owned(), Value::Array(edits));
            let mut workspace_edit = Map::new();
            workspace_edit.insert("changes".to_owned(), Value::Object(changes));
            Ok(Value::Object(workspace_edit))
        }
    }

    fn configuration_for(&self, uri: &str) -> &Configuration {
        self.document_configurations
            .get(uri)
            .filter(|scoped| {
                scoped.generation == self.configuration_generation
                    && self.document_instances.get(uri) == Some(&scoped.instance)
            })
            .map_or(&self.configuration, |scoped| &scoped.configuration)
    }

    fn auto_wrap_enabled(&self, uri: &str) -> bool {
        self.configuration_for(uri).auto_wrap_enabled() ^ self.auto_wrap_overrides.contains_key(uri)
    }

    fn document(&self, uri: &str) -> Result<&Document, RpcError> {
        self.documents
            .get(uri)
            .ok_or_else(|| RpcError::new(INVALID_PARAMS, format!("document is not open: {uri}")))
    }

    fn document_mut(&mut self, uri: &str) -> Result<&mut Document, RpcError> {
        self.documents
            .get_mut(uri)
            .ok_or_else(|| RpcError::new(INVALID_PARAMS, format!("document is not open: {uri}")))
    }

    fn request_all_configurations(&mut self) {
        self.request_configuration(ConfigurationScope::Global);
        let documents = self
            .document_instances
            .iter()
            .map(|(uri, instance)| (uri.clone(), *instance))
            .collect::<Vec<_>>();
        for (uri, instance) in documents {
            self.request_configuration(ConfigurationScope::Document { uri, instance });
        }
    }

    fn request_configuration(&mut self, scope: ConfigurationScope) {
        let scope_uri = match &scope {
            ConfigurationScope::Global => None,
            ConfigurationScope::Document { uri, .. } => Some(uri),
        };
        let item = |section: &str| {
            let mut item = json!({"section": section});
            if let Some(uri) = scope_uri {
                item["scopeUri"] = Value::String(uri.clone());
            }
            item
        };
        self.queue_request(
            "workspace/configuration",
            json!({"items": [item("lil-wrapper"), item("editor")]}),
            PendingRequest::Configuration {
                generation: self.configuration_generation,
                scope,
            },
        );
    }

    fn queue_request(&mut self, method: &str, params: Value, pending: PendingRequest) -> String {
        let id = self.next_request_id;
        self.next_request_id += 1;
        let id_value = json!(id);
        let key = request_key(&id_value);
        self.pending_requests.insert(key.clone(), pending);
        let mut request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": null
        });
        request["params"] = params;
        self.outbound_requests.push(request);
        key
    }

    fn cycle_token(&self, uri: &str) -> Option<CycleToken> {
        self.pending_cycles.get(uri).map(|pending| CycleToken {
            uri: uri.to_owned(),
            source_version: pending.source_version,
            instance: pending.instance,
        })
    }

    fn fail_cycle(&mut self, token: &CycleToken) {
        let Some(pending) = self.pending_cycles.get(&token.uri) else {
            return;
        };
        if pending.source_version != token.source_version || pending.instance != token.instance {
            return;
        }
        let source = DocState {
            file_path: token.uri.clone(),
            version: pending.source_version,
            selections: pending.source_selections.clone(),
        };
        self.pending_cycles.remove(&token.uri);
        self.column_state.commit(source);
    }

    fn command_target(&self, arguments: &[Value]) -> Result<CommandTarget, RpcError> {
        let first = arguments.first();
        let object = first.and_then(Value::as_object);
        let uri = object
            .and_then(|value| value.get("uri"))
            .and_then(Value::as_str)
            .or_else(|| {
                object
                    .and_then(|value| value.get("textDocument"))
                    .and_then(|value| value.get("uri"))
                    .and_then(Value::as_str)
            })
            .or_else(|| first.and_then(Value::as_str))
            .map(str::to_owned)
            .or_else(|| {
                (self.documents.len() == 1)
                    .then(|| self.documents.keys().next().cloned())
                    .flatten()
            })
            .ok_or_else(|| RpcError::new(INVALID_PARAMS, "command requires a document URI"))?;
        let range_value = object
            .and_then(|value| value.get("range"))
            .cloned()
            .or_else(|| arguments.get(1).filter(|value| value.is_object()).cloned());
        let range = range_value.map(parse_value).transpose()?;
        let column = object
            .and_then(|value| value.get("column"))
            .and_then(value_to_column)
            .or_else(|| arguments.iter().skip(1).find_map(value_to_column));
        Ok(CommandTarget { uri, range, column })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ColumnChoice {
    Current,
    Cycle,
    Custom(usize),
}

struct CommandTarget {
    uri: String,
    range: Option<LspRange>,
    column: Option<usize>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InitializeParams {
    capabilities: Map<String, Value>,
    initialization_options: Option<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TextDocumentItem {
    uri: String,
    language_id: String,
    version: i64,
    text: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DidOpenParams {
    text_document: TextDocumentItem,
}

#[derive(Deserialize)]
struct VersionedTextDocumentIdentifier {
    uri: String,
    version: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DidChangeParams {
    text_document: VersionedTextDocumentIdentifier,
    content_changes: Vec<ContentChange>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OnTypeParams {
    text_document: TextDocumentIdentifier,
    position: LspPosition,
    ch: String,
    options: FormattingOptions,
}

#[derive(Deserialize)]
struct TextDocumentIdentifier {
    uri: String,
}

#[derive(Deserialize)]
struct CodeActionParams {
    #[serde(rename = "textDocument")]
    text_document: TextDocumentIdentifier,
    range: LspRange,
    context: CodeActionContext,
}

#[derive(Deserialize)]
struct CodeActionContext {
    #[serde(rename = "diagnostics")]
    _diagnostics: Vec<Value>,
    only: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FormattingOptions {
    tab_size: f64,
    #[serde(rename = "insertSpaces")]
    _insert_spaces: bool,
}

#[derive(Clone, Debug)]
struct ProtocolTextEdit {
    range: LspRange,
    new_text: String,
}

impl ProtocolTextEdit {
    fn into_value(self) -> Value {
        json!({
            "range": range_value(self.range),
            "newText": self.new_text
        })
    }
}

fn core_file(document: &Document, configuration: &Configuration) -> File {
    File {
        language: document.language.clone(),
        path: document.uri.clone(),
        custom_markers: configuration.custom_markers.clone(),
    }
}

fn edit_to_text_edit(document: &Document, edit: &Edit) -> Option<ProtocolTextEdit> {
    if edit.is_empty() {
        return None;
    }
    let end_line = usize::try_from(edit.end_line).ok()?;
    let end_character = document.line_utf16_len(end_line)?;
    Some(ProtocolTextEdit {
        range: LspRange {
            start: LspPosition {
                line: edit.start_line,
                character: 0,
            },
            end: LspPosition {
                line: end_line,
                character: end_character,
            },
        },
        new_text: edit.lines.join(document.preferred_eol()),
    })
}

fn on_type_change(document: &Document, params: &OnTypeParams) -> Option<(String, LspPosition)> {
    if let Some(batch) = &document.last_change_batch
        && let Some(change) = &batch.separate_newline_indent
        && params.ch == "\n"
        && change.end == params.position
    {
        return Some((change.text.clone(), change.start));
    }
    if let Some(batch) = &document.last_change_batch
        && batch.change_count == 1
        && let Some(change) = &batch.insertion
        && change.end == params.position
        && !change.text.is_empty()
        && change.text.chars().all(char::is_whitespace)
    {
        return Some((change.text.clone(), change.start));
    }
    None
}

fn text_document_uri(params: &Value) -> Result<&str, RpcError> {
    params
        .pointer("/textDocument/uri")
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError::new(INVALID_PARAMS, "textDocument.uri is required"))
}

fn formatting_tab_width(params: &Value) -> Result<Option<f64>, RpcError> {
    let options = params
        .get("options")
        .cloned()
        .ok_or_else(|| RpcError::new(INVALID_PARAMS, "options are required"))?;
    let options: FormattingOptions = parse_value(options)?;
    Ok(Some(options.tab_size))
}

fn code_action_kind_requested(only: Option<&[String]>, action_kind: &str) -> bool {
    only.is_none_or(|kinds| {
        kinds
            .iter()
            .any(|kind| code_action_kind_matches(kind, action_kind))
    })
}

fn code_action_kind_matches(requested: &str, action_kind: &str) -> bool {
    action_kind == requested
        || action_kind
            .strip_prefix(requested)
            .is_some_and(|suffix| suffix.starts_with('.'))
}

fn change_matches_cycle(changes: &[ContentChange], pending: &PendingCycle) -> bool {
    let [change] = changes else {
        return false;
    };
    change.range == Some(pending.expected_range) && change.text == pending.expected_text
}

fn apply_edit_failure(result: Option<&Value>, error: Option<Value>) -> Option<String> {
    if let Some(error) = error {
        return Some(
            error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("client rejected workspace/applyEdit")
                .to_owned(),
        );
    }
    let Some(result) = result else {
        return Some("client returned no workspace/applyEdit result".to_owned());
    };
    if result.get("applied").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    Some(
        result
            .get("failureReason")
            .and_then(Value::as_str)
            .unwrap_or("client did not apply the workspace edit")
            .to_owned(),
    )
}

fn parse_params<T>(value: Value) -> Result<T, RpcError>
where
    T: for<'de> Deserialize<'de>,
{
    parse_value(value)
}

fn parse_value<T>(value: Value) -> Result<T, RpcError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value(value)
        .map_err(|error| RpcError::new(INVALID_PARAMS, format!("invalid parameters: {error}")))
}

fn range_value(range: LspRange) -> Value {
    json!({
        "start": {"line": range.start.line, "character": range.start.character},
        "end": {"line": range.end.line, "character": range.end.character}
    })
}

fn value_to_column(value: &Value) -> Option<usize> {
    if let Some(column) = value.as_u64() {
        return usize::try_from(column).ok();
    }
    value
        .as_i64()
        .map(|column| usize::try_from(column).unwrap_or(0))
}

fn request_key(id: &Value) -> String {
    match id {
        Value::String(id) => format!("s:{id}"),
        Value::Number(id) => format!("n:{id}"),
        Value::Null => "null".to_owned(),
        _ => format!("invalid:{id}"),
    }
}

#[cfg(test)]
mod tests {
    use super::LanguageServer;
    use crate::{CONTENT_MODIFIED, INVALID_PARAMS};
    use serde_json::json;

    fn initialized_server(uri: &str, version: i64, text: &str) -> LanguageServer {
        let mut server = LanguageServer::new();
        server
            .request(
                "initialize",
                json!({"capabilities": {
                    "workspace": {
                        "applyEdit": true,
                        "workspaceEdit": {"documentChanges": true}
                    },
                    "textDocument": {"codeAction": {
                        "codeActionLiteralSupport": {
                            "codeActionKind": {"valueSet": ["refactor.rewrite"]}
                        }
                    }}
                }}),
            )
            .expect("initialize");
        server
            .notify(
                "textDocument/didOpen",
                json!({"textDocument": {
                    "uri": uri, "languageId": "plaintext", "version": version, "text": text
                }}),
            )
            .expect("open");
        server
    }

    fn formatting(server: &mut LanguageServer, uri: &str) -> serde_json::Value {
        server
            .request(
                "textDocument/formatting",
                json!({
                    "textDocument": {"uri": uri},
                    "options": {"tabSize": 4, "insertSpaces": true}
                }),
            )
            .expect("formatting")
    }

    fn enable_auto_wrap(server: &mut LanguageServer) {
        server
            .notify(
                "workspace/didChangeConfiguration",
                json!({"settings": {"lil-wrapper": {
                    "wrappingColumn": 8,
                    "autoWrap": {"enabled": true}
                }}}),
            )
            .expect("configuration");
    }

    #[test]
    fn stale_did_change_is_rejected() {
        let mut server = initialized_server("file:///a.txt", 2, "one two");
        let error = server
            .notify(
                "textDocument/didChange",
                json!({
                    "textDocument": {"uri": "file:///a.txt", "version": 2},
                    "contentChanges": [{"text": "replacement"}]
                }),
            )
            .expect_err("stale change");

        assert_eq!(error.code, CONTENT_MODIFIED);
    }

    #[test]
    fn cycles_rulers_only_after_the_first_edit_is_synchronized() {
        let uri = "file:///rulers.txt";
        let mut server = initialized_server(uri, 1, "one two three four");
        server
            .notify(
                "workspace/didChangeConfiguration",
                json!({"settings": {
                    "lil-wrapper": {"wrappingColumn": 0},
                    "editor": {"rulers": [8, {"column": 12}], "wordWrapColumn": 80}
                }}),
            )
            .expect("configuration");

        let first = formatting(&mut server, uri);
        let unapplied = formatting(&mut server, uri);
        server
            .notify(
                "textDocument/didChange",
                json!({
                    "textDocument": {"uri": uri, "version": 2},
                    "contentChanges": [{
                        "range": first[0]["range"].clone(),
                        "rangeLength": 18,
                        "text": first[0]["newText"].clone()
                    }]
                }),
            )
            .expect("first formatting edit synchronized");
        let cycled = formatting(&mut server, uri);

        assert_eq!(first[0]["newText"], "one two\nthree\nfour");
        assert_eq!(unapplied[0]["newText"], first[0]["newText"]);
        assert_eq!(cycled[0]["range"]["start"]["line"], 1);
        assert_eq!(cycled[0]["newText"], "three four");
    }

    #[test]
    fn on_type_formatting_obeys_the_document_auto_wrap_toggle() {
        let uri = "file:///auto.txt";
        let mut server = initialized_server(uri, 1, "one two three");
        enable_auto_wrap(&mut server);
        server
            .notify(
                "textDocument/didChange",
                json!({
                    "textDocument": {"uri": uri, "version": 2},
                    "contentChanges": [{
                        "range": {
                            "start": {"line": 0, "character": 13},
                            "end": {"line": 0, "character": 13}
                        },
                        "rangeLength": 0,
                        "text": " "
                    }]
                }),
            )
            .expect("typing synchronized");
        let params = json!({
            "textDocument": {"uri": uri},
            "position": {"line": 0, "character": 14},
            "ch": " ",
            "options": {"tabSize": 4, "insertSpaces": true}
        });

        let enabled = server
            .request("textDocument/onTypeFormatting", params.clone())
            .expect("on-type formatting");
        let toggle = server
            .request(
                "workspace/executeCommand",
                json!({
                    "command": "lil-wrapper.toggleAutoWrap",
                    "arguments": [{"uri": uri}]
                }),
            )
            .expect("toggle");
        let disabled = server
            .request("textDocument/onTypeFormatting", params)
            .expect("disabled on-type formatting");

        assert_eq!(enabled[0]["newText"], "one two\nthree ");
        assert_eq!(toggle["enabled"], false);
        assert_eq!(disabled, json!([]));
    }

    #[test]
    fn on_type_formatting_rejects_ranged_replacements() {
        let uri = "file:///replacement.txt";
        let mut server = initialized_server(uri, 1, "one two threex");
        enable_auto_wrap(&mut server);
        server
            .notify(
                "textDocument/didChange",
                json!({
                    "textDocument": {"uri": uri, "version": 2},
                    "contentChanges": [{
                        "range": {
                            "start": {"line": 0, "character": 13},
                            "end": {"line": 0, "character": 14}
                        },
                        "rangeLength": 1,
                        "text": " "
                    }]
                }),
            )
            .expect("replacement synchronized");

        let edits = server
            .request(
                "textDocument/onTypeFormatting",
                json!({
                    "textDocument": {"uri": uri},
                    "position": {"line": 0, "character": 14},
                    "ch": " ",
                    "options": {"tabSize": 4, "insertSpaces": true}
                }),
            )
            .expect("on-type formatting");

        assert_eq!(edits, json!([]));
    }

    #[test]
    fn on_type_formatting_rejects_multi_change_batches() {
        let uri = "file:///multiple.txt";
        let mut server = initialized_server(uri, 1, "one two three");
        enable_auto_wrap(&mut server);
        server
            .notify(
                "textDocument/didChange",
                json!({
                    "textDocument": {"uri": uri, "version": 2},
                    "contentChanges": [
                        {
                            "range": {
                                "start": {"line": 0, "character": 13},
                                "end": {"line": 0, "character": 13}
                            },
                            "rangeLength": 0,
                            "text": " "
                        },
                        {
                            "range": {
                                "start": {"line": 0, "character": 0},
                                "end": {"line": 0, "character": 0}
                            },
                            "rangeLength": 0,
                            "text": ""
                        }
                    ]
                }),
            )
            .expect("changes synchronized");

        let edits = server
            .request(
                "textDocument/onTypeFormatting",
                json!({
                    "textDocument": {"uri": uri},
                    "position": {"line": 0, "character": 14},
                    "ch": " ",
                    "options": {"tabSize": 4, "insertSpaces": true}
                }),
            )
            .expect("on-type formatting");

        assert_eq!(edits, json!([]));
    }

    #[test]
    fn on_type_formatting_combines_separate_newline_and_indent_changes() {
        let uri = "file:///newline.txt";
        let mut server = initialized_server(uri, 1, "one two three");
        enable_auto_wrap(&mut server);
        server
            .notify(
                "textDocument/didChange",
                json!({
                    "textDocument": {"uri": uri, "version": 2},
                    "contentChanges": [{
                        "range": {
                            "start": {"line": 0, "character": 13},
                            "end": {"line": 0, "character": 13}
                        },
                        "rangeLength": 0,
                        "text": "\n"
                    }]
                }),
            )
            .expect("newline synchronized");
        server
            .notify(
                "textDocument/didChange",
                json!({
                    "textDocument": {"uri": uri, "version": 3},
                    "contentChanges": [{
                        "range": {
                            "start": {"line": 1, "character": 0},
                            "end": {"line": 1, "character": 0}
                        },
                        "rangeLength": 0,
                        "text": "  "
                    }]
                }),
            )
            .expect("indent synchronized");

        let edits = server
            .request(
                "textDocument/onTypeFormatting",
                json!({
                    "textDocument": {"uri": uri},
                    "position": {"line": 1, "character": 2},
                    "ch": "\n",
                    "options": {"tabSize": 4, "insertSpaces": true}
                }),
            )
            .expect("on-type formatting");

        assert_eq!(edits[0]["newText"], "one two\nthree");
    }

    #[test]
    fn custom_column_commands_emit_versioned_apply_edits() {
        let uri = "file:///command.txt";
        let mut server = initialized_server(uri, 7, "one two three four");

        let edit = server
            .request(
                "workspace/executeCommand",
                json!({
                    "command": "lil-wrapper.wrapCommentAt",
                    "arguments": [{
                        "uri": uri,
                        "column": 8,
                        "range": {
                            "start": {"line": 0, "character": 0},
                            "end": {"line": 0, "character": 18}
                        }
                    }]
                }),
            )
            .expect("custom-column command");
        let outbound = server.take_outbound_requests();

        assert_eq!(edit["documentChanges"][0]["textDocument"]["version"], 7);
        assert_eq!(
            edit["documentChanges"][0]["edits"][0]["newText"],
            "one two\nthree\nfour"
        );
        assert_eq!(outbound[0]["method"], "workspace/applyEdit");
        assert_eq!(outbound[0]["params"]["edit"], edit);
    }

    #[test]
    fn scoped_configuration_responses_override_global_settings() {
        let uri = "file:///scoped.txt";
        let mut server = LanguageServer::new();
        server
            .request(
                "initialize",
                json!({"capabilities": {"workspace": {"configuration": true}}}),
            )
            .expect("initialize");
        server
            .notify("initialized", json!({}))
            .expect("initialized");
        server
            .notify(
                "textDocument/didOpen",
                json!({"textDocument": {
                    "uri": uri, "languageId": "plaintext", "version": 1,
                    "text": "one two three"
                }}),
            )
            .expect("open");
        let requests = server.take_outbound_requests();
        let scoped = requests
            .iter()
            .find(|request| request["params"]["items"][0]["scopeUri"] == uri)
            .expect("scoped request");
        server
            .client_response(
                &scoped["id"],
                Some(json!([{"wrappingColumn": 8}, {"wordWrapColumn": 80}])),
                None,
            )
            .expect("configuration response");

        let edits = formatting(&mut server, uri);

        assert_eq!(edits[0]["newText"], "one two\nthree");
    }

    #[test]
    fn obsolete_configuration_generations_are_ignored_when_responses_reverse() {
        let uri = "file:///generation.txt";
        let mut server = LanguageServer::new();
        server
            .request(
                "initialize",
                json!({"capabilities": {"workspace": {"configuration": true}}}),
            )
            .expect("initialize");
        server
            .notify("initialized", json!({}))
            .expect("initialized");
        server
            .notify(
                "textDocument/didOpen",
                json!({"textDocument": {
                    "uri": uri, "languageId": "plaintext", "version": 1,
                    "text": "one two three"
                }}),
            )
            .expect("open");
        let old_requests = server.take_outbound_requests();
        let old_scoped_id = old_requests
            .iter()
            .find(|request| request["params"]["items"][0]["scopeUri"] == uri)
            .expect("old scoped request")["id"]
            .clone();
        server
            .notify(
                "workspace/didChangeConfiguration",
                json!({"settings": {"lil-wrapper": {"wrappingColumn": 10}}}),
            )
            .expect("configuration change");
        let new_requests = server.take_outbound_requests();
        assert!(
            new_requests
                .iter()
                .any(|request| request["params"]["items"][0].get("scopeUri").is_none())
        );
        let new_scoped_id = new_requests
            .iter()
            .find(|request| request["params"]["items"][0]["scopeUri"] == uri)
            .expect("new scoped request")["id"]
            .clone();

        server
            .client_response(
                &new_scoped_id,
                Some(json!([{"wrappingColumn": 8}, {"wordWrapColumn": 80}])),
                None,
            )
            .expect("new response");
        server
            .client_response(
                &old_scoped_id,
                Some(json!([{"wrappingColumn": 12}, {"wordWrapColumn": 80}])),
                None,
            )
            .expect("obsolete response ignored");

        let edits = formatting(&mut server, uri);

        assert_eq!(edits[0]["newText"], "one two\nthree");
    }

    #[test]
    fn close_and_reopen_rejects_the_previous_document_configuration_instance() {
        let uri = "file:///reopen.txt";
        let mut server = LanguageServer::new();
        server
            .request(
                "initialize",
                json!({"capabilities": {"workspace": {"configuration": true}}}),
            )
            .expect("initialize");
        server
            .notify("initialized", json!({}))
            .expect("initialized");
        server
            .notify(
                "textDocument/didOpen",
                json!({"textDocument": {
                    "uri": uri, "languageId": "plaintext", "version": 1,
                    "text": "one two three"
                }}),
            )
            .expect("first open");
        let first_requests = server.take_outbound_requests();
        let first_id = first_requests
            .iter()
            .find(|request| request["params"]["items"][0]["scopeUri"] == uri)
            .expect("first scoped request")["id"]
            .clone();
        server
            .notify(
                "textDocument/didClose",
                json!({"textDocument": {"uri": uri}}),
            )
            .expect("close");
        server
            .notify(
                "textDocument/didOpen",
                json!({"textDocument": {
                    "uri": uri, "languageId": "plaintext", "version": 1,
                    "text": "one two three"
                }}),
            )
            .expect("reopen");
        let second_requests = server.take_outbound_requests();
        let second_id = second_requests[0]["id"].clone();

        server
            .client_response(
                &first_id,
                Some(json!([{"wrappingColumn": 12}, {"wordWrapColumn": 80}])),
                None,
            )
            .expect("closed response ignored");
        server
            .client_response(
                &second_id,
                Some(json!([{"wrappingColumn": 8}, {"wordWrapColumn": 80}])),
                None,
            )
            .expect("reopened response");

        let edits = formatting(&mut server, uri);

        assert_eq!(edits[0]["newText"], "one two\nthree");
    }

    #[test]
    fn production_ruler_shapes_distinguish_detailed_and_numeric_zero() {
        let uri = "file:///zero-ruler.txt";
        let mut server = initialized_server(uri, 1, "one two\nthree");
        let action_params = json!({
            "textDocument": {"uri": uri},
            "range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 1, "character": 5}
            },
            "context": {"diagnostics": []}
        });
        server
            .notify(
                "workspace/didChangeConfiguration",
                json!({"settings": {
                    "lil-wrapper": {"wrappingColumn": 0},
                    "editor": {"rulers": [0], "wordWrapColumn": 8}
                }}),
            )
            .expect("numeric ruler configuration");
        let numeric = server
            .request("textDocument/codeAction", action_params.clone())
            .expect("numeric actions");
        server
            .notify(
                "workspace/didChangeConfiguration",
                json!({"settings": {
                    "lil-wrapper": {"wrappingColumn": 0},
                    "editor": {"rulers": [{"column": 0}], "wordWrapColumn": 8}
                }}),
            )
            .expect("detailed ruler configuration");
        let detailed = server
            .request("textDocument/codeAction", action_params)
            .expect("detailed actions");

        assert!(
            numeric
                .as_array()
                .expect("numeric action array")
                .iter()
                .any(|action| action["title"] == "Lil Wrapper: Wrap at Column 8")
        );
        let zero = detailed
            .as_array()
            .expect("detailed action array")
            .iter()
            .find(|action| action["title"] == "Lil Wrapper: Wrap at Column 0")
            .expect("unbounded ruler action");
        assert_eq!(
            zero["edit"]["documentChanges"][0]["edits"][0]["newText"],
            "one two three"
        );
    }

    #[test]
    fn scoped_configuration_responses_preserve_detailed_zero_rulers() {
        let uri = "file:///scoped-zero.txt";
        let mut server = LanguageServer::new();
        server
            .request(
                "initialize",
                json!({"capabilities": {
                    "workspace": {
                        "configuration": true,
                        "workspaceEdit": {"documentChanges": true}
                    },
                    "textDocument": {"codeAction": {
                        "codeActionLiteralSupport": {
                            "codeActionKind": {"valueSet": ["refactor.rewrite"]}
                        }
                    }}
                }}),
            )
            .expect("initialize");
        server
            .notify("initialized", json!({}))
            .expect("initialized");
        server
            .notify(
                "textDocument/didOpen",
                json!({"textDocument": {
                    "uri": uri, "languageId": "plaintext", "version": 1,
                    "text": "one two\nthree"
                }}),
            )
            .expect("open");
        let requests = server.take_outbound_requests();
        let scoped_id = requests
            .iter()
            .find(|request| request["params"]["items"][0]["scopeUri"] == uri)
            .expect("scoped request")["id"]
            .clone();
        server
            .client_response(
                &scoped_id,
                Some(json!([
                    {"wrappingColumn": 0},
                    {"rulers": [{"column": 0}], "wordWrapColumn": 8}
                ])),
                None,
            )
            .expect("scoped response");

        let actions = server
            .request(
                "textDocument/codeAction",
                json!({
                    "textDocument": {"uri": uri},
                    "range": {
                        "start": {"line": 0, "character": 0},
                        "end": {"line": 1, "character": 5}
                    },
                    "context": {"diagnostics": []}
                }),
            )
            .expect("actions");

        assert!(
            actions
                .as_array()
                .expect("action array")
                .iter()
                .any(|action| action["title"] == "Lil Wrapper: Wrap at Column 0")
        );
    }

    #[test]
    fn closing_a_document_discards_synchronized_content() {
        let uri = "file:///closed.txt";
        let mut server = initialized_server(uri, 1, "one two three");
        server
            .notify(
                "textDocument/didClose",
                json!({"textDocument": {"uri": uri}}),
            )
            .expect("close");

        let error = server
            .request(
                "textDocument/formatting",
                json!({
                    "textDocument": {"uri": uri},
                    "options": {"tabSize": 4, "insertSpaces": true}
                }),
            )
            .expect_err("closed document");

        assert_eq!(error.code, INVALID_PARAMS);
    }
}
