//! Built-in local OpenAI-compatible HTTP bridge (`http://127.0.0.1:11535/v1`)
//! backed by macOS `FoundationModels.framework` (`SystemLanguageModel.default`
//! and `LanguageModelSession`).

use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::process::Command;

const BRIDGE_ADDR: &str = "127.0.0.1:11535";

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

async fn ensure_helper_binary() -> Result<PathBuf, String> {
    let bin_path =
        helper_binary_path().ok_or_else(|| "Application support folder unavailable".to_string())?;
    if bin_path.exists() {
        return Ok(bin_path);
    }
    if let Some(parent) = bin_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create bin directory: {e}"))?;
    }
    let src_path = bin_path.with_extension("swift");
    std::fs::write(&src_path, SWIFT_HELPER_SOURCE)
        .map_err(|e| format!("Failed to write Swift helper source: {e}"))?;

    let status = Command::new("xcrun")
        .args([
            "swiftc",
            "-O",
            "-parse-as-library",
            src_path.to_string_lossy().as_ref(),
            "-o",
            bin_path.to_string_lossy().as_ref(),
        ])
        .status()
        .await
        .map_err(|e| format!("Failed to invoke xcrun swiftc: {e}"))?;

    if !status.success() {
        return Err("Failed to compile Apple FoundationModels Swift helper".to_string());
    }
    Ok(bin_path)
}

async fn run_apple_foundation_model(
    instructions: Option<String>,
    prompt: String,
) -> Result<String, String> {
    let bin = ensure_helper_binary().await?;
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
        .spawn()
        .map_err(|e| format!("Failed to launch Apple FoundationModels helper: {e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(&payload_bytes)
            .await
            .map_err(|e| format!("Failed writing to helper stdin: {e}"))?;
    }

    let output = child
        .wait_with_output()
        .await
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
        let _ = ensure_helper_binary().await;
        let Ok(listener) = TcpListener::bind(BRIDGE_ADDR).await else {
            return;
        };
        tracing::info!("Apple FoundationModels bridge listening on http://{BRIDGE_ADDR}/v1");

        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            tauri::async_runtime::spawn(async move {
                let _ = handle_connection(stream).await;
            });
        }
    });
}

async fn handle_connection(mut stream: TcpStream) -> Result<(), String> {
    let mut buf = vec![0u8; 65_536];
    let mut total_read = 0usize;
    let mut header_end = None;
    let mut content_length = 0usize;

    loop {
        let n = stream
            .read(&mut buf[total_read..])
            .await
            .map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        total_read += n;
        if header_end.is_none()
            && let Some(pos) = buf[..total_read].windows(4).position(|w| w == b"\r\n\r\n")
        {
            let end = pos + 4;
            header_end = Some(end);
            let headers_str = String::from_utf8_lossy(&buf[..end]);
            for line in headers_str.lines() {
                if let Some((k, v)) = line.split_once(':')
                    && k.trim().eq_ignore_ascii_case("content-length")
                {
                    content_length = v.trim().parse::<usize>().unwrap_or(0);
                }
            }
        }
        if let Some(end) = header_end
            && total_read >= end + content_length
        {
            break;
        }
        if total_read == buf.len() {
            buf.resize(buf.len() * 2, 0);
        }
    }

    if total_read == 0 {
        return Ok(());
    }

    let req_str = String::from_utf8_lossy(&buf[..total_read]);
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
        let body_slice = if let Some(end) = header_end {
            &buf[end..total_read]
        } else {
            &[][..]
        };

        let parsed: serde_json::Value =
            serde_json::from_slice(body_slice).unwrap_or_else(|_| serde_json::json!({}));
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
