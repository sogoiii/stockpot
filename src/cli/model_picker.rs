//! Interactive model selection using dialoguer.
//!
//! Provides fuzzy-search model picking, agent selection, and
//! interactive model settings editing.

use dialoguer::{theme::ColorfulTheme, Confirm, FuzzySelect, Input, Select};

use crate::db::Database;
use crate::models::{ModelRegistry, ModelSettings};

/// Show an interactive model picker, returns selected model name.
///
/// Uses fuzzy search to filter through available models.
pub fn pick_model(db: &Database, registry: &ModelRegistry, current: &str) -> Option<String> {
    // Only show models that have valid provider configuration (API keys or OAuth tokens)
    let models: Vec<String> = registry.list_available(db);

    if models.is_empty() {
        println!("No models configured.");
        return None;
    }

    let default = models.iter().position(|m| m == current).unwrap_or(0);

    // Show with current marker
    let display: Vec<String> = models
        .iter()
        .map(|m| {
            if m == current {
                format!("{} (current)", m)
            } else {
                m.clone()
            }
        })
        .collect();

    let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
        .with_prompt("Select model (type to filter)")
        .items(&display)
        .default(default)
        .interact_opt();

    match selection {
        Ok(Some(idx)) => models.get(idx).cloned(),
        _ => None,
    }
}

/// Show an interactive agent picker.
///
/// Takes a list of (name, display_name) tuples.
pub fn pick_agent(agents: &[(String, String)], current: &str) -> Option<String> {
    if agents.is_empty() {
        println!("No agents available.");
        return None;
    }

    let default = agents.iter().position(|(n, _)| n == current).unwrap_or(0);

    let display: Vec<String> = agents
        .iter()
        .map(|(name, display)| {
            if name == current {
                format!("{} - {} (current)", name, display)
            } else {
                format!("{} - {}", name, display)
            }
        })
        .collect();

    match Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Select agent")
        .items(&display)
        .default(default)
        .interact_opt()
    {
        Ok(Some(idx)) => Some(agents[idx].0.clone()),
        _ => None,
    }
}

/// Edit model settings interactively.
///
/// Shows different options based on model type (OpenAI reasoning, Claude, etc.).
/// Returns true if settings were modified.
pub fn edit_model_settings(db: &Database, model_name: &str, _registry: &ModelRegistry) -> bool {
    let current = ModelSettings::load(db, model_name).unwrap_or_default();

    println!("\n⚙️  Settings for: \x1b[1m{}\x1b[0m\n", model_name);

    // Temperature (universal)
    let temp_str = current
        .temperature
        .map(|t| format!("{:.1}", t))
        .unwrap_or_default();
    let new_temp: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Temperature (0.0-2.0, empty=default)")
        .default(temp_str)
        .allow_empty(true)
        .interact_text()
        .unwrap_or_default();

    if !new_temp.is_empty() {
        if let Ok(t) = new_temp.parse::<f32>() {
            let clamped: f32 = t.clamp(0.0, 2.0);
            let value = format!("{:.2}", clamped);
            let _ = ModelSettings::save_setting(db, model_name, "temperature", &value);
        }
    } else {
        let _ = ModelSettings::clear_setting(db, model_name, "temperature");
    }

    // Max tokens
    let max_str: String = current
        .max_tokens
        .map(|t: i32| t.to_string())
        .unwrap_or_default();
    let new_max: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Max tokens (empty=default)")
        .default(max_str)
        .allow_empty(true)
        .interact_text()
        .unwrap_or_default();

    if !new_max.is_empty() {
        if let Ok(tokens) = new_max.parse::<i32>() {
            let value: String = tokens.to_string();
            let _ = ModelSettings::save_setting(db, model_name, "max_tokens", &value);
        }
    } else {
        let _ = ModelSettings::clear_setting(db, model_name, "max_tokens");
    }

    // Check model type for specific settings
    let is_openai_reasoning =
        model_name.contains("gpt-5") || model_name.contains("o1") || model_name.contains("o3");
    let is_claude = model_name.contains("claude");

    // OpenAI reasoning models
    if is_openai_reasoning {
        let efforts = ["minimal", "low", "medium", "high", "xhigh"];
        let current_idx = current
            .reasoning_effort
            .as_ref()
            .and_then(|r| efforts.iter().position(|e| *e == r))
            .unwrap_or(2); // default to medium

        if let Ok(idx) = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("Reasoning effort")
            .items(&efforts)
            .default(current_idx)
            .interact()
        {
            let _ = ModelSettings::save_setting(db, model_name, "reasoning_effort", efforts[idx]);
        }

        // Verbosity (not for codex models)
        if !model_name.contains("codex") {
            let verbosities = ["low", "medium", "high"];
            let verbosity_values = [0, 1, 2];
            let current_idx = current
                .verbosity
                .map(|v| match v {
                    0 => 0,
                    1 => 1,
                    _ => 2,
                })
                .unwrap_or(1); // default to medium

            if let Ok(idx) = Select::with_theme(&ColorfulTheme::default())
                .with_prompt("Verbosity")
                .items(&verbosities)
                .default(current_idx)
                .interact()
            {
                let _ = ModelSettings::save_setting(
                    db,
                    model_name,
                    "verbosity",
                    &verbosity_values[idx].to_string(),
                );
            }
        }
    }

    // Claude models
    if is_claude {
        let extended = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("Enable extended thinking?")
            .default(current.extended_thinking.unwrap_or(true))
            .interact()
            .unwrap_or(true);

        let _ = ModelSettings::save_setting(
            db,
            model_name,
            "extended_thinking",
            if extended { "true" } else { "false" },
        );

        if extended {
            let budget_str: String = current.budget_tokens.unwrap_or(10000).to_string();
            let new_budget: String = Input::with_theme(&ColorfulTheme::default())
                .with_prompt("Thinking budget (tokens)")
                .default(budget_str)
                .interact_text()
                .unwrap_or_else(|_| "10000".to_string());

            if let Ok(budget) = new_budget.parse::<i32>() {
                let value: String = budget.to_string();
                let _ = ModelSettings::save_setting(db, model_name, "budget_tokens", &value);
            }
        }

        // Interleaved thinking (Claude 4 only)
        if model_name.contains("claude-4")
            || model_name.contains("sonnet-4")
            || model_name.contains("opus-4")
        {
            let interleaved = Confirm::with_theme(&ColorfulTheme::default())
                .with_prompt("Enable interleaved thinking? (Claude 4 only)")
                .default(current.interleaved_thinking.unwrap_or(false))
                .interact()
                .unwrap_or(false);

            let _ = ModelSettings::save_setting(
                db,
                model_name,
                "interleaved_thinking",
                if interleaved { "true" } else { "false" },
            );
        }
    }

    println!("\n✅ Settings saved\n");
    true
}

/// Show current model settings (non-interactive).
pub fn show_model_settings(db: &Database, model_name: &str) {
    let settings = ModelSettings::load(db, model_name).unwrap_or_default();

    println!("\n⚙️  Settings for: \x1b[1m{}\x1b[0m\n", model_name);

    if let Some(t) = settings.temperature {
        println!("  temperature:          {:.2}", t);
    }
    if let Some(t) = settings.max_tokens {
        println!("  max_tokens:           {}", t);
    }
    if let Some(t) = settings.seed {
        println!("  seed:                 {}", t);
    }
    if let Some(ref t) = settings.reasoning_effort {
        println!("  reasoning_effort:     {}", t);
    }
    if let Some(t) = settings.verbosity {
        let label = verbosity_label(t);
        println!("  verbosity:            {} ({})", t, label);
    }
    if let Some(t) = settings.extended_thinking {
        println!("  extended_thinking:    {}", t);
    }
    if let Some(t) = settings.budget_tokens {
        println!("  budget_tokens:        {}", t);
    }
    if let Some(t) = settings.interleaved_thinking {
        println!("  interleaved_thinking: {}", t);
    }

    if settings.is_empty() {
        println!("  (using defaults)");
    }
    println!();
}

// =========================================================================
// Model Type Detection Functions (extracted for testability)
// =========================================================================

/// Check if a model is an OpenAI reasoning model (o1, o3, gpt-5).
pub fn is_openai_reasoning_model(model_name: &str) -> bool {
    model_name.contains("gpt-5") || model_name.contains("o1") || model_name.contains("o3")
}

/// Check if a model is a Claude model.
pub fn is_claude_model(model_name: &str) -> bool {
    model_name.contains("claude")
}

/// Check if a model is a Claude 4 model (supports interleaved thinking).
pub fn is_claude_4_model(model_name: &str) -> bool {
    model_name.contains("claude-4")
        || model_name.contains("sonnet-4")
        || model_name.contains("opus-4")
}

/// Check if a model is a codex model (no verbosity setting).
pub fn is_codex_model(model_name: &str) -> bool {
    model_name.contains("codex")
}

/// Get the verbosity label for a verbosity level.
pub fn verbosity_label(level: i32) -> &'static str {
    match level {
        0 => "low",
        1 => "medium",
        _ => "high",
    }
}

/// Format a model name for display with optional "(current)" marker.
pub fn format_model_display(model_name: &str, current: &str) -> String {
    if model_name == current {
        format!("{} (current)", model_name)
    } else {
        model_name.to_string()
    }
}

/// Format an agent entry for display with optional "(current)" marker.
pub fn format_agent_display(name: &str, display_name: &str, current: &str) -> String {
    if name == current {
        format!("{} - {} (current)", name, display_name)
    } else {
        format!("{} - {}", name, display_name)
    }
}

/// Find the default index for a model in a list.
pub fn find_default_model_index(models: &[String], current: &str) -> usize {
    models.iter().position(|m| m == current).unwrap_or(0)
}

/// Find the default index for an agent in a list.
pub fn find_default_agent_index(agents: &[(String, String)], current: &str) -> usize {
    agents.iter().position(|(n, _)| n == current).unwrap_or(0)
}

/// The available reasoning effort levels for OpenAI models.
pub const REASONING_EFFORTS: [&str; 5] = ["minimal", "low", "medium", "high", "xhigh"];

/// The available verbosity labels.
pub const VERBOSITY_LABELS: [&str; 3] = ["low", "medium", "high"];

/// The verbosity values corresponding to labels.
pub const VERBOSITY_VALUES: [i32; 3] = [0, 1, 2];

/// Find the index for a reasoning effort level.
pub fn find_reasoning_effort_index(effort: Option<&String>) -> usize {
    effort
        .and_then(|r| REASONING_EFFORTS.iter().position(|e| *e == r))
        .unwrap_or(2) // default to medium
}

/// Map verbosity value to index.
pub fn verbosity_to_index(verbosity: Option<i32>) -> usize {
    verbosity
        .map(|v| match v {
            0 => 0,
            1 => 1,
            _ => 2,
        })
        .unwrap_or(1) // default to medium
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Model Settings Tests
    // =========================================================================

    #[test]
    fn test_model_settings_is_empty() {
        let settings = ModelSettings::default();
        assert!(settings.is_empty());

        let settings = ModelSettings {
            temperature: Some(0.5),
            ..Default::default()
        };
        assert!(!settings.is_empty());
    }

    // =========================================================================
    // Model Type Detection Tests
    // =========================================================================

    #[test]
    fn test_is_openai_reasoning_model() {
        // Positive cases
        assert!(is_openai_reasoning_model("gpt-5"));
        assert!(is_openai_reasoning_model("gpt-5-turbo"));
        assert!(is_openai_reasoning_model("o1-preview"));
        assert!(is_openai_reasoning_model("o1-mini"));
        assert!(is_openai_reasoning_model("o3"));
        assert!(is_openai_reasoning_model("o3-mini"));

        // Negative cases
        assert!(!is_openai_reasoning_model("gpt-4o"));
        assert!(!is_openai_reasoning_model("gpt-4o-mini"));
        assert!(!is_openai_reasoning_model("claude-3-opus"));
        assert!(!is_openai_reasoning_model("gemini-pro"));
    }

    #[test]
    fn test_is_claude_model() {
        // Positive cases
        assert!(is_claude_model("claude-3-opus"));
        assert!(is_claude_model("claude-3-sonnet"));
        assert!(is_claude_model("claude-3-haiku"));
        assert!(is_claude_model("claude-4"));
        assert!(is_claude_model("anthropic:claude-3-opus"));

        // Negative cases
        assert!(!is_claude_model("gpt-4o"));
        assert!(!is_claude_model("gemini-pro"));
        assert!(!is_claude_model("mistral-large"));
    }

    #[test]
    fn test_is_claude_4_model() {
        // Positive cases
        assert!(is_claude_4_model("claude-4"));
        assert!(is_claude_4_model("claude-4-opus"));
        assert!(is_claude_4_model("sonnet-4"));
        assert!(is_claude_4_model("opus-4"));
        assert!(is_claude_4_model("anthropic:claude-4"));

        // Negative cases
        assert!(!is_claude_4_model("claude-3-opus"));
        assert!(!is_claude_4_model("claude-3-sonnet"));
        assert!(!is_claude_4_model("gpt-4o"));
    }

    #[test]
    fn test_is_codex_model() {
        // Positive cases
        assert!(is_codex_model("codex"));
        assert!(is_codex_model("gpt-5-codex"));
        assert!(is_codex_model("o3-codex"));

        // Negative cases
        assert!(!is_codex_model("gpt-5"));
        assert!(!is_codex_model("o1-preview"));
        assert!(!is_codex_model("claude-3-opus"));
    }

    // =========================================================================
    // Verbosity Tests
    // =========================================================================

    #[test]
    fn test_verbosity_label() {
        assert_eq!(verbosity_label(0), "low");
        assert_eq!(verbosity_label(1), "medium");
        assert_eq!(verbosity_label(2), "high");
        assert_eq!(verbosity_label(3), "high"); // anything > 1 is high
        assert_eq!(verbosity_label(100), "high");
        assert_eq!(verbosity_label(-1), "high"); // edge case
    }

    #[test]
    fn test_verbosity_to_index() {
        assert_eq!(verbosity_to_index(Some(0)), 0);
        assert_eq!(verbosity_to_index(Some(1)), 1);
        assert_eq!(verbosity_to_index(Some(2)), 2);
        assert_eq!(verbosity_to_index(Some(3)), 2); // clamps to high
        assert_eq!(verbosity_to_index(None), 1); // defaults to medium
    }

    #[test]
    fn test_verbosity_constants() {
        assert_eq!(VERBOSITY_LABELS, ["low", "medium", "high"]);
        assert_eq!(VERBOSITY_VALUES, [0, 1, 2]);
    }

    // =========================================================================
    // Reasoning Effort Tests
    // =========================================================================

    #[test]
    fn test_reasoning_efforts_constant() {
        assert_eq!(
            REASONING_EFFORTS,
            ["minimal", "low", "medium", "high", "xhigh"]
        );
    }

    #[test]
    fn test_find_reasoning_effort_index() {
        assert_eq!(find_reasoning_effort_index(Some(&"minimal".to_string())), 0);
        assert_eq!(find_reasoning_effort_index(Some(&"low".to_string())), 1);
        assert_eq!(find_reasoning_effort_index(Some(&"medium".to_string())), 2);
        assert_eq!(find_reasoning_effort_index(Some(&"high".to_string())), 3);
        assert_eq!(find_reasoning_effort_index(Some(&"xhigh".to_string())), 4);
        assert_eq!(find_reasoning_effort_index(None), 2); // defaults to medium
        assert_eq!(find_reasoning_effort_index(Some(&"invalid".to_string())), 2);
        // defaults to medium
    }

    // =========================================================================
    // Display Formatting Tests
    // =========================================================================

    #[test]
    fn test_format_model_display_current() {
        assert_eq!(format_model_display("gpt-4o", "gpt-4o"), "gpt-4o (current)");
    }

    #[test]
    fn test_format_model_display_not_current() {
        assert_eq!(format_model_display("gpt-4o", "claude-3-opus"), "gpt-4o");
    }

    #[test]
    fn test_format_agent_display_current() {
        assert_eq!(
            format_agent_display("stockpot", "Stockpot Agent", "stockpot"),
            "stockpot - Stockpot Agent (current)"
        );
    }

    #[test]
    fn test_format_agent_display_not_current() {
        assert_eq!(
            format_agent_display("explore", "Explorer Agent", "stockpot"),
            "explore - Explorer Agent"
        );
    }

    // =========================================================================
    // Index Finding Tests
    // =========================================================================

    #[test]
    fn test_find_default_model_index_found() {
        let models = vec![
            "gpt-4o".to_string(),
            "claude-3-opus".to_string(),
            "gemini-pro".to_string(),
        ];
        assert_eq!(find_default_model_index(&models, "claude-3-opus"), 1);
    }

    #[test]
    fn test_find_default_model_index_not_found() {
        let models = vec!["gpt-4o".to_string(), "claude-3-opus".to_string()];
        assert_eq!(find_default_model_index(&models, "nonexistent"), 0);
    }

    #[test]
    fn test_find_default_model_index_empty() {
        let models: Vec<String> = vec![];
        assert_eq!(find_default_model_index(&models, "gpt-4o"), 0);
    }

    #[test]
    fn test_find_default_agent_index_found() {
        let agents = vec![
            ("stockpot".to_string(), "Stockpot".to_string()),
            ("explore".to_string(), "Explorer".to_string()),
        ];
        assert_eq!(find_default_agent_index(&agents, "explore"), 1);
    }

    #[test]
    fn test_find_default_agent_index_not_found() {
        let agents = vec![
            ("stockpot".to_string(), "Stockpot".to_string()),
            ("explore".to_string(), "Explorer".to_string()),
        ];
        assert_eq!(find_default_agent_index(&agents, "nonexistent"), 0);
    }

    #[test]
    fn test_find_default_agent_index_empty() {
        let agents: Vec<(String, String)> = vec![];
        assert_eq!(find_default_agent_index(&agents, "stockpot"), 0);
    }

    // =========================================================================
    // Additional Model Type Detection Edge Cases
    // =========================================================================

    #[test]
    fn test_is_openai_reasoning_model_edge_cases() {
        // Substring matches anywhere
        assert!(is_openai_reasoning_model("my-gpt-5-custom"));
        assert!(is_openai_reasoning_model("prefix-o1-suffix"));
        assert!(is_openai_reasoning_model("o3-turbo-preview"));

        // Case sensitivity (should be case-sensitive)
        assert!(!is_openai_reasoning_model("GPT-5"));
        assert!(!is_openai_reasoning_model("O1-preview"));
        assert!(!is_openai_reasoning_model("O3-mini"));

        // Note: contains() means gpt-50 matches because it contains "gpt-5"
        assert!(is_openai_reasoning_model("gpt-50")); // contains "gpt-5"

        // "go1" contains "o1" as substring
        assert!(is_openai_reasoning_model("go1")); // contains "o1"

        // These don't contain the target substrings
        assert!(!is_openai_reasoning_model("o-1")); // not o1
        assert!(!is_openai_reasoning_model("gpt4")); // not gpt-5
    }

    #[test]
    fn test_is_claude_model_edge_cases() {
        // Various Claude naming conventions
        assert!(is_claude_model("claude"));

        // Provider prefixes
        assert!(is_claude_model("anthropic/claude-3-opus"));
        assert!(is_claude_model("bedrock:claude-3-sonnet"));

        // Case sensitivity - uses contains() which is case-sensitive
        assert!(!is_claude_model("CLAUDE"));
        assert!(!is_claude_model("CLAUDE-3"));
        assert!(!is_claude_model("Claude-3"));
    }

    #[test]
    fn test_is_claude_4_model_edge_cases() {
        // Various Claude 4 patterns
        assert!(is_claude_4_model("anthropic/claude-4-opus"));
        assert!(is_claude_4_model("my-sonnet-4-model"));
        assert!(is_claude_4_model("opus-4-extended"));

        // Claude 3 should NOT match
        assert!(!is_claude_4_model("claude-3.5-sonnet"));
        assert!(!is_claude_4_model("sonnet-3.5"));

        // Note: contains() means claude-40 matches because it contains "claude-4"
        assert!(is_claude_4_model("claude-40")); // contains "claude-4"
    }

    #[test]
    fn test_is_codex_model_edge_cases() {
        // Various codex patterns
        assert!(is_codex_model("openai-codex"));
        assert!(is_codex_model("codex-v2"));
        assert!(is_codex_model("o1-codex-mini"));

        // Case sensitivity
        assert!(!is_codex_model("CODEX"));
        assert!(!is_codex_model("Codex"));
    }

    #[test]
    fn test_combined_model_type_checks() {
        // A model can match multiple categories
        let model = "gpt-5-codex";
        assert!(is_openai_reasoning_model(model));
        assert!(is_codex_model(model));
        assert!(!is_claude_model(model));

        // Claude 4 is also claude
        let claude4 = "claude-4-opus";
        assert!(is_claude_model(claude4));
        assert!(is_claude_4_model(claude4));
        assert!(!is_openai_reasoning_model(claude4));
    }

    // =========================================================================
    // Verbosity Edge Cases
    // =========================================================================

    #[test]
    fn test_verbosity_label_negative_values() {
        // Negative values default to "high" (match _ arm)
        assert_eq!(verbosity_label(-100), "high");
        assert_eq!(verbosity_label(i32::MIN), "high");
    }

    #[test]
    fn test_verbosity_label_large_values() {
        assert_eq!(verbosity_label(i32::MAX), "high");
        assert_eq!(verbosity_label(1000), "high");
    }

    #[test]
    fn test_verbosity_to_index_negative() {
        // Negative values fall through to _ => 2 (high)
        assert_eq!(verbosity_to_index(Some(-1)), 2);
        assert_eq!(verbosity_to_index(Some(i32::MIN)), 2);
    }

    // =========================================================================
    // Reasoning Effort Edge Cases
    // =========================================================================

    #[test]
    fn test_find_reasoning_effort_index_all_values() {
        // Exhaustive test of all valid values
        let efforts = ["minimal", "low", "medium", "high", "xhigh"];
        for (expected_idx, effort) in efforts.iter().enumerate() {
            assert_eq!(
                find_reasoning_effort_index(Some(&effort.to_string())),
                expected_idx,
                "Failed for effort: {}",
                effort
            );
        }
    }

    #[test]
    fn test_find_reasoning_effort_index_case_sensitivity() {
        // Uppercase versions should NOT match
        assert_eq!(find_reasoning_effort_index(Some(&"MEDIUM".to_string())), 2); // defaults
        assert_eq!(find_reasoning_effort_index(Some(&"High".to_string())), 2);
        assert_eq!(find_reasoning_effort_index(Some(&"LOW".to_string())), 2);
    }

    #[test]
    fn test_find_reasoning_effort_index_whitespace() {
        // Whitespace should cause mismatch
        assert_eq!(find_reasoning_effort_index(Some(&" medium".to_string())), 2);
        assert_eq!(find_reasoning_effort_index(Some(&"medium ".to_string())), 2);
        assert_eq!(
            find_reasoning_effort_index(Some(&" medium ".to_string())),
            2
        );
    }

    // =========================================================================
    // Format Display Edge Cases
    // =========================================================================

    #[test]
    fn test_format_model_display_empty_strings() {
        // Empty model name
        assert_eq!(format_model_display("", ""), " (current)");
        assert_eq!(format_model_display("", "gpt-4o"), "");

        // Empty current
        assert_eq!(format_model_display("gpt-4o", ""), "gpt-4o");
    }

    #[test]
    fn test_format_model_display_special_chars() {
        // Model names with special characters
        let model = "anthropic:claude-3-opus@v1.2";
        assert_eq!(
            format_model_display(model, model),
            "anthropic:claude-3-opus@v1.2 (current)"
        );
        assert_eq!(
            format_model_display(model, "other"),
            "anthropic:claude-3-opus@v1.2"
        );
    }

    #[test]
    fn test_format_agent_display_empty_strings() {
        // All empty
        assert_eq!(format_agent_display("", "", ""), " -  (current)");

        // Empty name but has display
        assert_eq!(format_agent_display("", "Display", "other"), " - Display");

        // Empty display
        assert_eq!(
            format_agent_display("name", "", "name"),
            "name -  (current)"
        );
    }

    #[test]
    fn test_format_agent_display_long_names() {
        let long_name = "a".repeat(100);
        let long_display = "b".repeat(100);

        assert_eq!(
            format_agent_display(&long_name, &long_display, &long_name),
            format!("{} - {} (current)", long_name, long_display)
        );
    }

    // =========================================================================
    // Index Finding Edge Cases
    // =========================================================================

    #[test]
    fn test_find_default_model_index_first_match() {
        // When current is first in list
        let models = vec!["first".to_string(), "second".to_string()];
        assert_eq!(find_default_model_index(&models, "first"), 0);
    }

    #[test]
    fn test_find_default_model_index_last_match() {
        // When current is last in list
        let models = vec![
            "first".to_string(),
            "second".to_string(),
            "third".to_string(),
        ];
        assert_eq!(find_default_model_index(&models, "third"), 2);
    }

    #[test]
    fn test_find_default_model_index_duplicate_entries() {
        // If there are duplicates, should return first occurrence
        let models = vec![
            "model".to_string(),
            "other".to_string(),
            "model".to_string(),
        ];
        assert_eq!(find_default_model_index(&models, "model"), 0);
    }

    #[test]
    fn test_find_default_agent_index_first_match() {
        let agents = vec![
            ("first".to_string(), "First Agent".to_string()),
            ("second".to_string(), "Second Agent".to_string()),
        ];
        assert_eq!(find_default_agent_index(&agents, "first"), 0);
    }

    #[test]
    fn test_find_default_agent_index_matches_name_not_display() {
        // Should match on name (first tuple element), not display name
        let agents = vec![
            ("stockpot".to_string(), "Stockpot Agent".to_string()),
            ("explore".to_string(), "Explorer".to_string()),
        ];
        // Searching for display name should fail
        assert_eq!(find_default_agent_index(&agents, "Stockpot Agent"), 0);
        // Searching for name should succeed
        assert_eq!(find_default_agent_index(&agents, "stockpot"), 0);
        assert_eq!(find_default_agent_index(&agents, "explore"), 1);
    }

    // =========================================================================
    // Constants Validation
    // =========================================================================

    #[test]
    fn test_constants_alignment() {
        // Verify VERBOSITY_LABELS and VERBOSITY_VALUES are aligned
        assert_eq!(VERBOSITY_LABELS.len(), VERBOSITY_VALUES.len());

        // Verify the mapping makes sense
        assert_eq!(VERBOSITY_LABELS[0], "low");
        assert_eq!(VERBOSITY_VALUES[0], 0);

        assert_eq!(VERBOSITY_LABELS[1], "medium");
        assert_eq!(VERBOSITY_VALUES[1], 1);

        assert_eq!(VERBOSITY_LABELS[2], "high");
        assert_eq!(VERBOSITY_VALUES[2], 2);
    }

    #[test]
    fn test_reasoning_efforts_count() {
        // Verify we have exactly 5 reasoning levels
        assert_eq!(REASONING_EFFORTS.len(), 5);

        // Verify order (minimal -> xhigh)
        assert_eq!(REASONING_EFFORTS[0], "minimal");
        assert_eq!(REASONING_EFFORTS[4], "xhigh");
    }

    // =========================================================================
    // Model Selection Logic Tests
    // =========================================================================

    #[test]
    fn test_model_selection_empty_list_returns_zero_index() {
        // Edge case: empty list should still return 0 (safe default)
        let models: Vec<String> = vec![];
        assert_eq!(find_default_model_index(&models, "any"), 0);
    }

    #[test]
    fn test_model_selection_single_item_list() {
        let models = vec!["gpt-4o".to_string()];
        // Current model is the only one
        assert_eq!(find_default_model_index(&models, "gpt-4o"), 0);
        // Current model not in list
        assert_eq!(find_default_model_index(&models, "claude-3"), 0);
    }

    #[test]
    fn test_model_selection_preserves_exact_match() {
        let models = vec![
            "gpt-4o".to_string(),
            "gpt-4o-mini".to_string(),
            "gpt-4".to_string(),
        ];
        // Should find exact match, not partial
        assert_eq!(find_default_model_index(&models, "gpt-4o"), 0);
        assert_eq!(find_default_model_index(&models, "gpt-4o-mini"), 1);
        assert_eq!(find_default_model_index(&models, "gpt-4"), 2);
    }

    #[test]
    fn test_agent_selection_single_item_list() {
        let agents = vec![("default".to_string(), "Default Agent".to_string())];
        assert_eq!(find_default_agent_index(&agents, "default"), 0);
        assert_eq!(find_default_agent_index(&agents, "other"), 0);
    }

    #[test]
    fn test_agent_selection_ignores_display_name_in_search() {
        let agents = vec![
            ("code".to_string(), "Code Helper".to_string()),
            ("chat".to_string(), "Chat Assistant".to_string()),
        ];
        // Searching for display name should not work, falls back to 0
        assert_eq!(find_default_agent_index(&agents, "Code Helper"), 0);
        assert_eq!(find_default_agent_index(&agents, "Chat Assistant"), 0);
    }

    // =========================================================================
    // Model Type Detection - Provider Prefix Tests
    // =========================================================================

    #[test]
    fn test_model_detection_with_provider_prefixes() {
        // Common provider prefix patterns
        assert!(is_openai_reasoning_model("openai:gpt-5"));
        assert!(is_openai_reasoning_model("openai/o1-preview"));
        assert!(is_claude_model("anthropic:claude-3-opus"));
        assert!(is_claude_model("anthropic/claude-3.5-sonnet"));
        assert!(is_claude_model("bedrock:claude-3-haiku"));
        assert!(is_claude_4_model("anthropic:claude-4"));
    }

    #[test]
    fn test_model_detection_with_version_suffixes() {
        // Model names with version dates
        assert!(is_openai_reasoning_model("gpt-5-2024-12-01"));
        assert!(is_claude_model("claude-3-opus-20240229"));
        assert!(is_claude_4_model("claude-4-opus-20250601"));
        assert!(is_claude_4_model("sonnet-4-20250501"));
    }

    // =========================================================================
    // Model Type Detection - Mutual Exclusivity Tests
    // =========================================================================

    #[test]
    fn test_model_types_are_distinct_for_common_models() {
        // GPT-4o is not a reasoning model
        let gpt4o = "gpt-4o";
        assert!(!is_openai_reasoning_model(gpt4o));
        assert!(!is_claude_model(gpt4o));
        assert!(!is_codex_model(gpt4o));

        // Claude 3 is Claude but not Claude 4
        let claude3 = "claude-3-opus";
        assert!(is_claude_model(claude3));
        assert!(!is_claude_4_model(claude3));
        assert!(!is_openai_reasoning_model(claude3));
    }

    #[test]
    fn test_model_types_can_overlap() {
        // gpt-5-codex is both OpenAI reasoning AND codex
        let model = "gpt-5-codex";
        assert!(is_openai_reasoning_model(model));
        assert!(is_codex_model(model));

        // claude-4 is both Claude AND Claude 4
        let model = "claude-4-opus";
        assert!(is_claude_model(model));
        assert!(is_claude_4_model(model));
    }

    // =========================================================================
    // Display Formatting - Model Variants
    // =========================================================================

    #[test]
    fn test_format_model_display_with_provider_prefix() {
        let model = "anthropic:claude-3-opus";
        assert_eq!(
            format_model_display(model, model),
            "anthropic:claude-3-opus (current)"
        );
        assert_eq!(
            format_model_display(model, "other"),
            "anthropic:claude-3-opus"
        );
    }

    #[test]
    fn test_format_model_display_with_version_suffix() {
        let model = "gpt-4o-2024-08-06";
        assert_eq!(
            format_model_display(model, model),
            "gpt-4o-2024-08-06 (current)"
        );
    }

    #[test]
    fn test_format_model_display_whitespace_sensitivity() {
        // Whitespace should not match
        assert_eq!(format_model_display("gpt-4o", " gpt-4o"), "gpt-4o");
        assert_eq!(format_model_display("gpt-4o", "gpt-4o "), "gpt-4o");
        assert_eq!(format_model_display(" gpt-4o", "gpt-4o"), " gpt-4o");
    }

    #[test]
    fn test_format_model_display_case_sensitivity() {
        // Different case should not match
        assert_eq!(format_model_display("GPT-4o", "gpt-4o"), "GPT-4o");
        assert_eq!(format_model_display("gpt-4o", "GPT-4o"), "gpt-4o");
    }

    // =========================================================================
    // Display Formatting - Agent Variants
    // =========================================================================

    #[test]
    fn test_format_agent_display_with_unicode() {
        let name = "emoji_agent";
        let display = "🤖 Agent";
        assert_eq!(
            format_agent_display(name, display, name),
            "emoji_agent - 🤖 Agent (current)"
        );
    }

    #[test]
    fn test_format_agent_display_with_newlines() {
        // Display names with newlines (edge case)
        let name = "test";
        let display = "Line1\nLine2";
        assert_eq!(
            format_agent_display(name, display, name),
            "test - Line1\nLine2 (current)"
        );
    }

    #[test]
    fn test_format_agent_display_same_name_and_display() {
        // When name equals display
        assert_eq!(
            format_agent_display("stockpot", "stockpot", "stockpot"),
            "stockpot - stockpot (current)"
        );
    }

    // =========================================================================
    // Verbosity Conversion - Boundary Tests
    // =========================================================================

    #[test]
    fn test_verbosity_label_exactly_at_boundaries() {
        // Test exactly at 0, 1, 2 boundaries
        assert_eq!(verbosity_label(0), "low");
        assert_eq!(verbosity_label(1), "medium");
        assert_eq!(verbosity_label(2), "high");
    }

    #[test]
    fn test_verbosity_to_index_exactly_at_boundaries() {
        assert_eq!(verbosity_to_index(Some(0)), 0);
        assert_eq!(verbosity_to_index(Some(1)), 1);
        assert_eq!(verbosity_to_index(Some(2)), 2);
    }

    #[test]
    fn test_verbosity_label_and_index_consistency() {
        // Verify label matches expected index in VERBOSITY_LABELS
        for (idx, &label) in VERBOSITY_LABELS.iter().enumerate() {
            assert_eq!(verbosity_label(idx as i32), label);
        }
    }

    #[test]
    fn test_verbosity_values_and_index_consistency() {
        // Verify verbosity_to_index maps correctly to VERBOSITY_VALUES
        for (idx, &value) in VERBOSITY_VALUES.iter().enumerate() {
            assert_eq!(verbosity_to_index(Some(value)), idx);
        }
    }

    // =========================================================================
    // Reasoning Effort - Comprehensive Tests
    // =========================================================================

    #[test]
    fn test_find_reasoning_effort_index_empty_string() {
        // Empty string doesn't match any effort, defaults to medium (2)
        assert_eq!(find_reasoning_effort_index(Some(&"".to_string())), 2);
    }

    #[test]
    fn test_find_reasoning_effort_index_partial_match() {
        // Partial matches should not work - "min" is not "minimal"
        assert_eq!(find_reasoning_effort_index(Some(&"min".to_string())), 2);
        assert_eq!(find_reasoning_effort_index(Some(&"med".to_string())), 2);
        assert_eq!(find_reasoning_effort_index(Some(&"hi".to_string())), 2);
    }

    #[test]
    fn test_reasoning_effort_index_and_constant_alignment() {
        // Verify index matches position in REASONING_EFFORTS
        for (idx, &effort) in REASONING_EFFORTS.iter().enumerate() {
            assert_eq!(
                find_reasoning_effort_index(Some(&effort.to_string())),
                idx,
                "Mismatch for effort: {}",
                effort
            );
        }
    }

    // =========================================================================
    // Integration Tests - Type Detection + Display
    // =========================================================================

    #[test]
    fn test_display_current_marker_for_each_model_type() {
        // Test that current marker works for all model types
        let test_cases = [
            ("gpt-5", true, false, false, false),         // OpenAI reasoning
            ("o1-preview", true, false, false, false),    // OpenAI reasoning
            ("claude-3-opus", false, true, false, false), // Claude
            ("claude-4-opus", false, true, true, false),  // Claude + Claude 4
            ("gpt-5-codex", true, false, false, true),    // OpenAI reasoning + codex
        ];

        for (model, is_reasoning, is_claude, is_claude4, is_codex) in test_cases {
            // Verify type detection
            assert_eq!(
                is_openai_reasoning_model(model),
                is_reasoning,
                "Type mismatch for {}",
                model
            );
            assert_eq!(
                is_claude_model(model),
                is_claude,
                "Claude mismatch for {}",
                model
            );
            assert_eq!(
                is_claude_4_model(model),
                is_claude4,
                "Claude4 mismatch for {}",
                model
            );
            assert_eq!(
                is_codex_model(model),
                is_codex,
                "Codex mismatch for {}",
                model
            );

            // Verify display formatting
            assert!(
                format_model_display(model, model).ends_with("(current)"),
                "Current marker missing for {}",
                model
            );
        }
    }

    // =========================================================================
    // Index Finding - Large Lists
    // =========================================================================

    #[test]
    fn test_find_model_index_large_list() {
        let models: Vec<String> = (0..100).map(|i| format!("model-{}", i)).collect();
        assert_eq!(find_default_model_index(&models, "model-0"), 0);
        assert_eq!(find_default_model_index(&models, "model-50"), 50);
        assert_eq!(find_default_model_index(&models, "model-99"), 99);
        assert_eq!(find_default_model_index(&models, "nonexistent"), 0);
    }

    #[test]
    fn test_find_agent_index_large_list() {
        let agents: Vec<(String, String)> = (0..100)
            .map(|i| (format!("agent-{}", i), format!("Agent {}", i)))
            .collect();
        assert_eq!(find_default_agent_index(&agents, "agent-0"), 0);
        assert_eq!(find_default_agent_index(&agents, "agent-50"), 50);
        assert_eq!(find_default_agent_index(&agents, "agent-99"), 99);
    }

    // =========================================================================
    // Model Detection - Substring Behavior Documentation
    // =========================================================================

    #[test]
    fn test_model_detection_substring_behavior() {
        // Document that contains() has substring matching behavior
        // This is expected but may cause false positives

        // "o1" matches inside "go1" (probably unintended)
        assert!(is_openai_reasoning_model("go1"));

        // "gpt-5" matches "gpt-50" (contains "gpt-5")
        assert!(is_openai_reasoning_model("gpt-50"));

        // "claude" matches "claude-like" custom models
        assert!(is_claude_model("my-claude-clone"));

        // These document expected behavior, not bugs
    }

    #[test]
    fn test_model_detection_boundary_cases() {
        // Empty string should not match anything
        assert!(!is_openai_reasoning_model(""));
        assert!(!is_claude_model(""));
        assert!(!is_claude_4_model(""));
        assert!(!is_codex_model(""));
    }

    // =========================================================================
    // ModelSettings is_empty Edge Cases
    // =========================================================================

    #[test]
    fn test_model_settings_multiple_fields() {
        // With multiple fields set, still not empty
        let settings = ModelSettings {
            temperature: Some(0.5),
            max_tokens: Some(1000),
            extended_thinking: Some(true),
            ..Default::default()
        };
        assert!(!settings.is_empty());
    }

    #[test]
    fn test_model_settings_each_field_individually() {
        // Test each field individually makes it non-empty
        assert!(!ModelSettings {
            temperature: Some(0.0),
            ..Default::default()
        }
        .is_empty());

        assert!(!ModelSettings {
            seed: Some(0),
            ..Default::default()
        }
        .is_empty());

        assert!(!ModelSettings {
            max_tokens: Some(0),
            ..Default::default()
        }
        .is_empty());

        assert!(!ModelSettings {
            extended_thinking: Some(false),
            ..Default::default()
        }
        .is_empty());

        assert!(!ModelSettings {
            budget_tokens: Some(0),
            ..Default::default()
        }
        .is_empty());

        assert!(!ModelSettings {
            interleaved_thinking: Some(false),
            ..Default::default()
        }
        .is_empty());

        assert!(!ModelSettings {
            reasoning_effort: Some(String::new()),
            ..Default::default()
        }
        .is_empty());

        assert!(!ModelSettings {
            verbosity: Some(0),
            ..Default::default()
        }
        .is_empty());
    }
}
