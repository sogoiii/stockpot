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
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub is_streaming: bool,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            role: MessageRole::User,
            content: content.into(),
            tool_calls: vec![],
            is_streaming: false,
        }
    }

    pub fn assistant() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            role: MessageRole::Assistant,
            content: String::new(),
            tool_calls: vec![],
            is_streaming: true,
        }
    }

    pub fn append_content(&mut self, text: &str) {
        self.content.push_str(text);
    }

    pub fn finish_streaming(&mut self) {
        self.is_streaming = false;
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

    pub fn append_to_current(&mut self, text: &str) {
        if let Some(msg) = self.messages.last_mut() {
            msg.append_content(text);
        }
    }

    pub fn finish_current_message(&mut self) {
        if let Some(msg) = self.messages.last_mut() {
            msg.finish_streaming();
        }
        self.is_generating = false;
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
        self.append_to_current(&marker);
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
        }
    }
}
