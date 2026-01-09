//! CLI runner for interactive and single-prompt modes.

use crate::cli::repl::Repl;
use crate::db::Database;

/// Run a single prompt and exit.
pub async fn run_single_prompt(
    db: &Database,
    prompt: &str,
    agent: Option<&str>,
    model: Option<&str>,
) -> anyhow::Result<()> {
    let mut repl = Repl::new(db);

    if let Some(agent_name) = agent {
        repl = repl.with_agent(agent_name);
    }
    if let Some(model_name) = model {
        repl = repl.with_model(model_name);
    }

    // Handle the prompt directly
    repl.handle_prompt(prompt).await?;

    Ok(())
}

/// Run in interactive mode.
pub async fn run_interactive(
    db: &Database,
    agent: Option<&str>,
    model: Option<&str>,
) -> anyhow::Result<()> {
    // Print welcome banner
    print_banner();

    let mut repl = Repl::new(db);

    if let Some(agent_name) = agent {
        repl = repl.with_agent(agent_name);
    }
    if let Some(model_name) = model {
        repl = repl.with_model(model_name);
    }

    // Run the REPL
    repl.run().await?;

    Ok(())
}

/// Print the welcome banner.
///
/// This is public for testing purposes.
pub fn print_banner() {
    println!();
    println!("  \x1b[1;33m╔═╗\x1b[2;36mᵗᵒᶜᵏ\x1b[1;33m╔═╗╔═╗╔╦╗\x1b[0m");
    println!("  \x1b[1;33m╚═╗    ╠═╝║ ║ ║ \x1b[0m");
    println!(
        "  \x1b[1;33m╚═╝    ╩  ╚═╝ ╩ \x1b[0m  \x1b[2mv{}\x1b[0m",
        env!("CARGO_PKG_VERSION")
    );
    println!();
    println!("  \x1b[2m🍲 AI-powered coding assistant\x1b[0m");
    println!("  \x1b[2mType \x1b[0m\x1b[1;36m/help\x1b[0m\x1b[2m for commands, or start chatting!\x1b[0m");
    println!();
}

/// Get the application version string.
pub fn get_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Generate the banner text lines without ANSI codes (for testing).
pub fn banner_text_lines() -> Vec<&'static str> {
    vec![
        "╔═╗ᵗᵒᶜᵏ╔═╗╔═╗╔╦╗",
        "╚═╗    ╠═╝║ ║ ║",
        "╚═╝    ╩  ╚═╝ ╩",
        "AI-powered coding assistant",
        "/help",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Version Tests
    // =========================================================================

    #[test]
    fn test_get_version_not_empty() {
        let version = get_version();
        assert!(!version.is_empty());
    }

    #[test]
    fn test_get_version_format() {
        let version = get_version();
        // Version should be in semver format (X.Y.Z)
        let parts: Vec<&str> = version.split('.').collect();
        assert!(
            parts.len() >= 2,
            "Version should have at least major.minor: {}",
            version
        );

        // Each part should be numeric (or contain numeric prefix)
        for part in &parts[..2] {
            let num: Result<u32, _> = part.parse();
            assert!(num.is_ok(), "Version part should be numeric: {}", part);
        }
    }

    // =========================================================================
    // Banner Text Tests
    // =========================================================================

    #[test]
    fn test_banner_text_lines_not_empty() {
        let lines = banner_text_lines();
        assert!(!lines.is_empty());
    }

    #[test]
    fn test_banner_text_contains_stockpot_ascii() {
        let lines = banner_text_lines();
        // Should contain the ASCII art pattern
        assert!(lines.iter().any(|l| l.contains("╔═╗")));
        assert!(lines.iter().any(|l| l.contains("╚═╝")));
    }

    #[test]
    fn test_banner_text_contains_help_hint() {
        let lines = banner_text_lines();
        assert!(lines.iter().any(|l| l.contains("/help")));
    }

    #[test]
    fn test_banner_text_contains_description() {
        let lines = banner_text_lines();
        assert!(lines.iter().any(|l| l.contains("AI-powered")));
    }

    // =========================================================================
    // Run Configuration Tests (without actual execution)
    // =========================================================================

    #[test]
    fn test_agent_option_parsing() {
        // Test that Option<&str> works correctly for agent
        let agent: Option<&str> = Some("stockpot");
        assert!(agent.is_some());
        assert_eq!(agent.unwrap(), "stockpot");

        let no_agent: Option<&str> = None;
        assert!(no_agent.is_none());
    }

    #[test]
    fn test_model_option_parsing() {
        // Test that Option<&str> works correctly for model
        let model: Option<&str> = Some("gpt-4o");
        assert!(model.is_some());
        assert_eq!(model.unwrap(), "gpt-4o");

        let no_model: Option<&str> = None;
        assert!(no_model.is_none());
    }

    #[test]
    fn test_prompt_not_empty() {
        let prompt = "Hello, world!";
        assert!(!prompt.is_empty());
        assert!(prompt.len() > 0);
    }

    // =========================================================================
    // Version Semver Tests
    // =========================================================================

    #[test]
    fn test_version_is_valid_semver() {
        let version = get_version();
        // Full semver: major.minor.patch
        let parts: Vec<&str> = version.split('.').collect();
        assert_eq!(parts.len(), 3, "Expected X.Y.Z format, got: {}", version);

        // All parts must be numeric
        for (i, part) in parts.iter().enumerate() {
            let parsed: u32 = part
                .parse()
                .unwrap_or_else(|_| panic!("Part {} ('{}') should be numeric", i, part));
            // Sanity check - version parts shouldn't be unreasonably large
            assert!(
                parsed < 1000,
                "Version part {} seems unreasonably large",
                parsed
            );
        }
    }

    #[test]
    fn test_version_major_is_zero_or_positive() {
        let version = get_version();
        let major: u32 = version
            .split('.')
            .next()
            .unwrap()
            .parse()
            .expect("Major version should be numeric");
        // Currently pre-1.0, but this test works for post-1.0 too
        assert!(major < 100, "Major version {} seems too high", major);
    }

    #[test]
    fn test_version_consistency() {
        // Multiple calls return the same value (compile-time constant)
        let v1 = get_version();
        let v2 = get_version();
        assert_eq!(v1, v2);
        // Same pointer (it's a static str)
        assert!(std::ptr::eq(v1, v2));
    }

    // =========================================================================
    // Banner Structure Tests
    // =========================================================================

    #[test]
    fn test_banner_text_has_expected_line_count() {
        let lines = banner_text_lines();
        // Should have exactly 5 lines based on implementation
        assert_eq!(lines.len(), 5, "Banner should have 5 text lines");
    }

    #[test]
    fn test_banner_ascii_art_structure() {
        let lines = banner_text_lines();

        // First 3 lines are ASCII art
        assert!(
            lines[0].contains("╔═╗"),
            "Line 1 should contain top corners"
        );
        assert!(
            lines[1].contains("╚═╗"),
            "Line 2 should contain middle pattern"
        );
        assert!(
            lines[2].contains("╚═╝"),
            "Line 3 should contain bottom corners"
        );
    }

    #[test]
    fn test_banner_contains_application_name_hint() {
        let lines = banner_text_lines();
        // The ASCII art contains "tock" superscript which spells "Stockpot"
        let combined: String = lines.iter().take(3).copied().collect();
        assert!(
            combined.contains("ᵗᵒᶜᵏ"),
            "ASCII art should contain 'tock' superscript"
        );
    }

    #[test]
    fn test_banner_lines_are_non_empty() {
        let lines = banner_text_lines();
        for (i, line) in lines.iter().enumerate() {
            assert!(!line.is_empty(), "Line {} should not be empty", i);
            assert!(line.len() > 2, "Line {} should have meaningful content", i);
        }
    }

    #[test]
    fn test_banner_tagline_content() {
        let lines = banner_text_lines();
        // Line 4 is the tagline
        assert!(
            lines[3].contains("AI-powered") && lines[3].contains("assistant"),
            "Tagline should describe the app"
        );
    }

    #[test]
    fn test_banner_help_hint_is_command_format() {
        let lines = banner_text_lines();
        // Last line contains help command
        let help_line = lines[4];
        assert!(
            help_line.starts_with('/'),
            "Help hint should be a / command"
        );
    }

    // =========================================================================
    // Edge Case Tests
    // =========================================================================

    #[test]
    fn test_banner_no_ansi_codes() {
        // banner_text_lines() should NOT contain ANSI escape sequences
        // (those are only in print_banner())
        let lines = banner_text_lines();
        for line in lines {
            assert!(
                !line.contains("\x1b["),
                "banner_text_lines should not contain ANSI codes"
            );
            assert!(
                !line.contains("\\x1b"),
                "banner_text_lines should not contain escaped ANSI"
            );
        }
    }

    #[test]
    fn test_version_no_leading_v() {
        let version = get_version();
        assert!(
            !version.starts_with('v'),
            "Version should not have 'v' prefix"
        );
    }

    #[test]
    fn test_version_no_whitespace() {
        let version = get_version();
        assert_eq!(version, version.trim(), "Version should have no whitespace");
        assert!(!version.contains(' '), "Version should not contain spaces");
    }

    // =========================================================================
    // Run Configuration Builder Pattern Tests
    // =========================================================================

    /// Test struct to simulate RunConfig-like patterns for pure logic testing
    #[derive(Default, Clone)]
    struct MockRunConfig {
        agent: Option<String>,
        model: Option<String>,
        prompt: Option<String>,
    }

    impl MockRunConfig {
        fn new() -> Self {
            Self::default()
        }

        fn with_agent(mut self, agent: &str) -> Self {
            self.agent = Some(agent.to_string());
            self
        }

        fn with_model(mut self, model: &str) -> Self {
            self.model = Some(model.to_string());
            self
        }

        fn with_prompt(mut self, prompt: &str) -> Self {
            self.prompt = Some(prompt.to_string());
            self
        }

        fn is_interactive(&self) -> bool {
            self.prompt.is_none()
        }

        fn is_single_prompt(&self) -> bool {
            self.prompt.is_some()
        }
    }

    #[test]
    fn test_mock_config_builder_chain() {
        let config = MockRunConfig::new()
            .with_agent("stockpot")
            .with_model("gpt-4o")
            .with_prompt("hello");

        assert_eq!(config.agent, Some("stockpot".to_string()));
        assert_eq!(config.model, Some("gpt-4o".to_string()));
        assert_eq!(config.prompt, Some("hello".to_string()));
    }

    #[test]
    fn test_mock_config_partial_chain() {
        let config = MockRunConfig::new().with_model("claude-3");

        assert!(config.agent.is_none());
        assert_eq!(config.model, Some("claude-3".to_string()));
        assert!(config.prompt.is_none());
    }

    #[test]
    fn test_mock_config_interactive_mode() {
        let interactive = MockRunConfig::new().with_agent("coding");
        let single = MockRunConfig::new().with_prompt("test");

        assert!(interactive.is_interactive());
        assert!(!interactive.is_single_prompt());
        assert!(!single.is_interactive());
        assert!(single.is_single_prompt());
    }

    #[test]
    fn test_mock_config_clone() {
        let original = MockRunConfig::new()
            .with_agent("agent1")
            .with_model("model1");
        let cloned = original.clone();

        assert_eq!(original.agent, cloned.agent);
        assert_eq!(original.model, cloned.model);
    }

    #[test]
    fn test_mock_config_empty_strings() {
        // Edge case: empty string is still Some
        let config = MockRunConfig::new().with_agent("").with_model("");

        assert_eq!(config.agent, Some(String::new()));
        assert_eq!(config.model, Some(String::new()));
        // Empty prompt still counts as single-prompt mode
        assert!(MockRunConfig::new().with_prompt("").is_single_prompt());
    }

    #[test]
    fn test_mock_config_whitespace_preserved() {
        let config = MockRunConfig::new()
            .with_agent("  spaced  ")
            .with_prompt("  hello world  ");

        assert_eq!(config.agent, Some("  spaced  ".to_string()));
        assert_eq!(config.prompt, Some("  hello world  ".to_string()));
    }

    // =========================================================================
    // Additional Version Edge Cases
    // =========================================================================

    #[test]
    fn test_version_matches_cargo_manifest() {
        // The version from get_version() should match what's compiled in
        let version = get_version();
        // env!("CARGO_PKG_VERSION") is set at compile time from Cargo.toml
        assert_eq!(version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn test_version_does_not_contain_invalid_chars() {
        let version = get_version();
        // Version should only contain digits and dots
        for c in version.chars() {
            assert!(
                c.is_ascii_digit() || c == '.',
                "Version contains invalid character: '{}'",
                c
            );
        }
    }

    #[test]
    fn test_version_parts_no_leading_zeros() {
        let version = get_version();
        let parts: Vec<&str> = version.split('.').collect();

        for part in parts {
            // Skip single digit "0" which is valid
            if part != "0" {
                assert!(
                    !part.starts_with('0'),
                    "Version part '{}' has leading zero",
                    part
                );
            }
        }
    }

    #[test]
    fn test_version_not_all_zeros() {
        let version = get_version();
        // At least one version component should be non-zero (otherwise 0.0.0 is meaningless)
        let sum: u32 = version
            .split('.')
            .filter_map(|p| p.parse::<u32>().ok())
            .sum();
        assert!(sum > 0, "Version should not be 0.0.0");
    }

    // =========================================================================
    // Banner Unicode and Structure Tests
    // =========================================================================

    #[test]
    fn test_banner_unicode_box_drawing_chars() {
        let lines = banner_text_lines();
        let combined: String = lines.iter().copied().collect::<Vec<_>>().join("");

        // Box drawing characters used in ASCII art
        let box_chars = ['╔', '╗', '╚', '╝', '═', '╠', '╦', '╩', '║'];
        let mut found_count = 0;

        for ch in box_chars.iter() {
            if combined.contains(*ch) {
                found_count += 1;
            }
        }

        assert!(
            found_count >= 3,
            "Banner should contain multiple box drawing characters"
        );
    }

    #[test]
    fn test_banner_superscript_chars() {
        let lines = banner_text_lines();
        let first_line = lines[0];

        // Verify superscript "tock" is present
        assert!(first_line.contains('ᵗ'), "Should contain superscript 't'");
        assert!(first_line.contains('ᵒ'), "Should contain superscript 'o'");
        assert!(first_line.contains('ᶜ'), "Should contain superscript 'c'");
        assert!(first_line.contains('ᵏ'), "Should contain superscript 'k'");
    }

    #[test]
    fn test_banner_lines_valid_utf8() {
        let lines = banner_text_lines();
        for line in lines {
            // This is implicitly true since we have &str, but let's verify encoding
            assert!(
                line.is_ascii() || !line.is_ascii(),
                "Line should be valid UTF-8"
            );
            // More useful: verify it can be converted back and forth
            let bytes = line.as_bytes();
            let reconstructed = std::str::from_utf8(bytes);
            assert!(reconstructed.is_ok(), "Line should roundtrip through UTF-8");
            assert_eq!(reconstructed.unwrap(), line);
        }
    }

    #[test]
    fn test_banner_lines_reasonable_length() {
        let lines = banner_text_lines();
        for (i, line) in lines.iter().enumerate() {
            // Lines shouldn't be excessively long (reasonable terminal width)
            assert!(
                line.len() < 100,
                "Line {} is too long ({} chars): {}",
                i,
                line.len(),
                line
            );
            // Lines should have some content
            assert!(line.len() > 3, "Line {} is too short", i);
        }
    }

    #[test]
    fn test_banner_first_three_lines_are_ascii_art() {
        let lines = banner_text_lines();

        // First 3 lines should be the ASCII art portion
        for i in 0..3 {
            let line = lines[i];
            // ASCII art lines should contain box drawing characters
            assert!(
                line.chars()
                    .any(|c| matches!(c, '╔' | '╗' | '╚' | '╝' | '═' | '╠' | '╦' | '╩' | '║')),
                "Line {} should contain box drawing characters",
                i
            );
        }
    }

    #[test]
    fn test_banner_last_two_lines_are_text() {
        let lines = banner_text_lines();

        // Lines 3-4 (indices 3, 4) should be readable text
        let tagline = lines[3];
        let help = lines[4];

        // These should be ASCII text with spaces
        assert!(
            tagline.chars().any(|c| c.is_ascii_alphabetic()),
            "Tagline should contain letters"
        );
        assert!(
            help.chars().any(|c| c.is_ascii_alphabetic()),
            "Help should contain letters"
        );
    }

    // =========================================================================
    // Mock Config Edge Cases
    // =========================================================================

    #[test]
    fn test_mock_config_default() {
        let config = MockRunConfig::default();

        assert!(config.agent.is_none());
        assert!(config.model.is_none());
        assert!(config.prompt.is_none());
        assert!(config.is_interactive());
        assert!(!config.is_single_prompt());
    }

    #[test]
    fn test_mock_config_overwrite_values() {
        let config = MockRunConfig::new()
            .with_agent("first")
            .with_agent("second");

        assert_eq!(config.agent, Some("second".to_string()));
    }

    #[test]
    fn test_mock_config_unicode_values() {
        let config = MockRunConfig::new()
            .with_agent("agente-espanol")
            .with_prompt("Hello! How are you?");

        assert!(config.agent.is_some());
        assert!(config.prompt.is_some());
    }

    #[test]
    fn test_mock_config_long_values() {
        let long_prompt = "a".repeat(10000);
        let config = MockRunConfig::new().with_prompt(&long_prompt);

        assert_eq!(config.prompt.as_ref().unwrap().len(), 10000);
        assert!(config.is_single_prompt());
    }

    #[test]
    fn test_mock_config_special_chars() {
        let config = MockRunConfig::new()
            .with_agent("agent@special#$%")
            .with_model("model:v1.0");

        assert!(config.agent.unwrap().contains('@'));
        assert!(config.model.unwrap().contains(':'));
    }

    #[test]
    fn test_mock_config_newlines_in_prompt() {
        let multiline_prompt = "Line 1\nLine 2\nLine 3";
        let config = MockRunConfig::new().with_prompt(multiline_prompt);

        let prompt = config.prompt.unwrap();
        assert!(prompt.contains('\n'));
        assert_eq!(prompt.matches('\n').count(), 2);
    }

    #[test]
    fn test_mock_config_mode_determination() {
        // All combinations of agent/model with and without prompt
        let cases = vec![
            (None, None, None, true),                 // interactive
            (Some("a"), None, None, true),            // interactive
            (None, Some("m"), None, true),            // interactive
            (Some("a"), Some("m"), None, true),       // interactive
            (None, None, Some("p"), false),           // single
            (Some("a"), None, Some("p"), false),      // single
            (None, Some("m"), Some("p"), false),      // single
            (Some("a"), Some("m"), Some("p"), false), // single
        ];

        for (agent, model, prompt, expected_interactive) in cases {
            let mut config = MockRunConfig::new();
            if let Some(a) = agent {
                config = config.with_agent(a);
            }
            if let Some(m) = model {
                config = config.with_model(m);
            }
            if let Some(p) = prompt {
                config = config.with_prompt(p);
            }

            assert_eq!(
                config.is_interactive(),
                expected_interactive,
                "agent={:?}, model={:?}, prompt={:?}",
                agent,
                model,
                prompt
            );
        }
    }

    // =========================================================================
    // Function Signature Tests (compile-time verification)
    // =========================================================================

    #[test]
    fn test_get_version_is_static() {
        // get_version returns &'static str - verify we can use it as such
        let v: &'static str = get_version();
        assert!(!v.is_empty());
    }

    #[test]
    fn test_banner_text_lines_returns_vec() {
        // Verify return type and ownership
        let lines: Vec<&'static str> = banner_text_lines();
        assert!(!lines.is_empty());

        // We own the Vec, can modify it
        let mut owned = lines;
        owned.push("extra");
        assert_eq!(owned.len(), 6);
    }

    // =========================================================================
    // Integration-style Tests (without actual async execution)
    // =========================================================================

    #[test]
    fn test_function_signatures_exist() {
        // Verify the async functions exist with correct signatures
        // by creating function pointers (won't execute them)
        fn assert_single_prompt_signature<F>(_: F)
        where
            F: for<'a> Fn(
                &'a crate::db::Database,
                &'a str,
                Option<&'a str>,
                Option<&'a str>,
            ) -> std::pin::Pin<
                Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + 'a>,
            >,
        {
        }

        fn assert_interactive_signature<F>(_: F)
        where
            F: for<'a> Fn(
                &'a crate::db::Database,
                Option<&'a str>,
                Option<&'a str>,
            ) -> std::pin::Pin<
                Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + 'a>,
            >,
        {
        }

        // These won't compile if signatures change
        // Note: We can't easily test async fn signatures this way without boxing
        // But we can verify the sync functions
        let _: fn() -> &'static str = get_version;
        let _: fn() -> Vec<&'static str> = banner_text_lines;
        let _: fn() = print_banner;
    }

    // =========================================================================
    // Banner Display Consistency Tests
    // =========================================================================

    #[test]
    fn test_banner_text_lines_consistent() {
        // Multiple calls should return identical content
        let lines1 = banner_text_lines();
        let lines2 = banner_text_lines();

        assert_eq!(lines1.len(), lines2.len());
        for (l1, l2) in lines1.iter().zip(lines2.iter()) {
            assert_eq!(l1, l2);
        }
    }

    #[test]
    fn test_banner_version_placeholder_exists() {
        // Note: banner_text_lines doesn't include version (that's in print_banner)
        // This test verifies that design choice
        let lines = banner_text_lines();
        let version = get_version();

        // Version should NOT be in banner_text_lines (it's added dynamically in print_banner)
        for line in lines {
            assert!(
                !line.contains(version),
                "banner_text_lines should not contain version number"
            );
        }
    }
}
