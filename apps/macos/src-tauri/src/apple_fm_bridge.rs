//! Built-in local OpenAI-compatible HTTP bridge (`http://127.0.0.1:11535/v1`)
//! backed by macOS `FoundationModels.framework` (`SystemLanguageModel.default`
//! and `LanguageModelSession`).
//!
//! The bridge listens on loopback only, but any web page can still aim a
//! request at a loopback port. Requests that carry an `Origin` header (every
//! browser request does) or a `Host` other than the bridge's own address are
//! refused, which also defeats DNS rebinding. Requests are size- and
//! time-limited, and one helper process runs at a time.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::process::Command;
use tokio::sync::{Mutex, Semaphore};

const BRIDGE_ADDR: &str = "127.0.0.1:11535";

/// The largest request header block the bridge accepts.
const MAX_HEADER_BYTES: usize = 16 * 1024;
/// The largest request body the bridge accepts.
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
/// How long a client has to send its whole request.
const READ_TIMEOUT: Duration = Duration::from_secs(10);
/// How long one on-device generation may run.
const GENERATION_TIMEOUT: Duration = Duration::from_secs(120);

const SWIFT_HELPER_SOURCE: &str = r#"import Foundation
import FoundationModels

struct Req: Decodable {
    let instructions: String?
    let prompt: String
}

@main
struct EloAppleFM {
    static func main() async {
        let args = CommandLine.arguments
        if args.contains("--check") {
            print(SystemLanguageModel.default.availability)
            return
        }
        let data = FileHandle.standardInput.readDataToEndOfFile()
        guard let req = try? JSONDecoder().decode(Req.self, from: data) else {
            fputs("Invalid JSON request\n", stderr)
            exit(1)
        }
        do {
            let session: LanguageModelSession
            if let inst = req.instructions, !inst.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                session = LanguageModelSession(instructions: inst)
            } else {
                session = LanguageModelSession()
            }
            let resp = try await session.respond(to: req.prompt)
            print(resp.content, terminator: "")
        } catch {
            fputs("FoundationModels error: \(error)\n", stderr)
            exit(2)
        }
    }
}
"#;

fn helper_binary_path() -> Option<PathBuf> {
    crate::config::Config::folder_path().map(|dir| dir.join("bin").join("elo-apple-fm"))
}

/// Compiles the Swift helper on first use.
///
/// The helper is rebuilt whenever its saved source differs from
/// `SWIFT_HELPER_SOURCE`, so an app update ships its new helper. It is
/// compiled to a temporary path and renamed into place, so a compile that is
/// interrupted never leaves a half-written binary that would be trusted on
/// the next launch. Compiling happens on the first chat request rather than at
/// app launch, because `xcrun` on a Mac without the developer tools opens an
/// install prompt.
async fn ensure_helper_binary() -> Result<PathBuf, String> {
    static BUILD: OnceLock<Mutex<()>> = OnceLock::new();
    let _building = BUILD.get_or_init(|| Mutex::new(())).lock().await;

    let bin_path =
        helper_binary_path().ok_or_else(|| "Application support folder unavailable".to_string())?;
    let src_path = bin_path.with_extension("swift");
    let source_is_current = std::fs::read_to_string(&src_path)
        .map(|saved| saved == SWIFT_HELPER_SOURCE)
        .unwrap_or(false);
    if source_is_current && bin_path.is_file() {
        return Ok(bin_path);
    }
    if let Some(parent) = bin_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create bin directory: {e}"))?;
    }
    std::fs::write(&src_path, SWIFT_HELPER_SOURCE)
        .map_err(|e| format!("Failed to write Swift helper source: {e}"))?;

    let partial_path = bin_path.with_extension("partial");
    let status = Command::new("xcrun")
        .args([
            "swiftc",
            "-O",
            "-parse-as-library",
            src_path.to_string_lossy().as_ref(),
            "-o",
            partial_path.to_string_lossy().as_ref(),
        ])
        .stdin(Stdio::null())
        .status()
        .await
        .map_err(|e| format!("Failed to invoke xcrun swiftc: {e}"))?;

    if !status.success() {
        let _ = std::fs::remove_file(&partial_path);
        return Err("Failed to compile Apple FoundationModels Swift helper".to_string());
    }
    std::fs::rename(&partial_path, &bin_path)
        .map_err(|e| format!("Failed to install the Apple FoundationModels helper: {e}"))?;
    Ok(bin_path)
}

/// Allows one on-device generation at a time; the model runs on the Neural
/// Engine and parallel helpers only queue up behind each other.
fn generation_permits() -> &'static Semaphore {
    static PERMITS: OnceLock<Semaphore> = OnceLock::new();
    PERMITS.get_or_init(|| Semaphore::new(1))
}

async fn run_apple_foundation_model(
    instructions: Option<String>,
    prompt: String,
) -> Result<String, String> {
    let bin = ensure_helper_binary().await?;
    let _permit = generation_permits()
        .acquire()
        .await
        .map_err(|e| format!("Apple FoundationModels bridge is shutting down: {e}"))?;
    let payload = serde_json::json!({
        "instructions": instructions,
        "prompt": prompt,
    });
    let payload_bytes =
        serde_json::to_vec(&payload).map_err(|e| format!("JSON encode error: {e}"))?;

    let mut child = Command::new(bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("Failed to launch Apple FoundationModels helper: {e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(&payload_bytes)
            .await
            .map_err(|e| format!("Failed writing to helper stdin: {e}"))?;
    }

    let output = tokio::time::timeout(GENERATION_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| "Apple FoundationModels took too long to respond".to_string())?
        .map_err(|e| format!("Apple FoundationModels helper failed: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "Apple FoundationModels session returned an error".to_string()
        } else {
            stderr
        });
    }

    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(text)
}

/// Starts the embedded OpenAI-compatible HTTP bridge on `127.0.0.1:11535`
/// backed by Apple's `FoundationModels.framework`.
pub fn start() {
    tauri::async_runtime::spawn(async move {
        let Ok(listener) = TcpListener::bind(BRIDGE_ADDR).await else {
            return;
        };
        tracing::info!("Apple FoundationModels bridge listening on http://{BRIDGE_ADDR}/v1");

        loop {
            let Ok((stream, _)) = listener.accept().await else {
                // Accept fails persistently when the process is out of file
                // descriptors; back off instead of spinning a core.
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            };
            tauri::async_runtime::spawn(async move {
                let _ = handle_connection(stream).await;
            });
        }
    });
}

/// A request read off the socket: the header block and the body.
struct RawRequest {
    head: String,
    body: Vec<u8>,
}

enum ReadError {
    TooLarge,
    Io,
}

/// Reads one request, refusing a header block or body over the size limits.
async fn read_request(stream: &mut TcpStream) -> Result<Option<RawRequest>, ReadError> {
    let mut buf: Vec<u8> = Vec::with_capacity(8 * 1024);
    let mut chunk = [0u8; 8 * 1024];
    let mut header_end = None;
    let mut content_length = 0usize;

    loop {
        let n = stream.read(&mut chunk).await.map_err(|_| ReadError::Io)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if header_end.is_none() {
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let end = pos + 4;
                header_end = Some(end);
                let head = String::from_utf8_lossy(&buf[..end]);
                for line in head.lines() {
                    if let Some((k, v)) = line.split_once(':')
                        && k.trim().eq_ignore_ascii_case("content-length")
                    {
                        content_length = v.trim().parse::<usize>().unwrap_or(0);
                    }
                }
                if content_length > MAX_BODY_BYTES {
                    return Err(ReadError::TooLarge);
                }
            } else if buf.len() > MAX_HEADER_BYTES {
                return Err(ReadError::TooLarge);
            }
        }
        if let Some(end) = header_end
            && buf.len() >= end + content_length
        {
            break;
        }
    }

    let Some(end) = header_end else {
        return Ok(None);
    };
    let body_end = (end + content_length).min(buf.len());
    Ok(Some(RawRequest {
        head: String::from_utf8_lossy(&buf[..end]).into_owned(),
        body: buf[end..body_end].to_vec(),
    }))
}

/// Whether a request may use the bridge: it must name the bridge's own
/// address as its `Host` and must not come from a web page.
fn is_trusted_request(head: &str) -> bool {
    let mut host_ok = false;
    for line in head.lines().skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim();
        if name.eq_ignore_ascii_case("origin") {
            return false;
        }
        if name.eq_ignore_ascii_case("host") {
            let value = value.trim().to_ascii_lowercase();
            host_ok = value == BRIDGE_ADDR || value == "localhost:11535";
        }
    }
    host_ok
}

async fn handle_connection(mut stream: TcpStream) -> Result<(), String> {
    let request = match tokio::time::timeout(READ_TIMEOUT, read_request(&mut stream)).await {
        Ok(Ok(Some(request))) => request,
        Ok(Ok(None)) | Ok(Err(ReadError::Io)) | Err(_) => return Ok(()),
        Ok(Err(ReadError::TooLarge)) => {
            let body = serde_json::json!({"error": {"message": "Request too large"}}).to_string();
            return write_json_response(&mut stream, 413, "Payload Too Large", &body).await;
        }
    };
    if !is_trusted_request(&request.head) {
        let body = serde_json::json!({"error": {"message": "Forbidden"}}).to_string();
        return write_json_response(&mut stream, 403, "Forbidden", &body).await;
    }

    let req_str = request.head.as_str();
    let first_line = req_str.lines().next().unwrap_or("");
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or("GET");
    let path = parts.next().unwrap_or("/");

    if method.eq_ignore_ascii_case("GET") && (path.starts_with("/v1/models") || path == "/models") {
        let body = serde_json::json!({
            "object": "list",
            "data": [
                {
                    "id": "apple/foundation-on-device",
                    "name": "Apple Foundation Model (On-Device)",
                    "object": "model",
                    "owned_by": "apple-foundation-models"
                },
                {
                    "id": "apple/foundation-adapter-editorial",
                    "name": "Apple Editorial Adapter",
                    "object": "model",
                    "owned_by": "apple-foundation-models"
                },
                {
                    "id": "apple/writing-tools-local",
                    "name": "Apple Writing Tools Engine",
                    "object": "model",
                    "owned_by": "apple-foundation-models"
                }
            ]
        })
        .to_string();
        return write_json_response(&mut stream, 200, "OK", &body).await;
    }

    if method.eq_ignore_ascii_case("POST")
        && (path.starts_with("/v1/chat/completions") || path.starts_with("/chat/completions"))
    {
        let parsed: serde_json::Value =
            serde_json::from_slice(&request.body).unwrap_or_else(|_| serde_json::json!({}));
        let model = parsed
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("apple/foundation-on-device")
            .to_string();
        let is_stream = parsed
            .get("stream")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let mut system_parts = Vec::new();
        let mut user_parts = Vec::new();
        if let Some(msgs) = parsed.get("messages").and_then(|v| v.as_array()) {
            for m in msgs {
                let role = m.get("role").and_then(|v| v.as_str()).unwrap_or("user");
                let content = m.get("content").and_then(|v| v.as_str()).unwrap_or("");
                if role == "system" || role == "developer" {
                    system_parts.push(content.to_string());
                } else {
                    user_parts.push(content.to_string());
                }
            }
        }

        if model.contains("editorial") {
            system_parts.push(
                "Output only the revised text without conversational filler or markdown fences."
                    .to_string(),
            );
        }

        let instructions = if system_parts.is_empty() {
            None
        } else {
            Some(system_parts.join("\n\n"))
        };
        let prompt = user_parts.join("\n\n");

        match run_apple_foundation_model(instructions.clone(), prompt.clone()).await {
            Ok(reply) => {
                let input_tokens =
                    ((instructions.unwrap_or_default().len() + prompt.len()) / 4).max(1) as u64;
                let output_tokens = (reply.len() / 4).max(1) as u64;

                if is_stream {
                    let chunk1 = serde_json::json!({
                        "id": "chatcmpl-apple-fm",
                        "object": "chat.completion.chunk",
                        "model": model,
                        "choices": [{
                            "index": 0,
                            "delta": { "role": "assistant", "content": reply },
                            "finish_reason": null
                        }]
                    });
                    let chunk2 = serde_json::json!({
                        "id": "chatcmpl-apple-fm",
                        "object": "chat.completion.chunk",
                        "model": model,
                        "choices": [{
                            "index": 0,
                            "delta": {},
                            "finish_reason": "stop"
                        }],
                        "usage": {
                            "prompt_tokens": input_tokens,
                            "completion_tokens": output_tokens,
                            "total_tokens": input_tokens + output_tokens,
                            "cost": 0.0
                        }
                    });
                    let sse_body = format!("data: {chunk1}\n\ndata: {chunk2}\n\ndata: [DONE]\n\n");
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        sse_body.len(),
                        sse_body
                    );
                    stream
                        .write_all(resp.as_bytes())
                        .await
                        .map_err(|e| e.to_string())?;
                    return Ok(());
                }

                let body = serde_json::json!({
                    "id": "chatcmpl-apple-fm",
                    "object": "chat.completion",
                    "model": model,
                    "choices": [{
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "content": reply
                        },
                        "finish_reason": "stop"
                    }],
                    "usage": {
                        "prompt_tokens": input_tokens,
                        "completion_tokens": output_tokens,
                        "total_tokens": input_tokens + output_tokens,
                        "cost": 0.0
                    }
                })
                .to_string();
                return write_json_response(&mut stream, 200, "OK", &body).await;
            }
            Err(err) => {
                let err_body = serde_json::json!({
                    "error": {
                        "message": err,
                        "type": "apple_foundation_model_error"
                    }
                })
                .to_string();
                return write_json_response(&mut stream, 500, "Internal Server Error", &err_body)
                    .await;
            }
        }
    }

    let not_found = serde_json::json!({"error": {"message": "Not found"}}).to_string();
    write_json_response(&mut stream, 404, "Not Found", &not_found).await
}

async fn write_json_response(
    stream: &mut TcpStream,
    code: u16,
    reason: &str,
    body: &str,
) -> Result<(), String> {
    let resp = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream
        .write_all(resp.as_bytes())
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::is_trusted_request;

    #[test]
    fn the_app_request_is_trusted() {
        let head = "POST /v1/chat/completions HTTP/1.1\r\nhost: 127.0.0.1:11535\r\ncontent-type: application/json\r\n\r\n";
        assert!(is_trusted_request(head));
    }

    #[test]
    fn a_browser_request_is_refused() {
        let head = "POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1:11535\r\nOrigin: https://example.com\r\n\r\n";
        assert!(!is_trusted_request(head));
    }

    #[test]
    fn a_rebound_host_is_refused() {
        let head = "GET /v1/models HTTP/1.1\r\nHost: attacker.example:11535\r\n\r\n";
        assert!(!is_trusted_request(head));
        assert!(!is_trusted_request("GET /v1/models HTTP/1.1\r\n\r\n"));
    }
}
