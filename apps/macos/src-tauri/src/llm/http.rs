//! The async provider client: one HTTP round trip, with retries.
//!
//! The client knows how to talk HTTP and how to back off. It does not know
//! any provider's JSON shape — that is the adapter's job, and the adapter
//! comes from `elocutio-llm`.

use elocutio_llm::{ChatRequest, ChatResponse, Provider, ProviderError};

use super::{CONNECT_TIMEOUT, MAX_ATTEMPTS, REQUEST_TIMEOUT, backoff_delay, is_retryable};

/// The headers OpenRouter wants on every request.
pub const APP_URL: &str = "https://github.com/pri-te-sh/elocutio";
pub const APP_TITLE: &str = "Elocutio";

/// A provider endpoint plus the key to use with it.
#[derive(Debug, Clone)]
pub struct Endpoint {
    /// The full URL of the chat completions endpoint.
    pub url: String,
    /// The bearer token, when the provider needs one.
    pub key: Option<String>,
    /// True when the provider wants the OpenRouter attribution headers.
    pub send_attribution_headers: bool,
}

/// The shared HTTP client.
///
/// One client is reused for every request so connections (and their TLS
/// sessions) are pooled; building a client per request paid a fresh
/// handshake every time.
pub fn client() -> Result<reqwest::Client, String> {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    if let Some(client) = CLIENT.get() {
        return Ok(client.clone());
    }
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
        .map_err(|error| format!("Could not create the HTTP client: {error}"))?;
    Ok(CLIENT.get_or_init(|| client).clone())
}

/// The shared client for streamed responses, which has a connect timeout but
/// no total ceiling; the reader enforces an idle timeout per chunk instead.
pub fn streaming_client() -> Result<reqwest::Client, String> {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    if let Some(client) = CLIENT.get() {
        return Ok(client.clone());
    }
    let client = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
        .map_err(|error| format!("Could not create the HTTP client: {error}"))?;
    Ok(CLIENT.get_or_init(|| client).clone())
}

/// The `Retry-After` header in seconds, when it is a plain number.
pub fn retry_after_seconds(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
}

/// Send a chat request and return the parsed response.
///
/// Retries only failures the taxonomy calls retryable, and only up to
/// [`MAX_ATTEMPTS`] attempts in total.
pub async fn send_chat(
    adapter: &dyn Provider,
    endpoint: &Endpoint,
    request: &ChatRequest,
) -> Result<ChatResponse, ProviderError> {
    let body = adapter.build_request(request);
    let mut last_error: Option<ProviderError> = None;

    for attempt in 0..MAX_ATTEMPTS {
        match attempt_once(adapter, endpoint, &body).await {
            Ok(response) => {
                // Every request that leaves the Mac is recorded, so the
                // privacy claim "no hidden network calls" has something to
                // point at. The entry holds metadata, never the text.
                let _ = crate::network_log::record_chat(
                    &crate::network_log::host_of(&endpoint.url),
                    adapter.display_name(),
                    Some(&request.model.model),
                    (response.usage.input_tokens, response.usage.output_tokens),
                    response.usage.cost_usd(),
                    crate::network_log::Outcome::success(200),
                );
                return Ok(response);
            }
            Err(error) => {
                let retryable = is_retryable(&error);
                let _ = crate::network_log::record_chat(
                    &crate::network_log::host_of(&endpoint.url),
                    adapter.display_name(),
                    Some(&request.model.model),
                    (0, 0),
                    None,
                    crate::network_log::Outcome::failure(None, error.message.clone()),
                );
                last_error = Some(error);
                if !retryable || attempt + 1 >= MAX_ATTEMPTS {
                    break;
                }
                let delay = backoff_delay(
                    last_error.as_ref().expect("the error was just stored"),
                    attempt,
                );
                tokio::time::sleep(delay).await;
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        ProviderError::new(
            elocutio_llm::ErrorKind::Unknown,
            "The provider request failed after retries.",
        )
    }))
}

/// One attempt at the request.
async fn attempt_once(
    adapter: &dyn Provider,
    endpoint: &Endpoint,
    body: &serde_json::Value,
) -> Result<ChatResponse, ProviderError> {
    let client =
        client().map_err(|error| ProviderError::new(elocutio_llm::ErrorKind::Unknown, error))?;

    let mut request = client.post(&endpoint.url).json(body);
    if let Some(key) = &endpoint.key {
        request = request.bearer_auth(key);
    }
    if endpoint.send_attribution_headers {
        request = request
            .header("HTTP-Referer", APP_URL)
            .header("X-Title", APP_TITLE);
    }

    // The taxonomy supplies the headline ("Could not reach the provider..."),
    // so only the underlying cause is passed as the detail. Passing the
    // headline here too would print it twice.
    let response = request
        .send()
        .await
        .map_err(|error| adapter.map_transport_error(&error.to_string()))?;

    let status = response.status();
    let retry_after = retry_after_seconds(response.headers());
    let raw = response.text().await.map_err(|error| {
        adapter.map_transport_error(&format!("could not read the response: {error}"))
    })?;

    if status.is_success() {
        return adapter.parse_response(&raw);
    }

    Err(adapter
        .map_http_error(status.as_u16(), &raw)
        .with_retry_after(retry_after))
}

#[cfg(test)]
mod tests {
    use super::super::user_message;
    use super::*;
    use elocutio_llm::{ErrorKind, OpenRouterAdapter, ProviderError};

    #[test]
    fn the_retry_delay_grows_with_the_attempt() {
        let error = ProviderError::new(ErrorKind::Server, "boom");
        assert!(backoff_delay(&error, 1) > backoff_delay(&error, 0));
    }

    #[test]
    fn a_timed_out_post_is_not_retried() {
        // The request may have reached the model; retrying could bill twice.
        assert!(!super::super::is_retryable(&ProviderError::new(
            ErrorKind::Timeout,
            "slow"
        )));
        assert!(super::super::is_retryable(&ProviderError::new(
            ErrorKind::RateLimit,
            "busy"
        )));
    }

    #[test]
    fn a_long_retry_after_is_not_waited_out() {
        let short = ProviderError::new(ErrorKind::RateLimit, "busy").with_retry_after(Some(5));
        let long = ProviderError::new(ErrorKind::RateLimit, "busy").with_retry_after(Some(600));
        assert!(super::super::is_retryable(&short));
        assert!(!super::super::is_retryable(&long));
        assert_eq!(backoff_delay(&short, 0), std::time::Duration::from_secs(5));
    }

    #[test]
    fn retry_after_is_read_from_the_headers() {
        let mut headers = reqwest::header::HeaderMap::new();
        assert_eq!(retry_after_seconds(&headers), None);
        headers.insert(
            reqwest::header::RETRY_AFTER,
            "12".parse().expect("a header"),
        );
        assert_eq!(retry_after_seconds(&headers), Some(12));
    }

    #[test]
    fn a_transport_failure_is_phrased_for_the_user() {
        // The headline comes from the taxonomy; the cause is the detail, and
        // the headline is not repeated.
        let message = user_message(&ProviderError::new(ErrorKind::Network, "no route to host"));
        assert!(message.contains("Could not reach the provider"));
        assert!(message.contains("no route to host"));
        assert_eq!(message.matches("Could not reach the provider").count(), 1);
    }

    #[test]
    fn an_endpoint_carries_its_attribution_flag() {
        let endpoint = Endpoint {
            url: "https://example.test/v1/chat/completions".to_string(),
            key: Some("secret".to_string()),
            send_attribution_headers: true,
        };
        assert!(endpoint.send_attribution_headers);
        assert_eq!(endpoint.key.as_deref(), Some("secret"));
    }

    #[test]
    fn the_adapter_compiles_the_body() {
        let request = ChatRequest::new(
            elocutio_llm::ModelRef::new(
                elocutio_llm::ProviderId::OpenRouter,
                "anthropic/claude-haiku-5.5",
            ),
            vec![elocutio_llm::Message::user("Hi")],
        );
        let body = OpenRouterAdapter.build_request(&request);
        assert_eq!(body["model"], "anthropic/claude-haiku-5.5");
        assert!(
            !body
                .as_object()
                .expect("an object")
                .contains_key("temperature")
        );
    }
}
