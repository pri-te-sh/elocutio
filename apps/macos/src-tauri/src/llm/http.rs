//! The async provider client: one HTTP round trip, with retries.
//!
//! The client knows how to talk HTTP and how to back off. It does not know
//! any provider's JSON shape — that is the adapter's job, and the adapter
//! comes from `elocutio-llm`.

use elocutio_llm::{ChatRequest, ChatResponse, Provider, ProviderError};

use super::{MAX_ATTEMPTS, REQUEST_TIMEOUT, backoff_delay, is_retryable};

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

/// Build an HTTP client.
pub fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|error| format!("Could not create the HTTP client: {error}"))
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
            Ok(response) => return Ok(response),
            Err(error) => {
                let retryable = is_retryable(&error);
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
    let raw = response.text().await.map_err(|error| {
        adapter.map_transport_error(&format!("could not read the response: {error}"))
    })?;

    if status.is_success() {
        return adapter.parse_response(&raw);
    }

    Err(adapter.map_http_error(status.as_u16(), &raw))
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
