//! Bridge mode for external UI communication.
//!
//! Provides NDJSON-based protocol for UI integrations (VS Code, web UI, etc.).
//! All messages are JSON objects separated by newlines.
//!
//! ## Protocol
//!
//! ### Outbound Messages (stdout)
//! ```json
//! {"type": "ready", "version": "0.5.0"}
//! {"type": "text_delta", "text": "Hello "}
//! {"type": "tool_call", "name": "read_file", "args": {...}}
//! {"type": "tool_result", "name": "read_file", "success": true, "output": "..."}
//! {"type": "complete", "run_id": "..."}
//! {"type": "error", "message": "..."}
//! ```
//!
//! ### Inbound Commands (stdin)
//! ```json
//! {"type": "prompt", "text": "...", "agent": "stockpot"}
//! {"type": "cancel"}
//! {"type": "tool_response", "name": "...", "approved": true}
//! ```

use crate::agents::{AgentExecutor, AgentManager, StreamEvent};
use crate::config::Settings;
use crate::db::Database;
use crate::mcp::McpManager;
use crate::messaging::{AgentEvent, Message, MessageBus, MessageReceiver, ToolStatus};
use crate::models::ModelRegistry;
use crate::tools::SpotToolRegistry;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader};
use tokio::sync::mpsc;
use tracing::debug;

/// Outbound message types sent to the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BridgeOutMessage {
    /// Bridge is ready to receive commands.
    Ready {
        version: String,
        agent: String,
        model: String,
    },
    /// Text content streaming.
    TextDelta { text: String },
    /// Thinking/reasoning content.
    ThinkingDelta { text: String },
    /// Tool call started.
    ToolCallStart {
        tool_name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_call_id: Option<String>,
    },
    /// Tool call arguments streaming.
    ToolCallDelta { delta: String },
    /// Tool call completed.
    ToolCallComplete { tool_name: String },
    /// Tool executed with result.
    ToolExecuted {
        tool_name: String,
        success: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// Request step started.
    RequestStart { step: u32 },
    /// Run completed successfully.
    Complete { run_id: String },
    /// Error occurred.
    Error { message: String },
    /// Agent switched.
    AgentChanged { agent: String },
    /// Model switched.
    ModelChanged { model: String },
    /// MCP server status.
    McpStatus { servers: Vec<McpServerStatus> },
}

#[derive(Debug, Clone, Serialize)]
pub struct McpServerStatus {
    pub name: String,
    pub running: bool,
}

impl BridgeOutMessage {
    /// Convert a Message bus event to a Bridge output message.
    fn from_message(msg: Message) -> Option<Self> {
        match msg {
            Message::TextDelta(delta) => Some(BridgeOutMessage::TextDelta { text: delta.text }),
            Message::Thinking(thinking) => Some(BridgeOutMessage::ThinkingDelta {
                text: thinking.text,
            }),
            Message::Agent(agent) => match agent.event {
                AgentEvent::Started => None, // Silent - UI can track from tool/text events
                AgentEvent::Completed { run_id } => Some(BridgeOutMessage::Complete { run_id }),
                AgentEvent::Error { message } => Some(BridgeOutMessage::Error { message }),
            },
            Message::Tool(tool) => match tool.status {
                ToolStatus::Started => Some(BridgeOutMessage::ToolCallStart {
                    tool_name: tool.tool_name,
                    tool_call_id: None,
                }),
                ToolStatus::ArgsStreaming => None, // We don't have delta here
                ToolStatus::Executing => Some(BridgeOutMessage::ToolCallComplete {
                    tool_name: tool.tool_name,
                }),
                ToolStatus::Completed => Some(BridgeOutMessage::ToolExecuted {
                    tool_name: tool.tool_name,
                    success: true,
                    error: None,
                }),
                ToolStatus::Failed => Some(BridgeOutMessage::ToolExecuted {
                    tool_name: tool.tool_name,
                    success: false,
                    error: tool.error,
                }),
            },
            // Other message types don't map to bridge protocol
            _ => None,
        }
    }
}

/// Inbound command types from the UI.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BridgeInCommand {
    /// Send a prompt to the agent.
    Prompt {
        text: String,
        #[serde(default)]
        agent: Option<String>,
        #[serde(default)]
        model: Option<String>,
    },
    /// Cancel the current operation.
    Cancel,
    /// Switch agent.
    SwitchAgent { agent: String },
    /// Switch model.
    SwitchModel { model: String },
    /// Start MCP server.
    McpStart {
        #[serde(default)]
        name: Option<String>,
    },
    /// Stop MCP server.
    McpStop {
        #[serde(default)]
        name: Option<String>,
    },
    /// List MCP tools.
    McpList,
    /// Approve/reject a tool call (for confirmation mode).
    ToolResponse { call_id: String, approved: bool },
    /// Shutdown the bridge.
    Shutdown,
}

/// Bridge renderer that outputs NDJSON to stdout.
struct BridgeRenderer;

impl BridgeRenderer {
    /// Run the render loop, converting Messages to NDJSON output.
    async fn run_loop(&self, mut receiver: MessageReceiver) {
        while let Ok(message) = receiver.recv().await {
            if let Some(bridge_msg) = BridgeOutMessage::from_message(message) {
                if let Ok(json) = serde_json::to_string(&bridge_msg) {
                    println!("{}", json);
                }
            }
        }
    }
}

/// Bridge state.
struct Bridge<'a> {
    db: &'a Database,
    agents: AgentManager,
    tool_registry: SpotToolRegistry,
    mcp_manager: McpManager,
    model_registry: ModelRegistry,
    current_model: String,
    /// Message bus for event-driven output.
    message_bus: MessageBus,
}

impl<'a> Bridge<'a> {
    fn new(db: &'a Database) -> Self {
        let settings = Settings::new(db);
        let current_model = settings.model();

        Self {
            db,
            agents: AgentManager::new(),
            tool_registry: SpotToolRegistry::new(),
            mcp_manager: McpManager::new(),
            model_registry: ModelRegistry::load_from_db(db).unwrap_or_default(),
            current_model,
            message_bus: MessageBus::new(),
        }
    }

    /// Send a message to the UI.
    fn emit(&self, msg: BridgeOutMessage) {
        if let Ok(json) = serde_json::to_string(&msg) {
            println!("{}", json);
        }
    }

    /// Handle a prompt command.
    async fn handle_prompt(&mut self, text: &str) {
        self.handle_prompt_with_bus(text).await;
    }

    /// Handle a prompt using the message bus architecture.
    ///
    /// This approach uses the message bus for event-driven output,
    /// making sub-agent output visible through the same NDJSON protocol.
    async fn handle_prompt_with_bus(&mut self, text: &str) {
        let agent = match self.agents.current() {
            Some(a) => a,
            None => {
                self.emit(BridgeOutMessage::Error {
                    message: "No agent selected".to_string(),
                });
                return;
            }
        };

        debug!(agent = %agent.name(), model = %self.current_model, "Bridge handling prompt");

        // Create executor with message bus
        let executor =
            AgentExecutor::new(self.db, &self.model_registry).with_bus(self.message_bus.sender());

        // Spawn bridge renderer
        let receiver = self.message_bus.subscribe();
        let render_handle = tokio::spawn(async move {
            BridgeRenderer.run_loop(receiver).await;
        });

        // Execute - all events flow through the bus automatically!
        let result = executor
            .execute_with_bus(
                agent,
                &self.current_model,
                text,
                None, // No history in bridge mode for now
                &self.tool_registry,
                &self.mcp_manager,
            )
            .await;

        // Give renderer a moment to finish processing, then abort if needed
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        render_handle.abort();

        // Handle errors (successes are emitted through the bus)
        if let Err(e) = result {
            self.emit(BridgeOutMessage::Error {
                message: e.to_string(),
            });
        }
    }

    /// Legacy prompt handling (direct stream processing).
    ///
    /// Kept for reference and fallback if needed.
    #[allow(dead_code)]
    async fn handle_prompt_legacy(&mut self, text: &str) {
        let agent = match self.agents.current() {
            Some(a) => a,
            None => {
                self.emit(BridgeOutMessage::Error {
                    message: "No agent selected".to_string(),
                });
                return;
            }
        };

        let executor = AgentExecutor::new(self.db, &self.model_registry);

        match executor
            .execute_stream(
                agent,
                &self.current_model,
                text,
                None,
                &self.tool_registry,
                &self.mcp_manager,
            )
            .await
        {
            Ok(mut stream) => {
                while let Some(event_result) = stream.recv().await {
                    match event_result {
                        Ok(event) => {
                            let msg = match event {
                                StreamEvent::RunStart { run_id: _ } => {
                                    Some(BridgeOutMessage::Ready {
                                        version: env!("CARGO_PKG_VERSION").to_string(),
                                        agent: self.agents.current_name(),
                                        model: self.current_model.clone(),
                                    })
                                }
                                StreamEvent::RequestStart { step } => {
                                    Some(BridgeOutMessage::RequestStart { step })
                                }
                                StreamEvent::TextDelta { text } => {
                                    Some(BridgeOutMessage::TextDelta { text })
                                }
                                StreamEvent::ThinkingDelta { text } => {
                                    Some(BridgeOutMessage::ThinkingDelta { text })
                                }
                                StreamEvent::ToolCallStart {
                                    tool_name,
                                    tool_call_id,
                                } => Some(BridgeOutMessage::ToolCallStart {
                                    tool_name,
                                    tool_call_id,
                                }),
                                StreamEvent::ToolCallDelta { delta, .. } => {
                                    Some(BridgeOutMessage::ToolCallDelta { delta })
                                }
                                StreamEvent::ToolCallComplete { tool_name, .. } => {
                                    Some(BridgeOutMessage::ToolCallComplete { tool_name })
                                }
                                StreamEvent::ToolExecuted {
                                    tool_name,
                                    success,
                                    error,
                                    ..
                                } => Some(BridgeOutMessage::ToolExecuted {
                                    tool_name,
                                    success,
                                    error,
                                }),
                                StreamEvent::ResponseComplete { .. } => None,
                                StreamEvent::OutputReady => None,
                                StreamEvent::RunComplete { run_id, .. } => {
                                    Some(BridgeOutMessage::Complete { run_id })
                                }
                                StreamEvent::Error { message } => {
                                    Some(BridgeOutMessage::Error { message })
                                }
                            };
                            if let Some(m) = msg {
                                self.emit(m);
                            }
                        }
                        Err(e) => {
                            self.emit(BridgeOutMessage::Error {
                                message: e.to_string(),
                            });
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                self.emit(BridgeOutMessage::Error {
                    message: e.to_string(),
                });
            }
        }
    }

    /// Handle MCP status request.
    async fn handle_mcp_list(&self) {
        let running = self.mcp_manager.running_servers().await;
        let config = self.mcp_manager.config();

        let servers: Vec<McpServerStatus> = config
            .servers
            .keys()
            .map(|name| McpServerStatus {
                name: name.clone(),
                running: running.contains(name),
            })
            .collect();

        self.emit(BridgeOutMessage::McpStatus { servers });
    }

    /// Handle agent switch.
    fn handle_switch_agent(&mut self, agent_name: &str) {
        if self.agents.exists(agent_name) {
            if self.agents.switch(agent_name).is_ok() {
                self.emit(BridgeOutMessage::AgentChanged {
                    agent: agent_name.to_string(),
                });
            }
        } else {
            self.emit(BridgeOutMessage::Error {
                message: format!("Agent not found: {}", agent_name),
            });
        }
    }

    /// Handle model switch.
    fn handle_switch_model(&mut self, model: &str) {
        self.current_model = model.to_string();
        let settings = Settings::new(self.db);
        let _ = settings.set("model", model);
        self.emit(BridgeOutMessage::ModelChanged {
            model: model.to_string(),
        });
    }
}

/// Run in bridge mode (NDJSON over stdio).
pub async fn run_bridge_mode() -> anyhow::Result<()> {
    let db = Database::open()?;
    db.migrate()?;

    let mut bridge = Bridge::new(&db);

    // Start enabled MCP servers
    let _ = bridge.mcp_manager.start_all().await;

    // Emit ready message
    bridge.emit(BridgeOutMessage::Ready {
        version: env!("CARGO_PKG_VERSION").to_string(),
        agent: bridge.agents.current_name(),
        model: bridge.current_model.clone(),
    });

    // Set up stdin reader in a separate thread
    let (tx, mut rx) = mpsc::channel::<BridgeInCommand>(32);

    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let reader = BufReader::new(stdin.lock());

        for line in reader.lines() {
            match line {
                Ok(line) if line.is_empty() => continue,
                Ok(line) => match serde_json::from_str::<BridgeInCommand>(&line) {
                    Ok(cmd) => {
                        if tx.blocking_send(cmd).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        eprintln!("{{\"type\":\"error\",\"message\":\"Invalid JSON: {}\"}}", e);
                    }
                },
                Err(_) => break,
            }
        }
    });

    // Main event loop
    while let Some(cmd) = rx.recv().await {
        match cmd {
            BridgeInCommand::Prompt { text, agent, model } => {
                if let Some(a) = agent {
                    bridge.handle_switch_agent(&a);
                }
                if let Some(m) = model {
                    bridge.handle_switch_model(&m);
                }
                bridge.handle_prompt(&text).await;
            }
            BridgeInCommand::Cancel => {
                // TODO: Implement cancellation
                bridge.emit(BridgeOutMessage::Error {
                    message: "Cancellation not yet implemented".to_string(),
                });
            }
            BridgeInCommand::SwitchAgent { agent } => {
                bridge.handle_switch_agent(&agent);
            }
            BridgeInCommand::SwitchModel { model } => {
                bridge.handle_switch_model(&model);
            }
            BridgeInCommand::McpStart { name } => {
                if let Some(n) = name {
                    let _ = bridge.mcp_manager.start_server(&n).await;
                } else {
                    let _ = bridge.mcp_manager.start_all().await;
                }
                bridge.handle_mcp_list().await;
            }
            BridgeInCommand::McpStop { name } => {
                if let Some(n) = name {
                    let _ = bridge.mcp_manager.stop_server(&n).await;
                } else {
                    let _ = bridge.mcp_manager.stop_all().await;
                }
                bridge.handle_mcp_list().await;
            }
            BridgeInCommand::McpList => {
                bridge.handle_mcp_list().await;
            }
            BridgeInCommand::ToolResponse { .. } => {
                // TODO: Implement tool confirmation
            }
            BridgeInCommand::Shutdown => {
                let _ = bridge.mcp_manager.stop_all().await;
                break;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::{
        AgentMessage, DiffMessage, FileMessage, FileOperation, InputRequest, InputType,
        MessageLevel, ReasoningMessage, ResponseMessage, ShellMessage, SpinnerMessage,
        TextDeltaMessage, TextMessage, ThinkingMessage, ToolMessage,
    };

    // =========================================================================
    // BridgeOutMessage Serialization Tests
    // =========================================================================

    #[test]
    fn test_out_message_ready_serialization() {
        let msg = BridgeOutMessage::Ready {
            version: "0.5.0".to_string(),
            agent: "stockpot".to_string(),
            model: "gpt-4".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();

        assert!(json.contains(r#""type":"ready""#));
        assert!(json.contains(r#""version":"0.5.0""#));
        assert!(json.contains(r#""agent":"stockpot""#));
        assert!(json.contains(r#""model":"gpt-4""#));
    }

    #[test]
    fn test_out_message_text_delta_serialization() {
        let msg = BridgeOutMessage::TextDelta {
            text: "Hello world".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();

        assert!(json.contains(r#""type":"text_delta""#));
        assert!(json.contains(r#""text":"Hello world""#));
    }

    #[test]
    fn test_out_message_thinking_delta_serialization() {
        let msg = BridgeOutMessage::ThinkingDelta {
            text: "Analyzing the problem...".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();

        assert!(json.contains(r#""type":"thinking_delta""#));
        assert!(json.contains(r#""text":"Analyzing the problem...""#));
    }

    #[test]
    fn test_out_message_tool_call_start_serialization() {
        let msg = BridgeOutMessage::ToolCallStart {
            tool_name: "read_file".to_string(),
            tool_call_id: Some("call-123".to_string()),
        };
        let json = serde_json::to_string(&msg).unwrap();

        assert!(json.contains(r#""type":"tool_call_start""#));
        assert!(json.contains(r#""tool_name":"read_file""#));
        assert!(json.contains(r#""tool_call_id":"call-123""#));
    }

    #[test]
    fn test_out_message_tool_call_start_no_id() {
        let msg = BridgeOutMessage::ToolCallStart {
            tool_name: "grep".to_string(),
            tool_call_id: None,
        };
        let json = serde_json::to_string(&msg).unwrap();

        assert!(json.contains(r#""type":"tool_call_start""#));
        assert!(json.contains(r#""tool_name":"grep""#));
        // tool_call_id should be omitted when None
        assert!(!json.contains("tool_call_id"));
    }

    #[test]
    fn test_out_message_tool_call_delta_serialization() {
        let msg = BridgeOutMessage::ToolCallDelta {
            delta: r#"{"file_path": "/test"#.to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();

        assert!(json.contains(r#""type":"tool_call_delta""#));
        assert!(json.contains(r#""delta""#));
    }

    #[test]
    fn test_out_message_tool_call_complete_serialization() {
        let msg = BridgeOutMessage::ToolCallComplete {
            tool_name: "write_file".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();

        assert!(json.contains(r#""type":"tool_call_complete""#));
        assert!(json.contains(r#""tool_name":"write_file""#));
    }

    #[test]
    fn test_out_message_tool_executed_success() {
        let msg = BridgeOutMessage::ToolExecuted {
            tool_name: "shell_command".to_string(),
            success: true,
            error: None,
        };
        let json = serde_json::to_string(&msg).unwrap();

        assert!(json.contains(r#""type":"tool_executed""#));
        assert!(json.contains(r#""tool_name":"shell_command""#));
        assert!(json.contains(r#""success":true"#));
        // error should be omitted when None
        assert!(!json.contains(r#""error""#));
    }

    #[test]
    fn test_out_message_tool_executed_failure() {
        let msg = BridgeOutMessage::ToolExecuted {
            tool_name: "read_file".to_string(),
            success: false,
            error: Some("File not found".to_string()),
        };
        let json = serde_json::to_string(&msg).unwrap();

        assert!(json.contains(r#""type":"tool_executed""#));
        assert!(json.contains(r#""success":false"#));
        assert!(json.contains(r#""error":"File not found""#));
    }

    #[test]
    fn test_out_message_request_start_serialization() {
        let msg = BridgeOutMessage::RequestStart { step: 3 };
        let json = serde_json::to_string(&msg).unwrap();

        assert!(json.contains(r#""type":"request_start""#));
        assert!(json.contains(r#""step":3"#));
    }

    #[test]
    fn test_out_message_complete_serialization() {
        let msg = BridgeOutMessage::Complete {
            run_id: "run-abc-123".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();

        assert!(json.contains(r#""type":"complete""#));
        assert!(json.contains(r#""run_id":"run-abc-123""#));
    }

    #[test]
    fn test_out_message_error_serialization() {
        let msg = BridgeOutMessage::Error {
            message: "Something went wrong".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();

        assert!(json.contains(r#""type":"error""#));
        assert!(json.contains(r#""message":"Something went wrong""#));
    }

    #[test]
    fn test_out_message_agent_changed_serialization() {
        let msg = BridgeOutMessage::AgentChanged {
            agent: "explorer".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();

        assert!(json.contains(r#""type":"agent_changed""#));
        assert!(json.contains(r#""agent":"explorer""#));
    }

    #[test]
    fn test_out_message_model_changed_serialization() {
        let msg = BridgeOutMessage::ModelChanged {
            model: "claude-3-opus".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();

        assert!(json.contains(r#""type":"model_changed""#));
        assert!(json.contains(r#""model":"claude-3-opus""#));
    }

    #[test]
    fn test_out_message_mcp_status_serialization() {
        let msg = BridgeOutMessage::McpStatus {
            servers: vec![
                McpServerStatus {
                    name: "filesystem".to_string(),
                    running: true,
                },
                McpServerStatus {
                    name: "github".to_string(),
                    running: false,
                },
            ],
        };
        let json = serde_json::to_string(&msg).unwrap();

        assert!(json.contains(r#""type":"mcp_status""#));
        assert!(json.contains(r#""name":"filesystem""#));
        assert!(json.contains(r#""running":true"#));
        assert!(json.contains(r#""name":"github""#));
        assert!(json.contains(r#""running":false"#));
    }

    #[test]
    fn test_out_message_mcp_status_empty() {
        let msg = BridgeOutMessage::McpStatus { servers: vec![] };
        let json = serde_json::to_string(&msg).unwrap();

        assert!(json.contains(r#""type":"mcp_status""#));
        assert!(json.contains(r#""servers":[]"#));
    }

    // =========================================================================
    // McpServerStatus Tests
    // =========================================================================

    #[test]
    fn test_mcp_server_status_serialization() {
        let status = McpServerStatus {
            name: "test-server".to_string(),
            running: true,
        };
        let json = serde_json::to_string(&status).unwrap();

        assert!(json.contains(r#""name":"test-server""#));
        assert!(json.contains(r#""running":true"#));
    }

    #[test]
    fn test_mcp_server_status_not_running() {
        let status = McpServerStatus {
            name: "stopped-server".to_string(),
            running: false,
        };
        let json = serde_json::to_string(&status).unwrap();

        assert!(json.contains(r#""running":false"#));
    }

    // =========================================================================
    // BridgeInCommand Deserialization Tests
    // =========================================================================

    #[test]
    fn test_in_command_prompt_basic() {
        let json = r#"{"type": "prompt", "text": "Hello world"}"#;
        let cmd: BridgeInCommand = serde_json::from_str(json).unwrap();

        match cmd {
            BridgeInCommand::Prompt { text, agent, model } => {
                assert_eq!(text, "Hello world");
                assert!(agent.is_none());
                assert!(model.is_none());
            }
            _ => panic!("Expected Prompt variant"),
        }
    }

    #[test]
    fn test_in_command_prompt_with_agent() {
        let json = r#"{"type": "prompt", "text": "Analyze code", "agent": "explorer"}"#;
        let cmd: BridgeInCommand = serde_json::from_str(json).unwrap();

        match cmd {
            BridgeInCommand::Prompt { text, agent, model } => {
                assert_eq!(text, "Analyze code");
                assert_eq!(agent, Some("explorer".to_string()));
                assert!(model.is_none());
            }
            _ => panic!("Expected Prompt variant"),
        }
    }

    #[test]
    fn test_in_command_prompt_with_model() {
        let json = r#"{"type": "prompt", "text": "Test", "model": "gpt-4"}"#;
        let cmd: BridgeInCommand = serde_json::from_str(json).unwrap();

        match cmd {
            BridgeInCommand::Prompt { text, agent, model } => {
                assert_eq!(text, "Test");
                assert!(agent.is_none());
                assert_eq!(model, Some("gpt-4".to_string()));
            }
            _ => panic!("Expected Prompt variant"),
        }
    }

    #[test]
    fn test_in_command_prompt_with_all_fields() {
        let json = r#"{"type": "prompt", "text": "Build feature", "agent": "planner", "model": "claude-3"}"#;
        let cmd: BridgeInCommand = serde_json::from_str(json).unwrap();

        match cmd {
            BridgeInCommand::Prompt { text, agent, model } => {
                assert_eq!(text, "Build feature");
                assert_eq!(agent, Some("planner".to_string()));
                assert_eq!(model, Some("claude-3".to_string()));
            }
            _ => panic!("Expected Prompt variant"),
        }
    }

    #[test]
    fn test_in_command_cancel() {
        let json = r#"{"type": "cancel"}"#;
        let cmd: BridgeInCommand = serde_json::from_str(json).unwrap();

        assert!(matches!(cmd, BridgeInCommand::Cancel));
    }

    #[test]
    fn test_in_command_switch_agent() {
        let json = r#"{"type": "switch_agent", "agent": "reviewer"}"#;
        let cmd: BridgeInCommand = serde_json::from_str(json).unwrap();

        match cmd {
            BridgeInCommand::SwitchAgent { agent } => {
                assert_eq!(agent, "reviewer");
            }
            _ => panic!("Expected SwitchAgent variant"),
        }
    }

    #[test]
    fn test_in_command_switch_model() {
        let json = r#"{"type": "switch_model", "model": "gpt-4-turbo"}"#;
        let cmd: BridgeInCommand = serde_json::from_str(json).unwrap();

        match cmd {
            BridgeInCommand::SwitchModel { model } => {
                assert_eq!(model, "gpt-4-turbo");
            }
            _ => panic!("Expected SwitchModel variant"),
        }
    }

    #[test]
    fn test_in_command_mcp_start_named() {
        let json = r#"{"type": "mcp_start", "name": "filesystem"}"#;
        let cmd: BridgeInCommand = serde_json::from_str(json).unwrap();

        match cmd {
            BridgeInCommand::McpStart { name } => {
                assert_eq!(name, Some("filesystem".to_string()));
            }
            _ => panic!("Expected McpStart variant"),
        }
    }

    #[test]
    fn test_in_command_mcp_start_all() {
        let json = r#"{"type": "mcp_start"}"#;
        let cmd: BridgeInCommand = serde_json::from_str(json).unwrap();

        match cmd {
            BridgeInCommand::McpStart { name } => {
                assert!(name.is_none());
            }
            _ => panic!("Expected McpStart variant"),
        }
    }

    #[test]
    fn test_in_command_mcp_stop_named() {
        let json = r#"{"type": "mcp_stop", "name": "github"}"#;
        let cmd: BridgeInCommand = serde_json::from_str(json).unwrap();

        match cmd {
            BridgeInCommand::McpStop { name } => {
                assert_eq!(name, Some("github".to_string()));
            }
            _ => panic!("Expected McpStop variant"),
        }
    }

    #[test]
    fn test_in_command_mcp_stop_all() {
        let json = r#"{"type": "mcp_stop"}"#;
        let cmd: BridgeInCommand = serde_json::from_str(json).unwrap();

        match cmd {
            BridgeInCommand::McpStop { name } => {
                assert!(name.is_none());
            }
            _ => panic!("Expected McpStop variant"),
        }
    }

    #[test]
    fn test_in_command_mcp_list() {
        let json = r#"{"type": "mcp_list"}"#;
        let cmd: BridgeInCommand = serde_json::from_str(json).unwrap();

        assert!(matches!(cmd, BridgeInCommand::McpList));
    }

    #[test]
    fn test_in_command_tool_response_approved() {
        let json = r#"{"type": "tool_response", "call_id": "call-xyz", "approved": true}"#;
        let cmd: BridgeInCommand = serde_json::from_str(json).unwrap();

        match cmd {
            BridgeInCommand::ToolResponse { call_id, approved } => {
                assert_eq!(call_id, "call-xyz");
                assert!(approved);
            }
            _ => panic!("Expected ToolResponse variant"),
        }
    }

    #[test]
    fn test_in_command_tool_response_rejected() {
        let json = r#"{"type": "tool_response", "call_id": "call-abc", "approved": false}"#;
        let cmd: BridgeInCommand = serde_json::from_str(json).unwrap();

        match cmd {
            BridgeInCommand::ToolResponse { call_id, approved } => {
                assert_eq!(call_id, "call-abc");
                assert!(!approved);
            }
            _ => panic!("Expected ToolResponse variant"),
        }
    }

    #[test]
    fn test_in_command_shutdown() {
        let json = r#"{"type": "shutdown"}"#;
        let cmd: BridgeInCommand = serde_json::from_str(json).unwrap();

        assert!(matches!(cmd, BridgeInCommand::Shutdown));
    }

    // =========================================================================
    // BridgeInCommand Error Handling Tests
    // =========================================================================

    #[test]
    fn test_in_command_invalid_type() {
        let json = r#"{"type": "invalid_command"}"#;
        let result = serde_json::from_str::<BridgeInCommand>(json);

        assert!(result.is_err());
    }

    #[test]
    fn test_in_command_missing_type() {
        let json = r#"{"text": "Hello"}"#;
        let result = serde_json::from_str::<BridgeInCommand>(json);

        assert!(result.is_err());
    }

    #[test]
    fn test_in_command_prompt_missing_text() {
        let json = r#"{"type": "prompt"}"#;
        let result = serde_json::from_str::<BridgeInCommand>(json);

        assert!(result.is_err());
    }

    #[test]
    fn test_in_command_switch_agent_missing_agent() {
        let json = r#"{"type": "switch_agent"}"#;
        let result = serde_json::from_str::<BridgeInCommand>(json);

        assert!(result.is_err());
    }

    #[test]
    fn test_in_command_switch_model_missing_model() {
        let json = r#"{"type": "switch_model"}"#;
        let result = serde_json::from_str::<BridgeInCommand>(json);

        assert!(result.is_err());
    }

    #[test]
    fn test_in_command_tool_response_missing_call_id() {
        let json = r#"{"type": "tool_response", "approved": true}"#;
        let result = serde_json::from_str::<BridgeInCommand>(json);

        assert!(result.is_err());
    }

    #[test]
    fn test_in_command_tool_response_missing_approved() {
        let json = r#"{"type": "tool_response", "call_id": "abc"}"#;
        let result = serde_json::from_str::<BridgeInCommand>(json);

        assert!(result.is_err());
    }

    #[test]
    fn test_in_command_malformed_json() {
        let json = r#"{"type": "prompt", "text": }"#;
        let result = serde_json::from_str::<BridgeInCommand>(json);

        assert!(result.is_err());
    }

    #[test]
    fn test_in_command_empty_json() {
        let json = r#"{}"#;
        let result = serde_json::from_str::<BridgeInCommand>(json);

        assert!(result.is_err());
    }

    // =========================================================================
    // BridgeOutMessage::from_message Tests
    // =========================================================================

    #[test]
    fn test_from_message_text_delta() {
        let msg = Message::TextDelta(TextDeltaMessage {
            text: "Hello ".to_string(),
            agent_name: None,
        });

        let bridge_msg = BridgeOutMessage::from_message(msg);
        assert!(bridge_msg.is_some());

        match bridge_msg.unwrap() {
            BridgeOutMessage::TextDelta { text } => {
                assert_eq!(text, "Hello ");
            }
            _ => panic!("Expected TextDelta variant"),
        }
    }

    #[test]
    fn test_from_message_thinking() {
        let msg = Message::Thinking(ThinkingMessage {
            text: "Let me think...".to_string(),
        });

        let bridge_msg = BridgeOutMessage::from_message(msg);
        assert!(bridge_msg.is_some());

        match bridge_msg.unwrap() {
            BridgeOutMessage::ThinkingDelta { text } => {
                assert_eq!(text, "Let me think...");
            }
            _ => panic!("Expected ThinkingDelta variant"),
        }
    }

    #[test]
    fn test_from_message_agent_started_returns_none() {
        let msg = Message::Agent(AgentMessage {
            agent_name: "stockpot".to_string(),
            display_name: "Stockpot".to_string(),
            event: AgentEvent::Started,
        });

        let bridge_msg = BridgeOutMessage::from_message(msg);
        assert!(bridge_msg.is_none()); // Silent - UI tracks from other events
    }

    #[test]
    fn test_from_message_agent_completed() {
        let msg = Message::Agent(AgentMessage {
            agent_name: "stockpot".to_string(),
            display_name: "Stockpot".to_string(),
            event: AgentEvent::Completed {
                run_id: "run-123".to_string(),
            },
        });

        let bridge_msg = BridgeOutMessage::from_message(msg);
        assert!(bridge_msg.is_some());

        match bridge_msg.unwrap() {
            BridgeOutMessage::Complete { run_id } => {
                assert_eq!(run_id, "run-123");
            }
            _ => panic!("Expected Complete variant"),
        }
    }

    #[test]
    fn test_from_message_agent_error() {
        let msg = Message::Agent(AgentMessage {
            agent_name: "stockpot".to_string(),
            display_name: "Stockpot".to_string(),
            event: AgentEvent::Error {
                message: "API error".to_string(),
            },
        });

        let bridge_msg = BridgeOutMessage::from_message(msg);
        assert!(bridge_msg.is_some());

        match bridge_msg.unwrap() {
            BridgeOutMessage::Error { message } => {
                assert_eq!(message, "API error");
            }
            _ => panic!("Expected Error variant"),
        }
    }

    #[test]
    fn test_from_message_tool_started() {
        let msg = Message::Tool(ToolMessage {
            tool_name: "read_file".to_string(),
            tool_call_id: None,
            status: ToolStatus::Started,
            args: None,
            result: None,
            error: None,
            agent_name: None,
        });

        let bridge_msg = BridgeOutMessage::from_message(msg);
        assert!(bridge_msg.is_some());

        match bridge_msg.unwrap() {
            BridgeOutMessage::ToolCallStart {
                tool_name,
                tool_call_id,
            } => {
                assert_eq!(tool_name, "read_file");
                assert!(tool_call_id.is_none());
            }
            _ => panic!("Expected ToolCallStart variant"),
        }
    }

    #[test]
    fn test_from_message_tool_args_streaming_returns_none() {
        let msg = Message::Tool(ToolMessage {
            tool_name: "grep".to_string(),
            tool_call_id: None,
            status: ToolStatus::ArgsStreaming,
            args: None,
            result: None,
            error: None,
            agent_name: None,
        });

        let bridge_msg = BridgeOutMessage::from_message(msg);
        assert!(bridge_msg.is_none()); // No delta available in ToolMessage
    }

    #[test]
    fn test_from_message_tool_executing() {
        let msg = Message::Tool(ToolMessage {
            tool_name: "shell_command".to_string(),
            tool_call_id: Some("call-123".to_string()),
            status: ToolStatus::Executing,
            args: Some(serde_json::json!({"command": "ls"})),
            result: None,
            error: None,
            agent_name: None,
        });

        let bridge_msg = BridgeOutMessage::from_message(msg);
        assert!(bridge_msg.is_some());

        match bridge_msg.unwrap() {
            BridgeOutMessage::ToolCallComplete { tool_name } => {
                assert_eq!(tool_name, "shell_command");
            }
            _ => panic!("Expected ToolCallComplete variant"),
        }
    }

    #[test]
    fn test_from_message_tool_completed() {
        let msg = Message::Tool(ToolMessage {
            tool_name: "write_file".to_string(),
            tool_call_id: None,
            status: ToolStatus::Completed,
            args: None,
            result: Some("File written".to_string()),
            error: None,
            agent_name: None,
        });

        let bridge_msg = BridgeOutMessage::from_message(msg);
        assert!(bridge_msg.is_some());

        match bridge_msg.unwrap() {
            BridgeOutMessage::ToolExecuted {
                tool_name,
                success,
                error,
            } => {
                assert_eq!(tool_name, "write_file");
                assert!(success);
                assert!(error.is_none());
            }
            _ => panic!("Expected ToolExecuted variant"),
        }
    }

    #[test]
    fn test_from_message_tool_failed() {
        let msg = Message::Tool(ToolMessage {
            tool_name: "read_file".to_string(),
            tool_call_id: None,
            status: ToolStatus::Failed,
            args: None,
            result: None,
            error: Some("Permission denied".to_string()),
            agent_name: None,
        });

        let bridge_msg = BridgeOutMessage::from_message(msg);
        assert!(bridge_msg.is_some());

        match bridge_msg.unwrap() {
            BridgeOutMessage::ToolExecuted {
                tool_name,
                success,
                error,
            } => {
                assert_eq!(tool_name, "read_file");
                assert!(!success);
                assert_eq!(error, Some("Permission denied".to_string()));
            }
            _ => panic!("Expected ToolExecuted variant"),
        }
    }

    #[test]
    fn test_from_message_divider_returns_none() {
        let msg = Message::Divider;
        let bridge_msg = BridgeOutMessage::from_message(msg);
        assert!(bridge_msg.is_none());
    }

    #[test]
    fn test_from_message_clear_returns_none() {
        let msg = Message::Clear;
        let bridge_msg = BridgeOutMessage::from_message(msg);
        assert!(bridge_msg.is_none());
    }

    // =========================================================================
    // NDJSON Format Tests
    // =========================================================================

    #[test]
    fn test_ndjson_output_format() {
        // Verify messages serialize to single-line JSON (no embedded newlines in output)
        let msg = BridgeOutMessage::TextDelta {
            text: "Line 1\nLine 2".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();

        // Should not contain literal newlines (they should be escaped)
        let newline_count = json.matches('\n').count();
        assert_eq!(
            newline_count, 0,
            "NDJSON should not contain literal newlines"
        );

        // Should contain escaped newlines
        assert!(json.contains("\\n"));
    }

    #[test]
    fn test_ndjson_special_characters() {
        let msg = BridgeOutMessage::Error {
            message: r#"Quote: "test", backslash: \, tab: 	"#.to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();

        // Should be valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.is_object());
    }

    #[test]
    fn test_ndjson_unicode() {
        let msg = BridgeOutMessage::TextDelta {
            text: "Hello 世界 🌍".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();

        // Should round-trip correctly
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["text"].as_str().unwrap(), "Hello 世界 🌍");
    }

    // =========================================================================
    // Edge Cases
    // =========================================================================

    #[test]
    fn test_empty_text_delta() {
        let msg = BridgeOutMessage::TextDelta {
            text: String::new(),
        };
        let json = serde_json::to_string(&msg).unwrap();

        assert!(json.contains(r#""text":"""#));
    }

    #[test]
    fn test_very_long_text() {
        let long_text = "a".repeat(100_000);
        let msg = BridgeOutMessage::TextDelta { text: long_text };
        let json = serde_json::to_string(&msg).unwrap();

        // Should serialize without error
        assert!(json.len() > 100_000);
    }

    #[test]
    fn test_prompt_with_empty_text() {
        let json = r#"{"type": "prompt", "text": ""}"#;
        let cmd: BridgeInCommand = serde_json::from_str(json).unwrap();

        match cmd {
            BridgeInCommand::Prompt { text, .. } => {
                assert!(text.is_empty());
            }
            _ => panic!("Expected Prompt variant"),
        }
    }

    #[test]
    fn test_in_command_with_extra_fields() {
        // Extra fields should be ignored
        let json = r#"{"type": "cancel", "extra_field": "ignored"}"#;
        let cmd: BridgeInCommand = serde_json::from_str(json).unwrap();

        assert!(matches!(cmd, BridgeInCommand::Cancel));
    }

    #[test]
    fn test_tool_executed_no_error_field() {
        // Verify error field is omitted in JSON when None
        let msg = BridgeOutMessage::ToolExecuted {
            tool_name: "test".to_string(),
            success: true,
            error: None,
        };
        let json = serde_json::to_string(&msg).unwrap();

        // Parse as generic JSON to check field absence
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.get("error").is_none());
    }

    // =========================================================================
    // Round-trip Deserialization Tests (BridgeOutMessage)
    // =========================================================================

    #[test]
    fn test_out_message_roundtrip_ready() {
        let original = BridgeOutMessage::Ready {
            version: "1.0.0".to_string(),
            agent: "test-agent".to_string(),
            model: "test-model".to_string(),
        };
        let json = serde_json::to_string(&original).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["type"], "ready");
        assert_eq!(parsed["version"], "1.0.0");
        assert_eq!(parsed["agent"], "test-agent");
        assert_eq!(parsed["model"], "test-model");
    }

    #[test]
    fn test_out_message_roundtrip_all_variants() {
        // Test all variants serialize to valid JSON and back
        let messages: Vec<BridgeOutMessage> = vec![
            BridgeOutMessage::Ready {
                version: "0.1.0".to_string(),
                agent: "a".to_string(),
                model: "m".to_string(),
            },
            BridgeOutMessage::TextDelta {
                text: "t".to_string(),
            },
            BridgeOutMessage::ThinkingDelta {
                text: "th".to_string(),
            },
            BridgeOutMessage::ToolCallStart {
                tool_name: "tool".to_string(),
                tool_call_id: Some("id".to_string()),
            },
            BridgeOutMessage::ToolCallDelta {
                delta: "d".to_string(),
            },
            BridgeOutMessage::ToolCallComplete {
                tool_name: "tool".to_string(),
            },
            BridgeOutMessage::ToolExecuted {
                tool_name: "tool".to_string(),
                success: true,
                error: None,
            },
            BridgeOutMessage::RequestStart { step: 1 },
            BridgeOutMessage::Complete {
                run_id: "run".to_string(),
            },
            BridgeOutMessage::Error {
                message: "err".to_string(),
            },
            BridgeOutMessage::AgentChanged {
                agent: "new".to_string(),
            },
            BridgeOutMessage::ModelChanged {
                model: "new".to_string(),
            },
            BridgeOutMessage::McpStatus { servers: vec![] },
        ];

        for msg in messages {
            let json = serde_json::to_string(&msg).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
            assert!(parsed.is_object(), "Message should serialize to object");
            assert!(
                parsed.get("type").is_some(),
                "Message should have type field"
            );
        }
    }

    // =========================================================================
    // Additional from_message Edge Cases
    // =========================================================================

    #[test]
    fn test_from_message_text_delta_with_agent_name() {
        // agent_name field is ignored in bridge conversion
        let msg = Message::TextDelta(TextDeltaMessage {
            text: "test".to_string(),
            agent_name: Some("nested-agent".to_string()),
        });

        let bridge_msg = BridgeOutMessage::from_message(msg);
        assert!(bridge_msg.is_some());

        match bridge_msg.unwrap() {
            BridgeOutMessage::TextDelta { text } => {
                assert_eq!(text, "test");
            }
            _ => panic!("Expected TextDelta variant"),
        }
    }

    #[test]
    fn test_from_message_tool_with_result() {
        let msg = Message::Tool(ToolMessage {
            tool_name: "write_file".to_string(),
            tool_call_id: Some("call-abc".to_string()),
            status: ToolStatus::Completed,
            args: Some(serde_json::json!({"path": "/test.txt", "content": "hello"})),
            result: Some("Successfully wrote 5 bytes".to_string()),
            error: None,
            agent_name: Some("main-agent".to_string()),
        });

        let bridge_msg = BridgeOutMessage::from_message(msg);
        assert!(bridge_msg.is_some());

        match bridge_msg.unwrap() {
            BridgeOutMessage::ToolExecuted {
                tool_name,
                success,
                error,
            } => {
                assert_eq!(tool_name, "write_file");
                assert!(success);
                assert!(error.is_none());
            }
            _ => panic!("Expected ToolExecuted variant"),
        }
    }

    #[test]
    fn test_from_message_tool_failed_with_no_error_text() {
        // Tool can fail without error message (edge case)
        let msg = Message::Tool(ToolMessage {
            tool_name: "read_file".to_string(),
            tool_call_id: None,
            status: ToolStatus::Failed,
            args: None,
            result: None,
            error: None, // No error message
            agent_name: None,
        });

        let bridge_msg = BridgeOutMessage::from_message(msg);
        assert!(bridge_msg.is_some());

        match bridge_msg.unwrap() {
            BridgeOutMessage::ToolExecuted { success, error, .. } => {
                assert!(!success);
                assert!(error.is_none());
            }
            _ => panic!("Expected ToolExecuted variant"),
        }
    }

    // =========================================================================
    // Additional Message Mapping Tests
    // =========================================================================

    #[test]
    fn test_from_message_text_message_returns_none() {
        let msg = Message::Text(TextMessage {
            level: MessageLevel::Info,
            text: "info".to_string(),
        });
        assert!(BridgeOutMessage::from_message(msg).is_none());
    }

    #[test]
    fn test_from_message_reasoning_returns_none() {
        let msg = Message::Reasoning(ReasoningMessage {
            reasoning: "thinking".to_string(),
            next_steps: None,
        });
        assert!(BridgeOutMessage::from_message(msg).is_none());
    }

    #[test]
    fn test_from_message_response_returns_none() {
        let msg = Message::Response(ResponseMessage {
            content: "response".to_string(),
            is_streaming: false,
        });
        assert!(BridgeOutMessage::from_message(msg).is_none());
    }

    #[test]
    fn test_from_message_shell_returns_none() {
        let msg = Message::Shell(ShellMessage {
            command: "ls".to_string(),
            output: None,
            exit_code: None,
            is_running: true,
        });
        assert!(BridgeOutMessage::from_message(msg).is_none());
    }

    #[test]
    fn test_from_message_file_returns_none() {
        let msg = Message::File(FileMessage {
            operation: FileOperation::Read,
            path: "/test".to_string(),
            content: None,
            error: None,
        });
        assert!(BridgeOutMessage::from_message(msg).is_none());
    }

    #[test]
    fn test_from_message_diff_returns_none() {
        let msg = Message::Diff(DiffMessage {
            path: "test.rs".to_string(),
            lines: vec![],
        });
        assert!(BridgeOutMessage::from_message(msg).is_none());
    }

    #[test]
    fn test_from_message_spinner_returns_none() {
        let msg = Message::Spinner(SpinnerMessage {
            text: "loading".to_string(),
            is_active: true,
        });
        assert!(BridgeOutMessage::from_message(msg).is_none());
    }

    #[test]
    fn test_from_message_input_request_returns_none() {
        let msg = Message::InputRequest(InputRequest {
            prompt: "?".to_string(),
            request_type: InputType::Text,
            options: None,
        });
        assert!(BridgeOutMessage::from_message(msg).is_none());
    }

    // =========================================================================
    // JSON Structure Validation Tests
    // =========================================================================

    #[test]
    fn test_json_field_order_consistency() {
        // Verify type field is always present and first-ish (serde should be consistent)
        let msg = BridgeOutMessage::TextDelta {
            text: "test".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();

        // Parse and verify structure
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(value.is_object());
        assert_eq!(value.as_object().unwrap().len(), 2); // type + text
    }

    #[test]
    fn test_json_null_vs_absent() {
        // Verify skip_serializing_if works correctly - None should be absent, not null
        let msg = BridgeOutMessage::ToolCallStart {
            tool_name: "test".to_string(),
            tool_call_id: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Field should be completely absent, not null
        assert!(!json.contains("tool_call_id"));
        assert!(value.get("tool_call_id").is_none());
    }

    #[test]
    fn test_json_with_tool_call_id_present() {
        let msg = BridgeOutMessage::ToolCallStart {
            tool_name: "test".to_string(),
            tool_call_id: Some("id-123".to_string()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert!(json.contains("tool_call_id"));
        assert_eq!(value.get("tool_call_id").unwrap().as_str(), Some("id-123"));
    }

    // =========================================================================
    // Whitespace and Formatting Edge Cases
    // =========================================================================

    #[test]
    fn test_in_command_with_whitespace_in_values() {
        let json = r#"{"type": "prompt", "text": "  leading and trailing spaces  "}"#;
        let cmd: BridgeInCommand = serde_json::from_str(json).unwrap();

        match cmd {
            BridgeInCommand::Prompt { text, .. } => {
                assert_eq!(text, "  leading and trailing spaces  ");
            }
            _ => panic!("Expected Prompt variant"),
        }
    }

    #[test]
    fn test_in_command_minified_json() {
        let json = r#"{"type":"prompt","text":"test","agent":"a","model":"m"}"#;
        let cmd: BridgeInCommand = serde_json::from_str(json).unwrap();

        match cmd {
            BridgeInCommand::Prompt { text, agent, model } => {
                assert_eq!(text, "test");
                assert_eq!(agent, Some("a".to_string()));
                assert_eq!(model, Some("m".to_string()));
            }
            _ => panic!("Expected Prompt variant"),
        }
    }

    #[test]
    fn test_in_command_with_extra_whitespace() {
        let json = r#"{
            "type"   :   "cancel"
        }"#;
        let cmd: BridgeInCommand = serde_json::from_str(json).unwrap();
        assert!(matches!(cmd, BridgeInCommand::Cancel));
    }

    // =========================================================================
    // Debug Trait Tests
    // =========================================================================

    #[test]
    fn test_bridge_out_message_debug() {
        let msg = BridgeOutMessage::Ready {
            version: "1.0.0".to_string(),
            agent: "test".to_string(),
            model: "model".to_string(),
        };
        let debug_str = format!("{:?}", msg);
        assert!(debug_str.contains("Ready"));
        assert!(debug_str.contains("1.0.0"));
    }

    #[test]
    fn test_bridge_in_command_debug() {
        let cmd = BridgeInCommand::Prompt {
            text: "hello".to_string(),
            agent: None,
            model: None,
        };
        let debug_str = format!("{:?}", cmd);
        assert!(debug_str.contains("Prompt"));
        assert!(debug_str.contains("hello"));
    }

    #[test]
    fn test_mcp_server_status_debug() {
        let status = McpServerStatus {
            name: "test".to_string(),
            running: true,
        };
        let debug_str = format!("{:?}", status);
        assert!(debug_str.contains("McpServerStatus"));
        assert!(debug_str.contains("test"));
    }

    // =========================================================================
    // Clone Trait Tests
    // =========================================================================

    #[test]
    fn test_bridge_out_message_clone() {
        let original = BridgeOutMessage::Error {
            message: "test error".to_string(),
        };
        let cloned = original.clone();

        let original_json = serde_json::to_string(&original).unwrap();
        let cloned_json = serde_json::to_string(&cloned).unwrap();
        assert_eq!(original_json, cloned_json);
    }

    #[test]
    fn test_bridge_in_command_clone() {
        let original = BridgeInCommand::ToolResponse {
            call_id: "call-1".to_string(),
            approved: true,
        };
        let cloned = original.clone();

        match (&original, &cloned) {
            (
                BridgeInCommand::ToolResponse {
                    call_id: id1,
                    approved: a1,
                },
                BridgeInCommand::ToolResponse {
                    call_id: id2,
                    approved: a2,
                },
            ) => {
                assert_eq!(id1, id2);
                assert_eq!(a1, a2);
            }
            _ => panic!("Clone should preserve variant"),
        }
    }

    #[test]
    fn test_mcp_server_status_clone() {
        let original = McpServerStatus {
            name: "server".to_string(),
            running: false,
        };
        let cloned = original.clone();

        assert_eq!(original.name, cloned.name);
        assert_eq!(original.running, cloned.running);
    }

    // =========================================================================
    // Boundary Value Tests
    // =========================================================================

    #[test]
    fn test_step_zero() {
        let msg = BridgeOutMessage::RequestStart { step: 0 };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""step":0"#));
    }

    #[test]
    fn test_step_max() {
        let msg = BridgeOutMessage::RequestStart { step: u32::MAX };
        let json = serde_json::to_string(&msg).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["step"], u32::MAX);
    }

    #[test]
    fn test_empty_agent_name() {
        let msg = BridgeOutMessage::AgentChanged {
            agent: String::new(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""agent":"""#));
    }

    #[test]
    fn test_empty_run_id() {
        let msg = BridgeOutMessage::Complete {
            run_id: String::new(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""run_id":"""#));
    }

    // =========================================================================
    // Special Character Handling in Commands
    // =========================================================================

    #[test]
    fn test_prompt_with_json_in_text() {
        // Ensure nested JSON in text doesn't break parsing
        let json = r#"{"type": "prompt", "text": "{\"key\": \"value\"}"}"#;
        let cmd: BridgeInCommand = serde_json::from_str(json).unwrap();

        match cmd {
            BridgeInCommand::Prompt { text, .. } => {
                assert_eq!(text, r#"{"key": "value"}"#);
            }
            _ => panic!("Expected Prompt variant"),
        }
    }

    #[test]
    fn test_prompt_with_backslash() {
        let json = r#"{"type": "prompt", "text": "path\\to\\file"}"#;
        let cmd: BridgeInCommand = serde_json::from_str(json).unwrap();

        match cmd {
            BridgeInCommand::Prompt { text, .. } => {
                assert_eq!(text, r"path\to\file");
            }
            _ => panic!("Expected Prompt variant"),
        }
    }

    #[test]
    fn test_agent_name_with_special_chars() {
        let json = r#"{"type": "switch_agent", "agent": "my-agent_v2.0"}"#;
        let cmd: BridgeInCommand = serde_json::from_str(json).unwrap();

        match cmd {
            BridgeInCommand::SwitchAgent { agent } => {
                assert_eq!(agent, "my-agent_v2.0");
            }
            _ => panic!("Expected SwitchAgent variant"),
        }
    }

    // =========================================================================
    // Type Tag Validation
    // =========================================================================

    #[test]
    fn test_all_out_message_type_tags() {
        // Verify snake_case type tag for each variant
        let test_cases = vec![
            (
                BridgeOutMessage::Ready {
                    version: "".into(),
                    agent: "".into(),
                    model: "".into(),
                },
                "ready",
            ),
            (
                BridgeOutMessage::TextDelta { text: "".into() },
                "text_delta",
            ),
            (
                BridgeOutMessage::ThinkingDelta { text: "".into() },
                "thinking_delta",
            ),
            (
                BridgeOutMessage::ToolCallStart {
                    tool_name: "".into(),
                    tool_call_id: None,
                },
                "tool_call_start",
            ),
            (
                BridgeOutMessage::ToolCallDelta { delta: "".into() },
                "tool_call_delta",
            ),
            (
                BridgeOutMessage::ToolCallComplete {
                    tool_name: "".into(),
                },
                "tool_call_complete",
            ),
            (
                BridgeOutMessage::ToolExecuted {
                    tool_name: "".into(),
                    success: true,
                    error: None,
                },
                "tool_executed",
            ),
            (BridgeOutMessage::RequestStart { step: 0 }, "request_start"),
            (BridgeOutMessage::Complete { run_id: "".into() }, "complete"),
            (BridgeOutMessage::Error { message: "".into() }, "error"),
            (
                BridgeOutMessage::AgentChanged { agent: "".into() },
                "agent_changed",
            ),
            (
                BridgeOutMessage::ModelChanged { model: "".into() },
                "model_changed",
            ),
            (
                BridgeOutMessage::McpStatus { servers: vec![] },
                "mcp_status",
            ),
        ];

        for (msg, expected_type) in test_cases {
            let json = serde_json::to_string(&msg).unwrap();
            let value: serde_json::Value = serde_json::from_str(&json).unwrap();
            assert_eq!(
                value["type"].as_str().unwrap(),
                expected_type,
                "Type tag mismatch for {:?}",
                msg
            );
        }
    }

    #[test]
    fn test_all_in_command_type_tags() {
        // Verify snake_case type tag parsing for each variant
        let test_cases = vec![
            (r#"{"type": "prompt", "text": ""}"#, "prompt"),
            (r#"{"type": "cancel"}"#, "cancel"),
            (r#"{"type": "switch_agent", "agent": ""}"#, "switch_agent"),
            (r#"{"type": "switch_model", "model": ""}"#, "switch_model"),
            (r#"{"type": "mcp_start"}"#, "mcp_start"),
            (r#"{"type": "mcp_stop"}"#, "mcp_stop"),
            (r#"{"type": "mcp_list"}"#, "mcp_list"),
            (
                r#"{"type": "tool_response", "call_id": "", "approved": true}"#,
                "tool_response",
            ),
            (r#"{"type": "shutdown"}"#, "shutdown"),
        ];

        for (json, expected_type) in test_cases {
            let result = serde_json::from_str::<BridgeInCommand>(json);
            assert!(
                result.is_ok(),
                "Failed to parse type '{}': {:?}",
                expected_type,
                result.err()
            );
        }
    }
}
