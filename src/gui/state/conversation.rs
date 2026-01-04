//! Conversation state management
//!
//! Manages chat messages and tool calls for the GUI.

/// Format a tool call as a nice one-liner for display in chat
pub fn format_tool_call_display(name: &str, args: &serde_json::Value) -> String {
    match name {
        "list_files" => {
            let dir = args
                .get("directory")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            let recursive = args
                .get("recursive")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let rec_str = if recursive { " (recursive)" } else { "" };
            format!("📂 `{}`{}", dir, rec_str)
        }
        "read_file" => {
            let path = args
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            format!("📄 `{}`", path)
        }
        "edit_file" => {
            let path = args
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            format!("✏️ `{}`", path)
        }
        "delete_file" => {
            let path = args
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            format!("🗑️ `{}`", path)
        }
        "grep" => {
            let pattern = args
                .get("pattern")
                .or(args.get("search_string"))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let dir = args
                .get("directory")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            format!("🔍 `{}` in `{}`", pattern, dir)
        }
        "run_shell_command" | "agent_run_shell_command" => {
            let cmd = args.get("command").and_then(|v| v.as_str()).unwrap_or("?");
            let preview = if cmd.len() > 60 {
                format!("{}...", &cmd[..57])
            } else {
                cmd.to_string()
            };
            format!("💻 `{}`", preview)
        }
        "invoke_agent" => {
            let agent = args
                .get("agent_name")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            format!("🤖 → {}", agent)
        }
        "agent_share_your_reasoning" => "💭 reasoning...".to_string(),
        _ => {
            // For unknown tools, show name with wrench emoji
            format!("🔧 {}", name)
        }
    }
}

/// A collapsible section containing output from a nested agent
#[derive(Debug, Clone)]
pub struct AgentSection {
    /// Unique ID for this section
    pub id: String,
    /// Agent's internal name
    pub agent_name: String,
    /// Agent's display name (shown in header)
    pub display_name: String,
    /// Content accumulated from this agent
    pub content: String,
    /// Whether the section is collapsed in UI
    pub is_collapsed: bool,
    /// Whether the agent has completed
    pub is_complete: bool,
}

impl AgentSection {
    pub fn new(agent_name: impl Into<String>, display_name: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            agent_name: agent_name.into(),
            display_name: display_name.into(),
            content: String::new(),
            is_collapsed: false,
            is_complete: false,
        }
    }

    pub fn append(&mut self, text: &str) {
        self.content.push_str(text);
    }

    pub fn finish(&mut self) {
        self.is_complete = true;
    }

    pub fn toggle_collapsed(&mut self) {
        self.is_collapsed = !self.is_collapsed;
    }
}

/// A section within an assistant message
#[derive(Debug, Clone)]
pub enum MessageSection {
    /// Plain text/markdown content
    Text(String),
    /// Nested agent output (collapsible)
    NestedAgent(AgentSection),
}

impl MessageSection {
    /// Returns true if this is a Text section
    pub fn is_text(&self) -> bool {
        matches!(self, MessageSection::Text(_))
    }

    /// Returns true if this is a NestedAgent section
    pub fn is_nested_agent(&self) -> bool {
        matches!(self, MessageSection::NestedAgent(_))
    }

    /// Get the section ID if it's a nested agent section
    pub fn agent_section_id(&self) -> Option<&str> {
        match self {
            MessageSection::NestedAgent(section) => Some(&section.id),
            _ => None,
        }
    }
}

/// Role of a message sender
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

/// State of a tool call
#[derive(Debug, Clone)]
pub enum ToolCallState {
    Pending,
    Running,
    Success { output: String },
    Error { message: String },
}

/// A tool call within an assistant message
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
    pub state: ToolCallState,
}

/// A single chat message
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub id: String,
    pub role: MessageRole,
    /// Legacy content field - kept for compatibility, represents flattened view
    pub content: String,
    /// Structured sections (for assistant messages with nested agents)
    pub sections: Vec<MessageSection>,
    pub tool_calls: Vec<ToolCall>,
    pub is_streaming: bool,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        let content_str = content.into();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            role: MessageRole::User,
            content: content_str.clone(),
            sections: vec![MessageSection::Text(content_str)],
            tool_calls: vec![],
            is_streaming: false,
        }
    }

    pub fn assistant() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            role: MessageRole::Assistant,
            content: String::new(),
            sections: vec![],
            tool_calls: vec![],
            is_streaming: true,
        }
    }

    /// Append content to the legacy content field (for backward compatibility)
    pub fn append_content(&mut self, text: &str) {
        self.content.push_str(text);
    }

    pub fn finish_streaming(&mut self) {
        self.is_streaming = false;
    }

    /// Append text to the current active section (last Text section, or creates one)
    pub fn append_to_section(&mut self, text: &str) {
        // Also update the legacy content field
        self.content.push_str(text);

        // Find or create a Text section to append to
        if let Some(MessageSection::Text(ref mut existing)) = self.sections.last_mut() {
            existing.push_str(text);
        } else {
            // Last section is either NestedAgent or there are no sections - create new Text
            self.sections.push(MessageSection::Text(text.to_string()));
        }
    }

    /// Start a new nested agent section, returns the section ID
    pub fn start_nested_section(&mut self, agent_name: &str, display_name: &str) -> String {
        let section = AgentSection::new(agent_name, display_name);
        let id = section.id.clone();
        self.sections.push(MessageSection::NestedAgent(section));
        id
    }

    /// Append text to a specific nested agent section by ID
    pub fn append_to_nested_section(&mut self, section_id: &str, text: &str) {
        // Also update legacy content for flattened view
        self.content.push_str(text);

        for section in &mut self.sections {
            if let MessageSection::NestedAgent(ref mut agent) = section {
                if agent.id == section_id {
                    agent.append(text);
                    return;
                }
            }
        }
    }

    /// Mark a nested section as complete
    pub fn finish_nested_section(&mut self, section_id: &str) {
        for section in &mut self.sections {
            if let MessageSection::NestedAgent(ref mut agent) = section {
                if agent.id == section_id {
                    agent.finish();
                    return;
                }
            }
        }
    }

    /// Toggle the collapsed state of a section
    pub fn toggle_section_collapsed(&mut self, section_id: &str) {
        for section in &mut self.sections {
            if let MessageSection::NestedAgent(ref mut agent) = section {
                if agent.id == section_id {
                    agent.toggle_collapsed();
                    return;
                }
            }
        }
    }

    /// Get a reference to a nested agent section by ID
    pub fn get_nested_section(&self, section_id: &str) -> Option<&AgentSection> {
        for section in &self.sections {
            if let MessageSection::NestedAgent(ref agent) = section {
                if agent.id == section_id {
                    return Some(agent);
                }
            }
        }
        None
    }

    /// Get a mutable reference to a nested agent section by ID
    pub fn get_nested_section_mut(&mut self, section_id: &str) -> Option<&mut AgentSection> {
        for section in &mut self.sections {
            if let MessageSection::NestedAgent(ref mut agent) = section {
                if agent.id == section_id {
                    return Some(agent);
                }
            }
        }
        None
    }

    /// Check if the message has any nested agent sections
    pub fn has_nested_sections(&self) -> bool {
        self.sections.iter().any(|s| s.is_nested_agent())
    }

    /// Get the currently active nested section ID (if any, and if not complete)
    pub fn active_nested_section_id(&self) -> Option<&str> {
        for section in self.sections.iter().rev() {
            if let MessageSection::NestedAgent(ref agent) = section {
                if !agent.is_complete {
                    return Some(&agent.id);
                }
            }
        }
        None
    }
}

/// A conversation (list of messages)
#[derive(Debug, Clone, Default)]
pub struct Conversation {
    pub messages: Vec<ChatMessage>,
    pub is_generating: bool,
}

impl Conversation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_user_message(&mut self, content: impl Into<String>) {
        self.messages.push(ChatMessage::user(content));
    }

    pub fn start_assistant_message(&mut self) {
        self.messages.push(ChatMessage::assistant());
        self.is_generating = true;
    }

    /// Append text to the current message, respecting section structure.
    /// If there's an active (incomplete) nested section, appends there.
    /// Otherwise appends to the main text section.
    pub fn append_to_current(&mut self, text: &str) {
        if let Some(msg) = self.messages.last_mut() {
            // Check if there's an active nested section
            if let Some(section_id) = msg.active_nested_section_id().map(String::from) {
                msg.append_to_nested_section(&section_id, text);
            } else {
                msg.append_to_section(text);
            }
        }
    }

    /// Append text directly to the main content (bypassing nested sections)
    pub fn append_to_main_content(&mut self, text: &str) {
        if let Some(msg) = self.messages.last_mut() {
            msg.append_to_section(text);
        }
    }

    pub fn finish_current_message(&mut self) {
        if let Some(msg) = self.messages.last_mut() {
            msg.finish_streaming();
        }
        self.is_generating = false;
    }

    /// Start a nested agent section in the current message.
    /// Returns the section ID if successful.
    pub fn start_nested_agent(&mut self, agent_name: &str, display_name: &str) -> Option<String> {
        self.messages
            .last_mut()
            .map(|msg| msg.start_nested_section(agent_name, display_name))
    }

    /// Append text to a specific nested agent section
    pub fn append_to_nested_agent(&mut self, section_id: &str, text: &str) {
        if let Some(msg) = self.messages.last_mut() {
            msg.append_to_nested_section(section_id, text);
        }
    }

    /// Mark a nested agent section as complete
    pub fn finish_nested_agent(&mut self, section_id: &str) {
        if let Some(msg) = self.messages.last_mut() {
            msg.finish_nested_section(section_id);
        }
    }

    /// Toggle the collapsed state of a nested section
    pub fn toggle_section_collapsed(&mut self, section_id: &str) {
        // Search through all messages for the section
        for msg in &mut self.messages {
            if msg.get_nested_section(section_id).is_some() {
                msg.toggle_section_collapsed(section_id);
                return;
            }
        }
    }

    /// Set the collapsed state of a nested section explicitly
    pub fn set_section_collapsed(&mut self, section_id: &str, collapsed: bool) {
        // Search through all messages for the section
        for msg in &mut self.messages {
            if let Some(section) = msg.get_nested_section_mut(section_id) {
                section.is_collapsed = collapsed;
                return;
            }
        }
    }

    /// Get the currently active nested section ID (if any)
    pub fn active_nested_section_id(&self) -> Option<&str> {
        self.messages
            .last()
            .and_then(|msg| msg.active_nested_section_id())
    }

    pub fn add_tool_call(&mut self, id: String, name: String, arguments: String) {
        if let Some(msg) = self.messages.last_mut() {
            msg.tool_calls.push(ToolCall {
                id,
                name,
                arguments,
                state: ToolCallState::Pending,
            });
        }
    }

    pub fn update_tool_call(&mut self, id: &str, state: ToolCallState) {
        if let Some(msg) = self.messages.last_mut() {
            if let Some(tool) = msg.tool_calls.iter_mut().find(|t| t.id == id) {
                tool.state = state;
            }
        }
    }

    pub fn clear(&mut self) {
        self.messages.clear();
        self.is_generating = false;
    }

    /// Append a tool call marker to the current message
    pub fn append_tool_call(&mut self, name: &str, args: Option<serde_json::Value>) {
        let args = args.unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
        let display = format_tool_call_display(name, &args);
        let marker = format!("\n{}\n", display);
        self.append_to_main_content(&marker);
    }

    /// Append a tool call marker to a specific nested section
    pub fn append_tool_call_to_section(
        &mut self,
        section_id: &str,
        name: &str,
        args: Option<serde_json::Value>,
    ) {
        let args = args.unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
        let display = format_tool_call_display(name, &args);
        let marker = format!("\n{}\n", display);
        self.append_to_nested_agent(section_id, &marker);
    }

    /// Mark the last tool call as completed with optional result indicator
    pub fn complete_tool_call(&mut self, _name: &str, success: bool) {
        let indicator = if success { " ✓" } else { " ✗" };
        // Find the last line and append indicator
        if let Some(msg) = self.messages.last_mut() {
            if msg.content.ends_with('\n') {
                msg.content.pop();
            }
            msg.content.push_str(indicator);
            msg.content.push('\n');

            // Also update the last Text section if it exists
            for section in msg.sections.iter_mut().rev() {
                if let MessageSection::Text(ref mut text) = section {
                    if text.ends_with('\n') {
                        text.pop();
                    }
                    text.push_str(indicator);
                    text.push('\n');
                    break;
                }
            }
        }
    }

    /// Complete a tool call in a specific nested section
    pub fn complete_tool_call_in_section(&mut self, section_id: &str, _name: &str, success: bool) {
        let indicator = if success { " ✓" } else { " ✗" };
        if let Some(msg) = self.messages.last_mut() {
            // Update the nested section content
            if let Some(section) = msg.get_nested_section_mut(section_id) {
                if section.content.ends_with('\n') {
                    section.content.pop();
                }
                section.content.push_str(indicator);
                section.content.push('\n');
            }
            // Also update legacy content for consistency
            if msg.content.ends_with('\n') {
                msg.content.pop();
            }
            msg.content.push_str(indicator);
            msg.content.push('\n');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==========================================================================
    // AgentSection tests
    // ==========================================================================

    #[test]
    fn test_agent_section_new() {
        let section = AgentSection::new("test-agent", "Test Agent");
        assert_eq!(section.agent_name, "test-agent");
        assert_eq!(section.display_name, "Test Agent");
        assert_eq!(section.content, "");
        assert!(!section.is_collapsed);
        assert!(!section.is_complete);
        assert!(!section.id.is_empty(), "ID should be generated");
    }

    #[test]
    fn test_agent_section_append() {
        let mut section = AgentSection::new("agent", "Agent");
        section.append("Hello ");
        section.append("World");
        assert_eq!(section.content, "Hello World");
    }

    #[test]
    fn test_agent_section_finish() {
        let mut section = AgentSection::new("agent", "Agent");
        assert!(!section.is_complete);
        section.finish();
        assert!(section.is_complete);
    }

    #[test]
    fn test_agent_section_toggle_collapsed() {
        let mut section = AgentSection::new("agent", "Agent");
        assert!(!section.is_collapsed, "Should start uncollapsed");

        section.toggle_collapsed();
        assert!(
            section.is_collapsed,
            "Should be collapsed after first toggle"
        );

        section.toggle_collapsed();
        assert!(
            !section.is_collapsed,
            "Should be uncollapsed after second toggle"
        );
    }

    // ==========================================================================
    // MessageSection tests
    // ==========================================================================

    #[test]
    fn test_message_section_is_text() {
        let text_section = MessageSection::Text("hello".to_string());
        let agent_section = MessageSection::NestedAgent(AgentSection::new("a", "A"));

        assert!(text_section.is_text());
        assert!(!text_section.is_nested_agent());
        assert!(!agent_section.is_text());
        assert!(agent_section.is_nested_agent());
    }

    #[test]
    fn test_message_section_agent_section_id() {
        let text_section = MessageSection::Text("hello".to_string());
        let agent = AgentSection::new("a", "A");
        let expected_id = agent.id.clone();
        let agent_section = MessageSection::NestedAgent(agent);

        assert!(text_section.agent_section_id().is_none());
        assert_eq!(agent_section.agent_section_id(), Some(expected_id.as_str()));
    }

    // ==========================================================================
    // ChatMessage nested section tests
    // ==========================================================================

    #[test]
    fn test_chat_message_nested_section_lifecycle() {
        let mut msg = ChatMessage::assistant();

        // Start nested section
        let section_id = msg.start_nested_section("sub-agent", "Sub Agent");
        assert!(!section_id.is_empty());
        assert!(msg.has_nested_sections());

        // Verify the section exists and is initially empty/incomplete
        let section = msg
            .get_nested_section(&section_id)
            .expect("Section should exist");
        assert_eq!(section.agent_name, "sub-agent");
        assert_eq!(section.display_name, "Sub Agent");
        assert_eq!(section.content, "");
        assert!(!section.is_complete);

        // Append content
        msg.append_to_nested_section(&section_id, "Line 1\n");
        msg.append_to_nested_section(&section_id, "Line 2");

        // Verify content before finishing
        let section = msg
            .get_nested_section(&section_id)
            .expect("Section should exist");
        assert_eq!(section.content, "Line 1\nLine 2");
        assert!(!section.is_complete);

        // Finish section
        msg.finish_nested_section(&section_id);

        // Verify is_complete is true
        let section = msg
            .get_nested_section(&section_id)
            .expect("Section should exist");
        assert!(section.is_complete);
    }

    #[test]
    fn test_chat_message_active_nested_section_tracking() {
        let mut msg = ChatMessage::assistant();

        // Initially no active nested section
        assert!(msg.active_nested_section_id().is_none());

        // Start a nested section - it should be active
        let section_id_1 = msg.start_nested_section("agent-1", "Agent 1");
        assert_eq!(msg.active_nested_section_id(), Some(section_id_1.as_str()));

        // Finish the section - no longer active
        msg.finish_nested_section(&section_id_1);
        assert!(msg.active_nested_section_id().is_none());

        // Start another section
        let section_id_2 = msg.start_nested_section("agent-2", "Agent 2");
        assert_eq!(msg.active_nested_section_id(), Some(section_id_2.as_str()));
    }

    #[test]
    fn test_chat_message_multiple_nested_sections() {
        let mut msg = ChatMessage::assistant();

        // Add text, then nested section, then more text
        msg.append_to_section("Before\n");

        let section_id = msg.start_nested_section("agent", "Agent");
        msg.append_to_nested_section(&section_id, "Nested content");
        msg.finish_nested_section(&section_id);

        msg.append_to_section("After\n");

        // Should have 3 sections: Text, NestedAgent, Text
        assert_eq!(msg.sections.len(), 3);
        assert!(msg.sections[0].is_text());
        assert!(msg.sections[1].is_nested_agent());
        assert!(msg.sections[2].is_text());
    }

    #[test]
    fn test_toggle_section_collapsed() {
        let mut msg = ChatMessage::assistant();
        let section_id = msg.start_nested_section("agent", "Agent");

        // Initially not collapsed
        let section = msg.get_nested_section(&section_id).unwrap();
        assert!(!section.is_collapsed);

        // Toggle to collapsed
        msg.toggle_section_collapsed(&section_id);
        let section = msg.get_nested_section(&section_id).unwrap();
        assert!(section.is_collapsed);

        // Toggle back to uncollapsed
        msg.toggle_section_collapsed(&section_id);
        let section = msg.get_nested_section(&section_id).unwrap();
        assert!(!section.is_collapsed);
    }

    #[test]
    fn test_get_nested_section_mut() {
        let mut msg = ChatMessage::assistant();
        let section_id = msg.start_nested_section("agent", "Agent");

        // Modify via mutable reference
        if let Some(section) = msg.get_nested_section_mut(&section_id) {
            section.content = "Modified directly".to_string();
            section.is_collapsed = true;
        }

        // Verify changes
        let section = msg.get_nested_section(&section_id).unwrap();
        assert_eq!(section.content, "Modified directly");
        assert!(section.is_collapsed);
    }

    // ==========================================================================
    // Conversation nested agent tests
    // ==========================================================================

    #[test]
    fn test_conversation_nested_agent() {
        let mut conv = Conversation::new();
        conv.start_assistant_message();

        // Start nested agent
        let section_id = conv.start_nested_agent("helper", "Helper Agent").unwrap();

        // Append to nested agent
        conv.append_to_nested_agent(&section_id, "Nested content");

        // Verify content is in the message
        let msg = conv.messages.last().unwrap();
        let section = msg.get_nested_section(&section_id).unwrap();
        assert_eq!(section.content, "Nested content");
        assert!(!section.is_complete);

        // Finish nested agent
        conv.finish_nested_agent(&section_id);

        let msg = conv.messages.last().unwrap();
        let section = msg.get_nested_section(&section_id).unwrap();
        assert!(section.is_complete);
    }

    #[test]
    fn test_conversation_start_nested_agent_no_message() {
        let mut conv = Conversation::new();

        // No messages exist, should return None
        let result = conv.start_nested_agent("agent", "Agent");
        assert!(result.is_none());
    }

    #[test]
    fn test_conversation_append_to_current_with_active_nested() {
        let mut conv = Conversation::new();
        conv.start_assistant_message();

        // Add some main content first
        conv.append_to_main_content("Main text\n");

        // Start nested section
        let section_id = conv.start_nested_agent("agent", "Agent").unwrap();

        // append_to_current should route to the active nested section
        conv.append_to_current("Goes to nested");

        let msg = conv.messages.last().unwrap();
        let section = msg.get_nested_section(&section_id).unwrap();
        assert_eq!(section.content, "Goes to nested");
    }

    #[test]
    fn test_conversation_append_to_current_without_nested() {
        let mut conv = Conversation::new();
        conv.start_assistant_message();

        // No nested section, should go to main content
        conv.append_to_current("Main content");

        let msg = conv.messages.last().unwrap();
        assert_eq!(msg.content, "Main content");
    }

    #[test]
    fn test_conversation_toggle_section_collapsed() {
        let mut conv = Conversation::new();
        conv.start_assistant_message();

        let section_id = conv.start_nested_agent("agent", "Agent").unwrap();

        // Toggle via conversation
        conv.toggle_section_collapsed(&section_id);

        let msg = conv.messages.last().unwrap();
        let section = msg.get_nested_section(&section_id).unwrap();
        assert!(section.is_collapsed);
    }

    #[test]
    fn test_conversation_active_nested_section_id() {
        let mut conv = Conversation::new();

        // No messages, should be None
        assert!(conv.active_nested_section_id().is_none());

        conv.start_assistant_message();

        // No nested sections, should be None
        assert!(conv.active_nested_section_id().is_none());

        // Start nested section
        let section_id = conv.start_nested_agent("agent", "Agent").unwrap();
        assert_eq!(conv.active_nested_section_id(), Some(section_id.as_str()));

        // Finish it
        conv.finish_nested_agent(&section_id);
        assert!(conv.active_nested_section_id().is_none());
    }

    // ==========================================================================
    // Edge case tests
    // ==========================================================================

    #[test]
    fn test_append_to_nonexistent_section() {
        let mut msg = ChatMessage::assistant();
        // Should not panic, just silently do nothing useful
        msg.append_to_nested_section("nonexistent-id", "text");
        // The text goes to legacy content but not to any nested section
        assert_eq!(msg.content, "text");
    }

    #[test]
    fn test_finish_nonexistent_section() {
        let mut msg = ChatMessage::assistant();
        // Should not panic
        msg.finish_nested_section("nonexistent-id");
    }

    #[test]
    fn test_toggle_nonexistent_section() {
        let mut msg = ChatMessage::assistant();
        // Should not panic
        msg.toggle_section_collapsed("nonexistent-id");
    }

    #[test]
    fn test_finish_already_finished_section() {
        let mut msg = ChatMessage::assistant();
        let section_id = msg.start_nested_section("agent", "Agent");

        // Finish once
        msg.finish_nested_section(&section_id);
        let section = msg.get_nested_section(&section_id).unwrap();
        assert!(section.is_complete);

        // Finish again - should not panic, just stay complete
        msg.finish_nested_section(&section_id);
        let section = msg.get_nested_section(&section_id).unwrap();
        assert!(section.is_complete);
    }

    #[test]
    fn test_get_nonexistent_section() {
        let msg = ChatMessage::assistant();
        assert!(msg.get_nested_section("nonexistent").is_none());
    }

    #[test]
    fn test_has_nested_sections_empty() {
        let msg = ChatMessage::assistant();
        assert!(!msg.has_nested_sections());
    }

    #[test]
    fn test_has_nested_sections_with_only_text() {
        let mut msg = ChatMessage::assistant();
        msg.append_to_section("Just text");
        assert!(!msg.has_nested_sections());
    }

    // ==========================================================================
    // Legacy content sync tests
    // ==========================================================================

    #[test]
    fn test_legacy_content_sync_with_nested_sections() {
        let mut msg = ChatMessage::assistant();

        // Add text to section
        msg.append_to_section("Main: ");

        // Start nested section and add content
        let section_id = msg.start_nested_section("agent", "Agent");
        msg.append_to_nested_section(&section_id, "Nested");

        // Legacy content should have both
        assert_eq!(msg.content, "Main: Nested");
    }

    // ==========================================================================
    // Format tool call display tests
    // ==========================================================================

    #[test]
    fn test_format_tool_call_display_list_files() {
        let args = serde_json::json!({"directory": "src", "recursive": true});
        let display = format_tool_call_display("list_files", &args);
        assert_eq!(display, "📂 `src` (recursive)");

        let args_non_recursive = serde_json::json!({"directory": ".", "recursive": false});
        let display = format_tool_call_display("list_files", &args_non_recursive);
        assert_eq!(display, "📂 `.`");
    }

    #[test]
    fn test_format_tool_call_display_read_file() {
        let args = serde_json::json!({"file_path": "src/main.rs"});
        let display = format_tool_call_display("read_file", &args);
        assert_eq!(display, "📄 `src/main.rs`");
    }

    #[test]
    fn test_format_tool_call_display_edit_file() {
        let args = serde_json::json!({"file_path": "test.py"});
        let display = format_tool_call_display("edit_file", &args);
        assert_eq!(display, "✏️ `test.py`");
    }

    #[test]
    fn test_format_tool_call_display_grep() {
        let args = serde_json::json!({"search_string": "TODO", "directory": "src"});
        let display = format_tool_call_display("grep", &args);
        assert_eq!(display, "🔍 `TODO` in `src`");
    }

    #[test]
    fn test_format_tool_call_display_shell_command() {
        let args = serde_json::json!({"command": "cargo build"});
        let display = format_tool_call_display("run_shell_command", &args);
        assert_eq!(display, "💻 `cargo build`");

        // Test truncation for long commands
        let long_cmd = "a".repeat(100);
        let args_long = serde_json::json!({"command": long_cmd});
        let display = format_tool_call_display("agent_run_shell_command", &args_long);
        assert!(display.ends_with("...`"));
        assert!(display.len() < 70); // Should be truncated
    }

    #[test]
    fn test_format_tool_call_display_invoke_agent() {
        let args = serde_json::json!({"agent_name": "code-reviewer"});
        let display = format_tool_call_display("invoke_agent", &args);
        assert_eq!(display, "🤖 → code-reviewer");
    }

    #[test]
    fn test_format_tool_call_display_unknown_tool() {
        let args = serde_json::json!({});
        let display = format_tool_call_display("custom_tool", &args);
        assert_eq!(display, "🔧 custom_tool");
    }

    // ==========================================================================
    // Section-specific tool call tests
    // ==========================================================================

    #[test]
    fn test_append_tool_call_to_section() {
        let mut conv = Conversation::new();
        conv.start_assistant_message();

        // Start a nested section
        let section_id = conv.start_nested_agent("sub-agent", "Sub Agent").unwrap();

        // Append tool call to that section
        let args = serde_json::json!({"file_path": "test.rs"});
        conv.append_tool_call_to_section(&section_id, "read_file", Some(args));

        // Verify it went to the nested section
        let msg = conv.messages.last().unwrap();
        let section = msg.get_nested_section(&section_id).unwrap();
        assert!(
            section.content.contains("📄 `test.rs`"),
            "Tool call should appear in nested section"
        );
    }

    #[test]
    fn test_complete_tool_call_in_section() {
        let mut conv = Conversation::new();
        conv.start_assistant_message();

        // Start a nested section and add a tool call
        let section_id = conv.start_nested_agent("sub-agent", "Sub Agent").unwrap();
        conv.append_tool_call_to_section(&section_id, "read_file", None);

        // Complete the tool call with success
        conv.complete_tool_call_in_section(&section_id, "read_file", true);

        // Verify the checkmark was added
        let msg = conv.messages.last().unwrap();
        let section = msg.get_nested_section(&section_id).unwrap();
        assert!(
            section.content.contains("✓"),
            "Success indicator should appear in nested section"
        );
    }

    #[test]
    fn test_complete_tool_call_in_section_failure() {
        let mut conv = Conversation::new();
        conv.start_assistant_message();

        // Start a nested section and add a tool call
        let section_id = conv.start_nested_agent("sub-agent", "Sub Agent").unwrap();
        conv.append_tool_call_to_section(&section_id, "read_file", None);

        // Complete the tool call with failure
        conv.complete_tool_call_in_section(&section_id, "read_file", false);

        // Verify the X mark was added
        let msg = conv.messages.last().unwrap();
        let section = msg.get_nested_section(&section_id).unwrap();
        assert!(
            section.content.contains("✗"),
            "Failure indicator should appear in nested section"
        );
    }
}
