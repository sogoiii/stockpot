//! Message section types for structured assistant messages
//!
//! Provides section abstractions for collapsible nested agent output.

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

/// A collapsible section containing model thinking/reasoning content
#[derive(Debug, Clone)]
pub struct ThinkingSection {
    /// Unique ID for this section
    pub id: String,
    /// Full thinking content (accumulated)
    pub content: String,
    /// Whether thinking is finished
    pub is_complete: bool,
}

impl ThinkingSection {
    /// Creates a new empty ThinkingSection with a UUID
    pub fn new() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            content: String::new(),
            is_complete: false,
        }
    }

    /// Appends text to the thinking content
    pub fn append(&mut self, text: &str) {
        self.content.push_str(text);
    }

    /// Marks thinking as complete
    pub fn finish(&mut self) {
        self.is_complete = true;
    }

    /// Returns first line or first 50 chars (whichever is shorter), with "..." if truncated
    pub fn preview(&self) -> String {
        if self.content.is_empty() {
            return String::new();
        }

        // Get first line
        let first_line = self.content.lines().next().unwrap_or("");

        // Take at most 50 chars
        let truncated: String = first_line.chars().take(50).collect();

        // Add "..." if we truncated (either by line break or char limit)
        let needs_ellipsis = truncated.len() < first_line.len() || self.content.contains('\n');

        if needs_ellipsis {
            format!("{}...", truncated)
        } else {
            truncated
        }
    }
}

impl Default for ThinkingSection {
    fn default() -> Self {
        Self::new()
    }
}

/// A section within an assistant message
#[derive(Debug, Clone)]
pub enum MessageSection {
    /// Plain text/markdown content
    Text(String),
    /// Nested agent output (collapsible)
    NestedAgent(AgentSection),
    /// Model thinking/reasoning (collapsible)
    Thinking(ThinkingSection),
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

    /// Returns true if this is a Thinking section
    pub fn is_thinking(&self) -> bool {
        matches!(self, MessageSection::Thinking(_))
    }

    /// Get the section ID if it's a thinking section
    pub fn thinking_section_id(&self) -> Option<&str> {
        match self {
            MessageSection::Thinking(section) => Some(&section.id),
            _ => None,
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
    // ThinkingSection tests
    // ==========================================================================

    #[test]
    fn test_thinking_section_new() {
        let section = ThinkingSection::new();
        assert!(!section.id.is_empty(), "ID should be generated");
        assert_eq!(section.content, "");
        assert!(!section.is_complete);
    }

    #[test]
    fn test_thinking_section_append() {
        let mut section = ThinkingSection::new();
        section.append("Thinking about ");
        section.append("the problem");
        assert_eq!(section.content, "Thinking about the problem");
    }

    #[test]
    fn test_thinking_section_finish() {
        let mut section = ThinkingSection::new();
        assert!(!section.is_complete);
        section.finish();
        assert!(section.is_complete);
    }

    #[test]
    fn test_thinking_section_preview_empty() {
        let section = ThinkingSection::new();
        assert_eq!(section.preview(), "");
    }

    #[test]
    fn test_thinking_section_preview_short_single_line() {
        let mut section = ThinkingSection::new();
        section.append("Short thought");
        assert_eq!(section.preview(), "Short thought");
    }

    #[test]
    fn test_thinking_section_preview_multiline() {
        let mut section = ThinkingSection::new();
        section.append("First line\nSecond line\nThird line");
        assert_eq!(section.preview(), "First line...");
    }

    #[test]
    fn test_thinking_section_preview_long_first_line() {
        let mut section = ThinkingSection::new();
        section
            .append("This is a very long line that definitely exceeds fifty characters in length");
        let preview = section.preview();
        assert!(preview.ends_with("..."));
        // 50 chars + "..."
        assert_eq!(preview.len(), 53);
    }

    #[test]
    fn test_thinking_section_preview_exactly_50_chars_no_newline() {
        let mut section = ThinkingSection::new();
        // Exactly 50 chars, no newline - should NOT have ellipsis
        section.append("12345678901234567890123456789012345678901234567890");
        assert_eq!(
            section.preview(),
            "12345678901234567890123456789012345678901234567890"
        );
    }

    #[test]
    fn test_message_section_is_thinking() {
        let text_section = MessageSection::Text("hello".to_string());
        let agent_section = MessageSection::NestedAgent(AgentSection::new("a", "A"));
        let thinking_section = MessageSection::Thinking(ThinkingSection::new());

        assert!(!text_section.is_thinking());
        assert!(!agent_section.is_thinking());
        assert!(thinking_section.is_thinking());
    }

    #[test]
    fn test_message_section_thinking_section_id() {
        let text_section = MessageSection::Text("hello".to_string());
        let thinking = ThinkingSection::new();
        let expected_id = thinking.id.clone();
        let thinking_section = MessageSection::Thinking(thinking);

        assert!(text_section.thinking_section_id().is_none());
        assert_eq!(
            thinking_section.thinking_section_id(),
            Some(expected_id.as_str())
        );
    }
}
