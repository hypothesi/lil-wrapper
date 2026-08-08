use crate::{INVALID_REQUEST, LanguageServer, RpcError};
use serde_json::{Value, json};
use std::fmt;
use std::io::{self, BufRead, Write};

const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_CONTENT_BYTES: usize = 64 * 1024 * 1024;
const PARSE_ERROR: i64 = -32_700;

#[derive(Debug)]
pub enum FramingError {
    Io(io::Error),
    InvalidHeader(String),
    UnsupportedContentType(String),
    UnsupportedCharset(String),
    InvalidJson(String),
}

impl fmt::Display for FramingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::InvalidHeader(message) => write!(formatter, "invalid LSP header: {message}"),
            Self::UnsupportedContentType(content_type) => {
                write!(formatter, "unsupported Content-Type: {content_type}")
            }
            Self::UnsupportedCharset(charset) => {
                write!(formatter, "unsupported Content-Type charset: {charset}")
            }
            Self::InvalidJson(message) => write!(formatter, "invalid JSON content: {message}"),
        }
    }
}

impl std::error::Error for FramingError {}

impl From<io::Error> for FramingError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Reads one Content-Length-framed LSP JSON message.
///
/// # Errors
///
/// Returns a framing error for I/O failures, malformed or oversized headers,
/// unsupported content metadata, and invalid JSON content.
pub fn read_message<R: BufRead>(reader: &mut R) -> Result<Option<Value>, FramingError> {
    let mut content_length = None;
    let mut content_type = None;
    let mut header_bytes = 0;
    let mut saw_header = false;

    loop {
        let mut line = Vec::new();
        let bytes_read = reader.read_until(b'\n', &mut line)?;
        if bytes_read == 0 {
            if saw_header {
                return Err(FramingError::InvalidHeader(
                    "unexpected end of input".to_owned(),
                ));
            }
            return Ok(None);
        }
        saw_header = true;
        header_bytes += bytes_read;
        if header_bytes > MAX_HEADER_BYTES {
            return Err(FramingError::InvalidHeader(
                "header exceeds 16 KiB".to_owned(),
            ));
        }
        if !line.ends_with(b"\r\n") {
            return Err(FramingError::InvalidHeader(
                "header lines must end with CRLF".to_owned(),
            ));
        }
        line.truncate(line.len() - 2);
        if line.is_empty() {
            break;
        }
        if !line.is_ascii() {
            return Err(FramingError::InvalidHeader(
                "headers must be ASCII".to_owned(),
            ));
        }
        let line = String::from_utf8(line)
            .map_err(|_| FramingError::InvalidHeader("headers must be valid ASCII".to_owned()))?;
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| FramingError::InvalidHeader("header field has no colon".to_owned()))?;
        let name = name.trim();
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            if content_length.is_some() {
                return Err(FramingError::InvalidHeader(
                    "duplicate Content-Length".to_owned(),
                ));
            }
            let length = value.parse::<usize>().map_err(|_| {
                FramingError::InvalidHeader("Content-Length is not an integer".to_owned())
            })?;
            if length > MAX_CONTENT_BYTES {
                return Err(FramingError::InvalidHeader(
                    "content exceeds 64 MiB".to_owned(),
                ));
            }
            content_length = Some(length);
        } else if name.eq_ignore_ascii_case("content-type")
            && content_type.replace(value.to_owned()).is_some()
        {
            return Err(FramingError::InvalidHeader(
                "duplicate Content-Type".to_owned(),
            ));
        }
    }

    let content_length = content_length
        .ok_or_else(|| FramingError::InvalidHeader("missing Content-Length".to_owned()))?;
    let mut content = vec![0; content_length];
    reader.read_exact(&mut content)?;
    if let Some(content_type) = content_type {
        validate_content_type(&content_type)?;
    }
    serde_json::from_slice(&content)
        .map(Some)
        .map_err(|error| FramingError::InvalidJson(error.to_string()))
}

/// Writes one byte-counted LSP JSON message and flushes the writer.
///
/// # Errors
///
/// Returns an I/O error if serialization or writing fails.
pub fn write_message<W: Write>(writer: &mut W, message: &Value) -> io::Result<()> {
    let content = serde_json::to_vec(message).map_err(io::Error::other)?;
    write!(
        writer,
        "Content-Length: {}\r\nContent-Type: application/vscode-jsonrpc; charset=utf-8\r\n\r\n",
        content.len()
    )?;
    writer.write_all(&content)?;
    writer.flush()
}

/// Dispatches a decoded JSON-RPC message to the in-process server.
pub fn dispatch_message(server: &mut LanguageServer, message: &Value) -> Option<Value> {
    let Some(object) = message.as_object() else {
        return Some(error_response(
            &Value::Null,
            INVALID_REQUEST,
            "invalid request",
        ));
    };
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Some(error_response(
            &Value::Null,
            INVALID_REQUEST,
            "jsonrpc must be \"2.0\"",
        ));
    }

    if let Some(method) = object.get("method") {
        let Some(method) = method.as_str() else {
            return Some(error_response(
                &Value::Null,
                INVALID_REQUEST,
                "method must be a string",
            ));
        };
        let params = object.get("params").cloned().unwrap_or(Value::Null);
        if let Some(id) = object.get("id") {
            if !valid_id(id) {
                return Some(error_response(
                    &Value::Null,
                    INVALID_REQUEST,
                    "request id must be a string, number, or null",
                ));
            }
            server.prepare_transport_request();
            return match server.request(method, params) {
                Ok(result)
                    if method == "workspace/executeCommand"
                        && server.defer_last_apply_edit_response(id, result.clone()) =>
                {
                    None
                }
                Ok(result) => Some(json!({"jsonrpc": "2.0", "id": id, "result": result})),
                Err(error) => Some(rpc_error_response(id, &error)),
            };
        }
        let _ = server.notify(method, params);
        return None;
    }

    if let Some(id) = object.get("id")
        && valid_id(id)
        && (object.contains_key("result") || object.contains_key("error"))
    {
        let _ = server.client_response(
            id,
            object.get("result").cloned(),
            object.get("error").cloned(),
        );
        return None;
    }

    Some(error_response(
        &Value::Null,
        INVALID_REQUEST,
        "invalid request envelope",
    ))
}

/// Runs the synchronous stdio-compatible JSON-RPC server until EOF or `exit`.
///
/// # Errors
///
/// Returns a framing error when input cannot be framed or output cannot be
/// written. Malformed JSON bodies receive a parse-error response and do not
/// stop the loop.
pub fn run_server<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
) -> Result<i32, FramingError> {
    let mut server = LanguageServer::new();
    loop {
        let message = match read_message(reader) {
            Ok(Some(message)) => message,
            Ok(None) => break,
            Err(FramingError::InvalidJson(message)) => {
                write_message(writer, &error_response(&Value::Null, PARSE_ERROR, &message))?;
                continue;
            }
            Err(error) => return Err(error),
        };
        if let Some(response) = dispatch_message(&mut server, &message) {
            write_message(writer, &response)?;
        }
        for request in server.take_outbound_requests() {
            write_message(writer, &request)?;
        }
        if server.should_exit() {
            break;
        }
    }
    Ok(server.exit_code())
}

fn validate_content_type(content_type: &str) -> Result<(), FramingError> {
    let mut parts = content_type.split(';');
    let media_type = parts.next().unwrap_or_default().trim();
    if !media_type.eq_ignore_ascii_case("application/vscode-jsonrpc") {
        return Err(FramingError::UnsupportedContentType(
            content_type.to_owned(),
        ));
    }
    for parameter in parts {
        let Some((name, value)) = parameter.split_once('=') else {
            continue;
        };
        if name.trim().eq_ignore_ascii_case("charset") {
            let charset = value.trim().trim_matches('"');
            if !charset.eq_ignore_ascii_case("utf-8") && !charset.eq_ignore_ascii_case("utf8") {
                return Err(FramingError::UnsupportedCharset(charset.to_owned()));
            }
        }
    }
    Ok(())
}

fn valid_id(id: &Value) -> bool {
    id.is_string() || id.is_number() || id.is_null()
}

fn rpc_error_response(id: &Value, error: &RpcError) -> Value {
    error_response(id, error.code, &error.message)
}

fn error_response(id: &Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": code, "message": message}
    })
}

#[cfg(test)]
mod tests {
    use super::{
        FramingError, dispatch_message, read_message, run_server, validate_content_type,
        write_message,
    };
    use crate::{INVALID_REQUEST, LanguageServer};
    use serde_json::{Value, json};
    use std::io::{BufReader, Cursor};

    fn framed_command_session(apply_response: Value) -> Vec<Value> {
        let uri = "file:///command.txt";
        let mut input = Vec::new();
        for message in [
            json!({
                "jsonrpc": "2.0", "id": 10, "method": "initialize",
                "params": {"capabilities": {"workspace": {
                    "applyEdit": true,
                    "workspaceEdit": {"documentChanges": true}
                }}}
            }),
            json!({
                "jsonrpc": "2.0", "method": "textDocument/didOpen",
                "params": {"textDocument": {
                    "uri": uri, "languageId": "plaintext", "version": 1,
                    "text": "one two three four"
                }}
            }),
            json!({
                "jsonrpc": "2.0", "id": 20, "method": "workspace/executeCommand",
                "params": {
                    "command": "rewrap.rewrapCommentAt",
                    "arguments": [{
                        "uri": uri,
                        "column": 8,
                        "range": {
                            "start": {"line": 0, "character": 0},
                            "end": {"line": 0, "character": 18}
                        }
                    }]
                }
            }),
            apply_response,
            json!({"jsonrpc": "2.0", "id": 30, "method": "shutdown"}),
            json!({"jsonrpc": "2.0", "method": "exit"}),
        ] {
            write_message(&mut input, &message).expect("input frame");
        }
        let mut reader = BufReader::new(Cursor::new(input));
        let mut output = Vec::new();
        assert_eq!(
            run_server(&mut reader, &mut output).expect("command session"),
            0
        );
        let mut responses = BufReader::new(Cursor::new(output));
        let mut messages = Vec::new();
        while let Some(message) = read_message(&mut responses).expect("output frame") {
            messages.push(message);
        }
        messages
    }

    #[test]
    fn reads_case_insensitive_headers_and_utf8_alias() {
        let content = br#"{"jsonrpc":"2.0","method":"exit"}"#;
        let input = format!(
            "content-length: {}\r\nCONTENT-TYPE: Application/Vscode-Jsonrpc; Charset=UTF8\r\n\r\n",
            content.len()
        );
        let mut bytes = input.into_bytes();
        bytes.extend_from_slice(content);
        let mut reader = BufReader::new(Cursor::new(bytes));

        let message = read_message(&mut reader)
            .expect("valid frame")
            .expect("message");

        assert_eq!(message["method"], "exit");
    }

    #[test]
    fn rejects_unsupported_charsets() {
        let error = validate_content_type("application/vscode-jsonrpc; charset=iso-8859-1")
            .expect_err("unsupported charset");

        assert!(matches!(error, FramingError::UnsupportedCharset(_)));
    }

    #[test]
    fn rejects_frames_without_content_length() {
        let input = b"Content-Type: application/vscode-jsonrpc\r\n\r\n{}";
        let mut reader = BufReader::new(Cursor::new(input));

        let error = read_message(&mut reader).expect_err("missing content length");

        assert!(matches!(error, FramingError::InvalidHeader(_)));
    }

    #[test]
    fn writes_byte_accurate_content_lengths() {
        let mut output = Vec::new();
        write_message(&mut output, &json!({"text": "😀"})).expect("write message");
        let mut reader = BufReader::new(Cursor::new(output));

        let message = read_message(&mut reader)
            .expect("valid frame")
            .expect("message");

        assert_eq!(message, json!({"text": "😀"}));
    }

    #[test]
    fn malformed_envelopes_return_invalid_request() {
        let response = dispatch_message(
            &mut LanguageServer::new(),
            &json!({"jsonrpc": "2.0", "id": 1}),
        )
        .expect("error response");

        assert_eq!(response["id"], Value::Null);
        assert_eq!(response["error"]["code"], INVALID_REQUEST);
    }

    #[test]
    fn malformed_json_does_not_prevent_the_next_message() {
        let invalid = b"{";
        let exit = br#"{"jsonrpc":"2.0","method":"exit"}"#;
        let mut input = format!("Content-Length: {}\r\n\r\n", invalid.len()).into_bytes();
        input.extend_from_slice(invalid);
        input.extend_from_slice(format!("Content-Length: {}\r\n\r\n", exit.len()).as_bytes());
        input.extend_from_slice(exit);
        let mut reader = BufReader::new(Cursor::new(input));
        let mut output = Vec::new();

        let code = run_server(&mut reader, &mut output).expect("server handles parse error");

        assert_eq!(code, 1);
        let mut responses = BufReader::new(Cursor::new(output));
        let response = read_message(&mut responses)
            .expect("valid response")
            .expect("parse error response");
        assert_eq!(response["error"]["code"], -32_700);
    }

    #[test]
    fn framed_server_runs_the_initialize_shutdown_exit_lifecycle() {
        let mut input = Vec::new();
        write_message(
            &mut input,
            &json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {"capabilities": {}}
            }),
        )
        .expect("initialize frame");
        write_message(
            &mut input,
            &json!({"jsonrpc": "2.0", "id": 2, "method": "shutdown"}),
        )
        .expect("shutdown frame");
        write_message(&mut input, &json!({"jsonrpc": "2.0", "method": "exit"}))
            .expect("exit frame");
        let mut reader = BufReader::new(Cursor::new(input));
        let mut output = Vec::new();

        let code = run_server(&mut reader, &mut output).expect("clean lifecycle");

        assert_eq!(code, 0);
        let mut responses = BufReader::new(Cursor::new(output));
        let initialize = read_message(&mut responses)
            .expect("initialize response frame")
            .expect("initialize response");
        let shutdown = read_message(&mut responses)
            .expect("shutdown response frame")
            .expect("shutdown response");
        assert_eq!(initialize["id"], 1);
        assert_eq!(
            initialize["result"]["capabilities"]["positionEncoding"],
            "utf-16"
        );
        assert_eq!(shutdown, json!({"jsonrpc": "2.0", "id": 2, "result": null}));
        assert!(
            read_message(&mut responses)
                .expect("response EOF")
                .is_none()
        );
    }

    #[test]
    fn framed_commands_wait_for_successful_apply_edit_responses() {
        let messages = framed_command_session(json!({
            "jsonrpc": "2.0", "id": 1, "result": {"applied": true}
        }));

        assert_eq!(messages[0]["id"], 10);
        assert_eq!(messages[1]["method"], "workspace/applyEdit");
        assert_eq!(messages[1]["id"], 1);
        assert_eq!(messages[2]["id"], 20);
        assert!(messages[2].get("result").is_some());
        assert_eq!(messages[3]["id"], 30);
    }

    #[test]
    fn framed_commands_return_errors_when_apply_edit_fails() {
        let messages = framed_command_session(json!({
            "jsonrpc": "2.0", "id": 1,
            "result": {"applied": false, "failureReason": "document changed"}
        }));

        assert_eq!(messages[1]["method"], "workspace/applyEdit");
        assert_eq!(messages[2]["id"], 20);
        assert_eq!(messages[2]["error"]["code"], -32_803);
        assert_eq!(messages[2]["error"]["message"], "document changed");
    }

    #[test]
    fn framed_commands_return_errors_for_apply_edit_error_responses() {
        let messages = framed_command_session(json!({
            "jsonrpc": "2.0", "id": 1,
            "error": {"code": -32_603, "message": "apply request failed"}
        }));

        assert_eq!(messages[1]["method"], "workspace/applyEdit");
        assert_eq!(messages[2]["id"], 20);
        assert_eq!(messages[2]["error"]["code"], -32_803);
        assert_eq!(messages[2]["error"]["message"], "apply request failed");
    }
}
