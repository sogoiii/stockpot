//! Model registry for loading and managing model configurations.
//!
//! This module provides `ModelRegistry` which handles:
//! - Loading models from the database
//! - Adding/removing models from the database
//! - Listing available models based on provider availability

use std::collections::HashMap;
use std::path::PathBuf;

use rusqlite::params;

use crate::db::Database;

use super::model_config::ModelConfig;
use super::types::{CustomEndpoint, ModelConfigError, ModelType};
use super::utils::{build_custom_endpoint, has_api_key, has_oauth_tokens, parse_model_type};

/// Registry of available models loaded from configuration files.
#[derive(Debug, Default)]
pub struct ModelRegistry {
    models: HashMap<String, ModelConfig>,
}

impl ModelRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Load models with in-memory defaults only.
    /// **Deprecated**: Use `load_from_db()` instead for database-backed storage.
    #[deprecated(note = "Use load_from_db() instead")]
    pub fn load() -> Result<Self, ModelConfigError> {
        // Returns empty registry - models come from database now
        Ok(Self::new())
    }

    /// Load models from the database.
    ///
    /// Models are added explicitly via `/add_model` or OAuth flows.
    /// The models.dev catalog provides available models, and users
    /// choose which ones to configure.
    pub fn load_from_db(db: &Database) -> Result<Self, ModelConfigError> {
        let mut registry = Self::new();

        // Load all models from database
        let mut stmt = db
            .conn()
            .prepare(
                "SELECT name, model_type, model_id, context_length, supports_thinking,
                        supports_vision, supports_tools, description, api_endpoint,
                        api_key_env, headers, azure_deployment, azure_api_version
                 FROM models ORDER BY name",
            )
            .map_err(|e| ModelConfigError::Io(std::io::Error::other(e.to_string())))?;

        let rows = stmt
            .query_map([], |row| {
                let model_type_str: String = row.get(1)?;
                let headers_json: Option<String> = row.get(10)?;

                Ok(ModelConfig {
                    name: row.get(0)?,
                    model_type: parse_model_type(&model_type_str),
                    model_id: row.get(2)?,
                    context_length: row.get::<_, i64>(3)? as usize,
                    supports_thinking: row.get::<_, i64>(4)? != 0,
                    supports_vision: row.get::<_, i64>(5)? != 0,
                    supports_tools: row.get::<_, i64>(6)? != 0,
                    description: row.get(7)?,
                    custom_endpoint: build_custom_endpoint(
                        row.get::<_, Option<String>>(8)?,
                        row.get::<_, Option<String>>(9)?,
                        headers_json,
                    ),
                    azure_deployment: row.get(11)?,
                    azure_api_version: row.get(12)?,
                    round_robin_models: Vec::new(),
                })
            })
            .map_err(|e| ModelConfigError::Io(std::io::Error::other(e.to_string())))?;

        for config in rows.flatten() {
            tracing::debug!(
                model = %config.name,
                model_type = %config.model_type,
                "Loaded model from database"
            );
            registry.models.insert(config.name.clone(), config);
        }

        tracing::debug!(
            total_models = registry.models.len(),
            "ModelRegistry loaded from database"
        );

        Ok(registry)
    }

    /// Add a model to the database.
    ///
    /// For backwards compatibility, this defaults to source="custom".
    /// Use `add_model_to_db_with_source` to specify the source explicitly.
    pub fn add_model_to_db(db: &Database, config: &ModelConfig) -> Result<(), ModelConfigError> {
        // Infer source from model type
        let source = match config.model_type {
            ModelType::ClaudeCode | ModelType::ChatgptOauth => "oauth",
            _ if config.custom_endpoint.is_some() => "custom",
            _ => "catalog",
        };
        Self::add_model_to_db_with_source(db, config, source)
    }

    /// Add a model to the database with explicit source tracking.
    ///
    /// Source values:
    /// - "catalog" - From the build-time models.dev catalog
    /// - "oauth" - From OAuth authentication (ChatGPT, Claude Code)
    /// - "custom" - User-added custom endpoint
    pub fn add_model_to_db_with_source(
        db: &Database,
        config: &ModelConfig,
        source: &str,
    ) -> Result<(), ModelConfigError> {
        tracing::debug!(
            model = %config.name,
            model_type = %config.model_type,
            source = %source,
            "Saving model to database"
        );

        let headers_json = config
            .custom_endpoint
            .as_ref()
            .map(|e| serde_json::to_string(&e.headers).unwrap_or_default());

        let result = db.conn().execute(
            "INSERT OR REPLACE INTO models (name, model_type, model_id, context_length,
                supports_thinking, supports_vision, supports_tools, description,
                api_endpoint, api_key_env, headers, is_builtin, source, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, unixepoch())",
            params![
                &config.name,
                config.model_type.to_string(),
                &config.model_id,
                config.context_length as i64,
                config.supports_thinking as i64,
                config.supports_vision as i64,
                config.supports_tools as i64,
                &config.description,
                config.custom_endpoint.as_ref().map(|e| &e.url),
                config
                    .custom_endpoint
                    .as_ref()
                    .and_then(|e| e.api_key.as_ref()),
                headers_json,
                source,
            ],
        );

        match &result {
            Ok(rows) => tracing::debug!(rows_affected = rows, "Model saved successfully"),
            Err(e) => tracing::error!(error = %e, model = %config.name, "Failed to save model"),
        }

        result.map_err(|e| ModelConfigError::Io(std::io::Error::other(e.to_string())))?;

        Ok(())
    }

    /// Remove a custom model from the database.
    pub fn remove_model_from_db(db: &Database, name: &str) -> Result<(), ModelConfigError> {
        db.conn()
            .execute(
                "DELETE FROM models WHERE name = ? AND is_builtin = 0",
                params![name],
            )
            .map_err(|e| ModelConfigError::Io(std::io::Error::other(e.to_string())))?;
        Ok(())
    }

    /// Reload the registry from database.
    pub fn reload_from_db(&mut self, db: &Database) -> Result<(), ModelConfigError> {
        self.models.clear();
        let fresh = Self::load_from_db(db)?;
        self.models = fresh.models;
        Ok(())
    }

    /// Load models from a specific JSON file.
    pub fn load_file(&mut self, path: &PathBuf) -> Result<(), ModelConfigError> {
        let content = std::fs::read_to_string(path)?;
        let models: Vec<ModelConfig> = serde_json::from_str(&content)?;

        for model in models {
            self.models.insert(model.name.clone(), model);
        }

        Ok(())
    }

    /// Add a model to the registry.
    pub fn add(&mut self, config: ModelConfig) {
        self.models.insert(config.name.clone(), config);
    }

    /// Get a model by name.
    pub fn get(&self, name: &str) -> Option<&ModelConfig> {
        self.models.get(name)
    }

    /// Check if a model exists.
    pub fn contains(&self, name: &str) -> bool {
        self.models.contains_key(name)
    }

    /// Get all model names.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.models.keys().map(|s| s.as_str())
    }

    /// Get all model names as a sorted vector.
    pub fn list(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.models.keys().map(|s| s.as_str()).collect();
        names.sort();
        names
    }

    /// Get all models.
    pub fn all(&self) -> impl Iterator<Item = &ModelConfig> {
        self.models.values()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.models.is_empty()
    }

    /// Number of models in the registry.
    pub fn len(&self) -> usize {
        self.models.len()
    }

    /// Reload the registry with in-memory defaults only.
    /// **Deprecated**: Use `reload_from_db()` instead for database-backed storage.
    #[deprecated(note = "Use reload_from_db() instead")]
    pub fn reload(&mut self) -> Result<(), ModelConfigError> {
        // Just clear - models come from database now
        self.models.clear();
        Ok(())
    }

    /// Get the config directory path.
    pub fn config_dir() -> Result<PathBuf, ModelConfigError> {
        let home = dirs::home_dir().ok_or(ModelConfigError::ConfigDirNotFound)?;
        Ok(home.join(".stockpot"))
    }

    /// List only models that have valid provider configuration.
    /// Checks for API keys in database or environment, or OAuth tokens.
    pub fn list_available(&self, db: &Database) -> Vec<String> {
        tracing::debug!(
            total_in_registry = self.models.len(),
            "list_available: checking models"
        );

        let mut available: Vec<String> = self
            .models
            .iter()
            .filter(|(name, config)| {
                let is_available = self.is_provider_available(db, name, config);
                tracing::debug!(
                    model = %name,
                    model_type = %config.model_type,
                    available = is_available,
                    "Provider availability check"
                );
                is_available
            })
            .map(|(name, _)| name.clone())
            .collect();
        available.sort();

        tracing::debug!(
            available_count = available.len(),
            "list_available: filtered result"
        );

        available
    }

    /// Check if a model's provider is available (has API key in DB/env or OAuth tokens).
    fn is_provider_available(&self, db: &Database, _name: &str, config: &ModelConfig) -> bool {
        match config.model_type {
            ModelType::Openai => has_api_key(db, "OPENAI_API_KEY"),
            ModelType::Anthropic => has_api_key(db, "ANTHROPIC_API_KEY"),
            ModelType::Gemini => {
                has_api_key(db, "GEMINI_API_KEY") || has_api_key(db, "GOOGLE_API_KEY")
            }
            ModelType::ClaudeCode => {
                // Check if we have valid OAuth tokens
                has_oauth_tokens(db, "claude-code")
            }
            ModelType::ChatgptOauth => {
                // Check if we have valid OAuth tokens
                has_oauth_tokens(db, "chatgpt")
            }
            ModelType::AzureOpenai => {
                has_api_key(db, "AZURE_OPENAI_API_KEY") || has_api_key(db, "AZURE_OPENAI_ENDPOINT")
            }
            ModelType::CustomOpenai | ModelType::CustomAnthropic => {
                // Custom endpoints - check if API key is configured
                // The api_key can be a literal or $ENV_VAR reference
                config
                    .custom_endpoint
                    .as_ref()
                    .map(|e| {
                        e.api_key.as_ref().is_some_and(|key| {
                            if key.starts_with('$') {
                                // It's an env var reference, check DB then env
                                let var_name = key
                                    .trim_start_matches('$')
                                    .trim_matches(|c| c == '{' || c == '}');
                                has_api_key(db, var_name)
                            } else {
                                // It's a literal key
                                !key.is_empty()
                            }
                        })
                    })
                    .unwrap_or(false)
            }
            ModelType::Openrouter => has_api_key(db, "OPENROUTER_API_KEY"),
            ModelType::RoundRobin => true, // Round robin is always "available" if it exists
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_model(name: &str) -> ModelConfig {
        ModelConfig {
            name: name.to_string(),
            model_type: ModelType::Openai,
            model_id: Some(format!("{}-id", name)),
            context_length: 8192,
            supports_thinking: false,
            supports_vision: false,
            supports_tools: true,
            description: Some(format!("Test model {}", name)),
            custom_endpoint: None,
            azure_deployment: None,
            azure_api_version: None,
            round_robin_models: Vec::new(),
        }
    }

    // =========================================================================
    // Basic Registry Tests
    // =========================================================================

    #[test]
    fn test_registry_starts_empty() {
        let registry = ModelRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn test_registry_default_is_empty() {
        let registry = ModelRegistry::default();
        assert!(registry.is_empty());
    }

    #[test]
    fn test_registry_new_equals_default() {
        let new_reg = ModelRegistry::new();
        let default_reg = ModelRegistry::default();
        assert_eq!(new_reg.len(), default_reg.len());
    }

    // =========================================================================
    // Add/Get/Contains Tests
    // =========================================================================

    #[test]
    fn test_add_and_get() {
        let mut registry = ModelRegistry::new();
        let model = create_test_model("gpt-4");
        registry.add(model);

        assert!(!registry.is_empty());
        assert_eq!(registry.len(), 1);
        assert!(registry.get("gpt-4").is_some());
        assert_eq!(registry.get("gpt-4").unwrap().name, "gpt-4");
    }

    #[test]
    fn test_get_nonexistent() {
        let registry = ModelRegistry::new();
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_contains() {
        let mut registry = ModelRegistry::new();
        registry.add(create_test_model("gpt-4"));

        assert!(registry.contains("gpt-4"));
        assert!(!registry.contains("claude"));
    }

    #[test]
    fn test_add_overwrites_existing() {
        let mut registry = ModelRegistry::new();
        let model1 = create_test_model("gpt-4");
        let mut model2 = create_test_model("gpt-4");
        model2.context_length = 16384;

        registry.add(model1);
        registry.add(model2);

        assert_eq!(registry.len(), 1);
        assert_eq!(registry.get("gpt-4").unwrap().context_length, 16384);
    }

    // =========================================================================
    // List/Names Tests
    // =========================================================================

    #[test]
    fn test_names_iterator() {
        let mut registry = ModelRegistry::new();
        registry.add(create_test_model("model-a"));
        registry.add(create_test_model("model-b"));

        let names: Vec<&str> = registry.names().collect();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"model-a"));
        assert!(names.contains(&"model-b"));
    }

    #[test]
    fn test_list_is_sorted() {
        let mut registry = ModelRegistry::new();
        registry.add(create_test_model("zebra"));
        registry.add(create_test_model("alpha"));
        registry.add(create_test_model("middle"));

        let list = registry.list();
        assert_eq!(list, vec!["alpha", "middle", "zebra"]);
    }

    #[test]
    fn test_all_iterator() {
        let mut registry = ModelRegistry::new();
        registry.add(create_test_model("model-1"));
        registry.add(create_test_model("model-2"));

        let all: Vec<&ModelConfig> = registry.all().collect();
        assert_eq!(all.len(), 2);
    }

    // =========================================================================
    // Edge Cases
    // =========================================================================

    #[test]
    fn test_empty_model_name() {
        let mut registry = ModelRegistry::new();
        let mut model = create_test_model("");
        model.name = String::new();
        registry.add(model);

        assert!(registry.contains(""));
        assert!(registry.get("").is_some());
    }

    #[test]
    fn test_unicode_model_name() {
        let mut registry = ModelRegistry::new();
        registry.add(create_test_model("模型-日本語"));

        assert!(registry.contains("模型-日本語"));
    }

    #[test]
    fn test_model_name_with_special_chars() {
        let mut registry = ModelRegistry::new();
        registry.add(create_test_model("openai:gpt-4"));
        registry.add(create_test_model("provider/model"));

        assert!(registry.contains("openai:gpt-4"));
        assert!(registry.contains("provider/model"));
    }

    #[test]
    fn test_many_models() {
        let mut registry = ModelRegistry::new();
        for i in 0..100 {
            registry.add(create_test_model(&format!("model-{}", i)));
        }

        assert_eq!(registry.len(), 100);
        assert!(registry.contains("model-0"));
        assert!(registry.contains("model-99"));
    }

    // =========================================================================
    // Model Type Tests
    // =========================================================================

    #[test]
    fn test_different_model_types() {
        let mut registry = ModelRegistry::new();

        let mut openai = create_test_model("openai-model");
        openai.model_type = ModelType::Openai;

        let mut anthropic = create_test_model("anthropic-model");
        anthropic.model_type = ModelType::Anthropic;

        let mut gemini = create_test_model("gemini-model");
        gemini.model_type = ModelType::Gemini;

        registry.add(openai);
        registry.add(anthropic);
        registry.add(gemini);

        assert_eq!(registry.len(), 3);
        assert_eq!(
            registry.get("openai-model").unwrap().model_type,
            ModelType::Openai
        );
        assert_eq!(
            registry.get("anthropic-model").unwrap().model_type,
            ModelType::Anthropic
        );
        assert_eq!(
            registry.get("gemini-model").unwrap().model_type,
            ModelType::Gemini
        );
    }

    // =========================================================================
    // Custom Endpoint Tests
    // =========================================================================

    #[test]
    fn test_model_with_custom_endpoint() {
        let mut registry = ModelRegistry::new();
        let mut model = create_test_model("custom-model");
        model.model_type = ModelType::CustomOpenai;
        model.custom_endpoint = Some(CustomEndpoint {
            url: "https://custom.api.com/v1".to_string(),
            api_key: Some("$CUSTOM_API_KEY".to_string()),
            headers: HashMap::new(),
        });

        registry.add(model);

        let retrieved = registry.get("custom-model").unwrap();
        assert!(retrieved.custom_endpoint.is_some());
        assert_eq!(
            retrieved.custom_endpoint.as_ref().unwrap().url,
            "https://custom.api.com/v1"
        );
    }

    // =========================================================================
    // Config Dir Test
    // =========================================================================

    #[test]
    fn test_config_dir_returns_path() {
        let result = ModelRegistry::config_dir();
        assert!(result.is_ok());
        let path = result.unwrap();
        assert!(path.ends_with(".stockpot"));
    }

    // =========================================================================
    // Debug Trait Test
    // =========================================================================

    #[test]
    fn test_registry_debug() {
        let registry = ModelRegistry::new();
        let debug_str = format!("{:?}", registry);
        assert!(debug_str.contains("ModelRegistry"));
    }
}
