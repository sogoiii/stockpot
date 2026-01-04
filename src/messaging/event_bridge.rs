//! Bridge between serdes_ai_agent StreamEvents and the Message bus.
//!
//! This module provides the conversion layer that translates execution events
//! from the agent runtime into UI-agnostic messages that can be rendered
//! by any subscriber (terminal, web UI, etc.).

use super::{Message, MessageSender};
use serdes_ai_agent::AgentStreamEvent as StreamEvent;
use std::collections::HashMap;

/// Converts StreamEvents to Messages and publishes to the message bus.
///
/// Tracks state across events (e.g., accumulating tool args) to produce
/// well-formed messages.
pub struct EventBridge {
    sender: MessageSender,
    agent_name: String,
    agent_display_name: String,
    /// Track multiple in-progress tool calls by tool_call_id (or tool_name as fallback)
    tool_states: HashMap<String, CurrentToolState>,
    /// Whether we've sent the first text (for agent header)
    first_text_sent: bool,
}

/// State for tracking an in-progress tool call.
struct CurrentToolState {
    name: String,
    tool_call_id: Option<String>,
    args_buffer: String,
}

impl EventBridge {
    /// Create a new event bridge for an agent.
    pub fn new(sender: MessageSender, agent_name: &str, display_name: &str) -> Self {
        Self {
            sender,
            agent_name: agent_name.to_string(),
            agent_display_name: display_name.to_string(),
            tool_states: HashMap::new(),
            first_text_sent: false,
        }
    }

    /// Get the agent name.
    pub fn agent_name(&self) -> &str {
        &self.agent_name
    }

    /// Signal that the agent has started execution.
    pub fn agent_started(&self) {
        let _ = self.sender.send(Message::agent_started(
            &self.agent_name,
            &self.agent_display_name,
        ));
    }

    /// Signal that the agent has completed execution.
    pub fn agent_completed(&self, run_id: &str) {
        let _ = self.sender.send(Message::agent_completed(
            &self.agent_name,
            &self.agent_display_name,
            run_id,
        ));
    }

    /// Signal that the agent encountered an error.
    pub fn agent_error(&self, error: &str) {
        let _ = self.sender.send(Message::agent_error(
            &self.agent_name,
            &self.agent_display_name,
            error,
        ));
    }

    /// Process a stream event and publish appropriate messages.
    ///
    /// This is the main entry point - call this for each event from the stream.
    pub fn process(&mut self, event: StreamEvent) {
        match event {
            StreamEvent::RunStart { .. } => {
                // Already handled by agent_started()
            }

            StreamEvent::RequestStart { step } => {
                // Could emit a step indicator if needed
                // For now, silent
                let _ = step;
            }

            StreamEvent::TextDelta { text } => {
                // Send as text delta with agent attribution
                let _ = self
                    .sender
                    .send(Message::text_delta_from(&text, &self.agent_name));
                self.first_text_sent = true;
            }

            StreamEvent::ThinkingDelta { text } => {
                let _ = self.sender.send(Message::thinking(&text));
            }

            StreamEvent::ToolCallStart {
                tool_name,
                tool_call_id,
            } => {
                // Use tool_call_id as key, or generate one from tool_name + count
                let key = tool_call_id
                    .clone()
                    .unwrap_or_else(|| format!("{}_{}", tool_name, self.tool_states.len()));

                self.tool_states.insert(
                    key,
                    CurrentToolState {
                        name: tool_name.clone(),
                        tool_call_id: tool_call_id.clone(),
                        args_buffer: String::new(),
                    },
                );

                if let Some(ref id) = tool_call_id {
                    let _ = self
                        .sender
                        .send(Message::tool_started_with_id(&tool_name, id));
                } else {
                    let _ = self.sender.send(Message::tool_started(&tool_name));
                }
            }

            StreamEvent::ToolCallDelta {
                delta,
                tool_call_id,
            } => {
                // Accumulate args for the tool with matching tool_call_id, or last started
                if let Some(ref id) = tool_call_id {
                    if let Some(state) = self.tool_states.get_mut(id) {
                        state.args_buffer.push_str(&delta);
                    }
                } else if let Some(state) = self.tool_states.values_mut().last() {
                    state.args_buffer.push_str(&delta);
                }
            }

            StreamEvent::ToolCallComplete {
                tool_name,
                tool_call_id,
            } => {
                // Use provided tool_call_id, or find by name
                let resolved_id = tool_call_id.clone().or_else(|| {
                    self.tool_states
                        .values()
                        .find(|s| s.name == tool_name)
                        .and_then(|s| s.tool_call_id.clone())
                });

                let args = if let Some(ref id) = resolved_id {
                    self.tool_states
                        .get(id)
                        .and_then(|s| serde_json::from_str(&s.args_buffer).ok())
                } else {
                    self.tool_states
                        .values()
                        .find(|s| s.name == tool_name)
                        .and_then(|s| serde_json::from_str(&s.args_buffer).ok())
                };

                if let Some(ref id) = resolved_id {
                    let _ = self
                        .sender
                        .send(Message::tool_executing_with_id(&tool_name, id, args));
                } else {
                    let _ = self.sender.send(Message::tool_executing(&tool_name, args));
                }
            }

            StreamEvent::ToolExecuted {
                tool_name,
                tool_call_id,
                success,
                error,
            } => {
                // Use provided tool_call_id, or find and remove by name
                let resolved_id = tool_call_id.clone().or_else(|| {
                    self.tool_states
                        .iter()
                        .find(|(_, s)| s.name == tool_name)
                        .map(|(k, _)| k.clone())
                });

                // Remove the tool state
                if let Some(ref key) = resolved_id {
                    self.tool_states.remove(key);
                }

                if success {
                    if let Some(ref id) = resolved_id {
                        let _ = self
                            .sender
                            .send(Message::tool_completed_with_id(&tool_name, id));
                    } else {
                        let _ = self.sender.send(Message::tool_completed(&tool_name));
                    }
                } else if let Some(ref id) = resolved_id {
                    let _ = self.sender.send(Message::tool_failed_with_id(
                        &tool_name,
                        id,
                        error.as_deref().unwrap_or("Unknown error"),
                    ));
                } else {
                    let _ = self.sender.send(Message::tool_failed(
                        &tool_name,
                        error.as_deref().unwrap_or("Unknown error"),
                    ));
                }
            }

            StreamEvent::ResponseComplete { .. } => {
                // Internal event, no message needed
            }

            StreamEvent::OutputReady => {
                // Internal event, no message needed
            }

            StreamEvent::RunComplete { run_id, .. } => {
                // Handled by agent_completed() called externally
                let _ = run_id;
            }

            StreamEvent::Error { message } => {
                self.agent_error(&message);
            }
        }
    }

    /// Reset the bridge state (useful for reuse).
    pub fn reset(&mut self) {
        self.tool_states.clear();
        self.first_text_sent = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::MessageBus;

    #[tokio::test]
    async fn test_event_bridge_text_delta() {
        let bus = MessageBus::new();
        let mut receiver = bus.subscribe();
        let mut bridge = EventBridge::new(bus.sender(), "test-agent", "Test Agent");

        bridge.process(StreamEvent::TextDelta {
            text: "Hello".to_string(),
        });

        let msg = receiver.recv().await.unwrap();
        match msg {
            Message::TextDelta(delta) => {
                assert_eq!(delta.text, "Hello");
                assert_eq!(delta.agent_name, Some("test-agent".to_string()));
            }
            _ => panic!("Expected TextDelta message"),
        }
    }

    #[tokio::test]
    async fn test_event_bridge_tool_lifecycle() {
        let bus = MessageBus::new();
        let mut receiver = bus.subscribe();
        let mut bridge = EventBridge::new(bus.sender(), "test-agent", "Test Agent");

        // Simulate tool call sequence
        bridge.process(StreamEvent::ToolCallStart {
            tool_name: "read_file".to_string(),
            tool_call_id: Some("123".to_string()),
        });
        bridge.process(StreamEvent::ToolCallDelta {
            delta: r#"{"file_path":"test.rs"}"#.to_string(),
        });
        bridge.process(StreamEvent::ToolCallComplete {
            tool_name: "read_file".to_string(),
        });
        bridge.process(StreamEvent::ToolExecuted {
            tool_name: "read_file".to_string(),
            success: true,
            error: None,
        });

        // Check messages
        let msg1 = receiver.recv().await.unwrap();
        assert!(matches!(msg1, Message::Tool(t) if t.tool_name == "read_file"));

        let msg2 = receiver.recv().await.unwrap();
        if let Message::Tool(t) = msg2 {
            assert!(t.args.is_some());
        }

        let msg3 = receiver.recv().await.unwrap();
        assert!(matches!(msg3, Message::Tool(t) if t.tool_name == "read_file"));
    }

    #[tokio::test]
    async fn test_event_bridge_agent_lifecycle() {
        let bus = MessageBus::new();
        let mut receiver = bus.subscribe();
        let bridge = EventBridge::new(bus.sender(), "reviewer", "Code Reviewer");

        bridge.agent_started();
        bridge.agent_completed("run-123");

        let msg1 = receiver.recv().await.unwrap();
        assert!(matches!(msg1, Message::Agent(a) if a.agent_name == "reviewer"));

        let msg2 = receiver.recv().await.unwrap();
        if let Message::Agent(a) = msg2 {
            assert!(matches!(
                a.event,
                crate::messaging::AgentEvent::Completed { .. }
            ));
        }
    }

    #[tokio::test]
    async fn test_nested_agent_events() {
        let bus = MessageBus::new();
        let mut receiver = bus.subscribe();

        // Simulate parent agent starting
        let parent_bridge = EventBridge::new(bus.sender(), "parent", "Parent Agent");
        parent_bridge.agent_started();

        // Simulate parent doing some work
        let mut parent_bridge = parent_bridge; // Make mutable for process()
        parent_bridge.process(StreamEvent::TextDelta {
            text: "Let me invoke a sub-agent...".to_string(),
        });

        // Simulate sub-agent starting (same bus!)
        let mut child_bridge = EventBridge::new(bus.sender(), "child", "Child Agent");
        child_bridge.agent_started();
        child_bridge.process(StreamEvent::TextDelta {
            text: "I am the child!".to_string(),
        });
        child_bridge.agent_completed("child-run-123");

        // Parent continues
        parent_bridge.process(StreamEvent::TextDelta {
            text: "Sub-agent finished.".to_string(),
        });
        parent_bridge.agent_completed("parent-run-456");

        // Verify we got all messages in order
        let mut messages = Vec::new();
        for _ in 0..7 {
            // 7 events expected
            if let Ok(msg) =
                tokio::time::timeout(std::time::Duration::from_millis(100), receiver.recv()).await
            {
                messages.push(msg.unwrap());
            }
        }

        assert_eq!(messages.len(), 7);

        // First message should be parent agent started
        assert!(
            matches!(&messages[0], Message::Agent(a) if a.agent_name == "parent"),
            "Expected parent agent started, got {:?}",
            messages[0]
        );

        // Second should be parent text delta
        assert!(
            matches!(&messages[1], Message::TextDelta(d) if d.agent_name == Some("parent".to_string())),
            "Expected parent text delta, got {:?}",
            messages[1]
        );

        // Third should be child agent started
        assert!(
            matches!(&messages[2], Message::Agent(a) if a.agent_name == "child"),
            "Expected child agent started, got {:?}",
            messages[2]
        );

        // Fourth should be child text delta
        assert!(
            matches!(&messages[3], Message::TextDelta(d) if d.agent_name == Some("child".to_string())),
            "Expected child text delta, got {:?}",
            messages[3]
        );

        // Fifth should be child agent completed
        assert!(
            matches!(&messages[4], Message::Agent(a) if a.agent_name == "child"),
            "Expected child agent completed, got {:?}",
            messages[4]
        );

        // Sixth should be parent text delta (after child)
        assert!(
            matches!(&messages[5], Message::TextDelta(d) if d.agent_name == Some("parent".to_string())),
            "Expected parent text delta, got {:?}",
            messages[5]
        );

        // Seventh should be parent agent completed
        assert!(
            matches!(&messages[6], Message::Agent(a) if a.agent_name == "parent"),
            "Expected parent agent completed, got {:?}",
            messages[6]
        );
    }
}
