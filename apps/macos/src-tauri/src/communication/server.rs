use crate::config::Config;
use harper_core::{DictWordMetadata, spell::Dictionary};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, BufReader};
use tokio::sync::Mutex;

use super::error::ProtocolError;
use super::framing::write_message;
use super::message::{Command, Request, Response};

/// Tauri-side protocol endpoint that owns shared state and responds to highlighter requests.
pub struct Server<R, W> {
    reader: BufReader<R>,
    writer: W,
    config: Arc<Mutex<Config>>,
    /// The shared suggestion count, so the menu bar can show it.
    suggestion_count: Option<Arc<crate::config::SuggestionCount>>,
}

impl<R, W> Server<R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    pub fn new(reader: R, writer: W, config: Arc<Mutex<Config>>) -> Self {
        Self {
            reader: BufReader::new(reader),
            writer,
            config,
            suggestion_count: None,
        }
    }

    /// Shares the suggestion count with the server, so the menu bar can show
    /// how many suggestions the card has.
    pub fn with_suggestion_count(
        mut self,
        suggestion_count: Arc<crate::config::SuggestionCount>,
    ) -> Self {
        self.suggestion_count = Some(suggestion_count);
        self
    }

    pub async fn receive_request(&mut self) -> Result<Option<Request>, ProtocolError> {
        let mut line = String::new();
        if self.reader.read_line(&mut line).await? == 0 {
            return Ok(None);
        }

        // The highlighter can send a request, which is answered, or a
        // command, which is not. Both arrive on the same stream.
        if let Ok(command) = serde_json::from_str::<Command>(&line) {
            self.handle_command(&command).await;
            return Ok(None);
        }

        let request = serde_json::from_str(&line)?;
        let response = self.handle_request(&request).await;
        write_message(&mut self.writer, &response).await?;

        Ok(Some(request))
    }

    /// Handles a command from the highlighter.
    async fn handle_command(&self, command: &Command) {
        match command {
            Command::CardState { suggestions } => {
                let mut config = self.config.lock().await;
                config.card_suggestion_count = *suggestions;
                if let Some(count) = &self.suggestion_count {
                    count.set(*suggestions);
                }
            }
            Command::Card(card_command) => {
                // The highlighter asks the main process to drive the card.
                let mut config = self.config.lock().await;
                config.card_command = Some(*card_command);
            }
        }
    }

    async fn handle_request(&self, request: &Request) -> Response {
        match request {
            Request::OpenSettings | Request::AnalyzeField => Response::Ack,
            Request::Preview => {
                // The settings window shows the request preview. The window is
                // opened by the worker, which holds the app handle.
                Response::Ack
            }
            Request::GetLintConfig => Response::GetLintConfig {
                config: self.config.lock().await.lint_config.clone(),
            },
            Request::GetDictionary => Response::GetDictionary {
                words: self
                    .config
                    .lock()
                    .await
                    .mutable_dictionary
                    .words_iter()
                    .map(|word| word.iter().collect())
                    .collect(),
            },
            Request::GetDialect => Response::GetDialect {
                dialect: self.config.lock().await.dialect,
            },
            Request::GetDebounceMs => Response::GetDebounceMs {
                debounce_ms: self.config.lock().await.debounce_ms,
            },
            Request::GetIgnoredLints => Response::GetIgnoredLints {
                ignored_lints: self.config.lock().await.ignored_lints.clone(),
            },
            Request::GetIntegrations => Response::GetIntegrations {
                integrations: self.config.lock().await.integrations.clone(),
            },
            Request::GetAutoEnableNewApps => Response::GetAutoEnableNewApps {
                enabled: self.config.lock().await.auto_enable_new_apps,
            },
            Request::GetCardState => {
                // The number of suggestions the card has open.
                let config = self.config.lock().await;
                Response::CardState {
                    open: config.card_suggestion_count > 0,
                    suggestions: config.card_suggestion_count,
                }
            }
            Request::GetCardCommand => {
                // The command is taken as it is read, so a keypress is
                // delivered exactly once.
                let mut config = self.config.lock().await;
                Response::CardCommand {
                    command: config.card_command.take(),
                }
            }
            Request::GetWidgetAnchor { bundle_id } => {
                let config = self.config.lock().await;
                Response::GetWidgetAnchor {
                    anchor: config.widget_anchors.get(bundle_id),
                }
            }
            Request::ResolveIntegration { bundle_id } => {
                let mut config = self.config.lock().await;
                let previous_len = config.integrations.len();
                let enabled = config.resolve_integration(bundle_id);
                if config.integrations.len() != previous_len
                    && let Err(error) = config.save_to_system().await
                {
                    config.remove_integration(bundle_id.trim());
                    eprintln!("failed to save discovered integration: {error}");
                    return Response::ResolveIntegration { enabled: false };
                }
                Response::ResolveIntegration { enabled }
            }
            Request::SetLintConfig { config } => {
                let mut stored_config = self.config.lock().await;
                stored_config.lint_config = config.clone();

                if let Err(error) = stored_config.save_to_system().await {
                    eprintln!("failed to save config: {error}");
                }

                Response::Ack
            }
            Request::IgnoreLint { ignored_lints } => {
                let mut config = self.config.lock().await;
                config.ignored_lints.append(ignored_lints.clone());

                if let Err(error) = config.save_to_system().await {
                    eprintln!("failed to save config: {error}");
                }

                Response::Ack
            }
            Request::AddToDictionary { word } => {
                let mut config = self.config.lock().await;
                config
                    .mutable_dictionary
                    .append_word_str(word, DictWordMetadata::default());

                if let Err(error) = config.save_to_system().await {
                    eprintln!("failed to save config: {error}");
                }

                Response::Ack
            }
            Request::AddIntegration { bundle_id } => {
                let mut config = self.config.lock().await;
                config.add_integration(bundle_id.clone());

                if let Err(error) = config.save_to_system().await {
                    eprintln!("failed to save config: {error}");
                }

                Response::Ack
            }
            Request::RemoveIntegration { bundle_id } => {
                let mut config = self.config.lock().await;
                config.remove_integration(bundle_id);

                if let Err(error) = config.save_to_system().await {
                    eprintln!("failed to save config: {error}");
                }

                Response::Ack
            }
            Request::SetIntegrationEnabled { bundle_id, enabled } => {
                let mut config = self.config.lock().await;
                config.set_integration_enabled(bundle_id, *enabled);

                if let Err(error) = config.save_to_system().await {
                    eprintln!("failed to save config: {error}");
                }

                Response::Ack
            }
        }
    }
}
