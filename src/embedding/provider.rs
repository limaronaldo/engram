//! EmbeddingProvider trait and registry (RML-1231)
//!
//! Extends the [`Embedder`] trait with provider metadata and a runtime registry
//! for discovering and selecting embedding backends.
//!
//! # Overview
//!
//! - [`EmbeddingProviderInfo`] — static metadata about a provider (id, model, dimensions, …)
//! - [`EmbeddingProvider`] — supertrait of [`Embedder`] that exposes provider metadata
//! - [`EmbeddingRegistry`] — a runtime map of named providers with default selection

use std::collections::HashMap;
use std::sync::Arc;

use crate::error::{EngramError, Result};

use super::Embedder;

// ── Provider metadata ─────────────────────────────────────────────────────────

/// Static metadata describing an embedding provider.
///
/// Returned by [`EmbeddingProvider::provider_info`] and exposed through the
/// [`EmbeddingRegistry`] without requiring a live embedding call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingProviderInfo {
    /// Unique, machine-readable identifier (e.g. `"tfidf"`, `"openai-3-small"`).
    pub id: String,
    /// Human-readable name (e.g. `"TF-IDF (local)"`, `"OpenAI text-embedding-3-small"`).
    pub name: String,
    /// Underlying model identifier (e.g. `"tfidf"`, `"text-embedding-3-small"`).
    pub model: String,
    /// Number of dimensions produced by this provider.
    pub dimensions: usize,
    /// Whether this provider requires an API key to operate.
    pub requires_api_key: bool,
    /// Whether this provider runs entirely on the local machine (no network calls).
    pub is_local: bool,
}

// ── EmbeddingProvider trait ───────────────────────────────────────────────────

/// An [`Embedder`] that also exposes self-describing metadata.
///
/// Implement both [`Embedder`] and this trait to participate in the
/// [`EmbeddingRegistry`].
pub trait EmbeddingProvider: Embedder {
    /// Return static metadata for this provider.
    fn provider_info(&self) -> EmbeddingProviderInfo;
}

// ── EmbeddingRegistry ─────────────────────────────────────────────────────────

/// A runtime registry of named [`EmbeddingProvider`] implementations.
///
/// Providers are keyed by [`EmbeddingProviderInfo::id`]. An optional default
/// can be set explicitly via [`EmbeddingRegistry::set_default`]; if none is
/// set, [`EmbeddingRegistry::default_provider`] returns the first registered
/// provider.
pub struct EmbeddingRegistry {
    providers: HashMap<String, Arc<dyn EmbeddingProvider>>,
    /// Insertion order — used to determine the first-registered provider.
    order: Vec<String>,
    /// Explicit default id, if set.
    default_id: Option<String>,
}

impl EmbeddingRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
            order: Vec::new(),
            default_id: None,
        }
    }

    /// Register a provider.
    ///
    /// If a provider with the same id already exists it is replaced.
    /// The insertion order of new ids is preserved for use by
    /// [`EmbeddingRegistry::default_provider`].
    pub fn register(&mut self, provider: Arc<dyn EmbeddingProvider>) {
        let id = provider.provider_info().id.clone();
        if !self.providers.contains_key(&id) {
            self.order.push(id.clone());
        }
        self.providers.insert(id, provider);
    }

    /// Look up a provider by id.
    ///
    /// Returns `None` if no provider with that id has been registered.
    pub fn get(&self, id: &str) -> Option<Arc<dyn EmbeddingProvider>> {
        self.providers.get(id).cloned()
    }

    /// Return metadata for all registered providers, in registration order.
    pub fn list(&self) -> Vec<EmbeddingProviderInfo> {
        self.order
            .iter()
            .filter_map(|id| self.providers.get(id))
            .map(|p| p.provider_info())
            .collect()
    }

    /// Return the default provider.
    ///
    /// - If a default was set via [`EmbeddingRegistry::set_default`], that provider is returned.
    /// - Otherwise the first registered provider is returned.
    /// - Returns `None` if the registry is empty.
    pub fn default_provider(&self) -> Option<Arc<dyn EmbeddingProvider>> {
        if let Some(ref id) = self.default_id {
            // Explicit default may have been de-registered; fall through if so.
            if let Some(p) = self.providers.get(id.as_str()) {
                return Some(p.clone());
            }
        }
        // Fallback: first registered.
        self.order
            .first()
            .and_then(|id| self.providers.get(id.as_str()))
            .cloned()
    }

    /// Change the default provider.
    ///
    /// Returns [`EngramError::InvalidInput`] if `id` is not registered.
    pub fn set_default(&mut self, id: &str) -> Result<()> {
        if self.providers.contains_key(id) {
            self.default_id = Some(id.to_string());
            Ok(())
        } else {
            Err(EngramError::InvalidInput(format!(
                "No embedding provider registered with id '{id}'"
            )))
        }
    }

    /// Return the number of registered providers.
    pub fn count(&self) -> usize {
        self.providers.len()
    }
}

impl Default for EmbeddingRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;

    // ── Mock provider ─────────────────────────────────────────────────────────

    struct MockProvider {
        info: EmbeddingProviderInfo,
    }

    impl MockProvider {
        fn new(id: &str, dimensions: usize) -> Self {
            Self {
                info: EmbeddingProviderInfo {
                    id: id.to_string(),
                    name: format!("Mock ({id})"),
                    model: format!("mock-{id}"),
                    dimensions,
                    requires_api_key: false,
                    is_local: true,
                },
            }
        }
    }

    impl Embedder for MockProvider {
        fn embed(&self, _text: &str) -> Result<Vec<f32>> {
            Ok(vec![0.0_f32; self.info.dimensions])
        }

        fn dimensions(&self) -> usize {
            self.info.dimensions
        }

        fn model_name(&self) -> &str {
            &self.info.model
        }
    }

    impl EmbeddingProvider for MockProvider {
        fn provider_info(&self) -> EmbeddingProviderInfo {
            self.info.clone()
        }
    }

    fn make_provider(id: &str) -> Arc<dyn EmbeddingProvider> {
        Arc::new(MockProvider::new(id, 64))
    }

    // ── Tests ─────────────────────────────────────────────────────────────────

    #[test]
    fn test_register_and_get_by_id() {
        let mut registry = EmbeddingRegistry::new();
        registry.register(make_provider("alpha"));

        let provider = registry.get("alpha");
        assert!(provider.is_some(), "registered provider should be found");
        assert_eq!(provider.unwrap().provider_info().id, "alpha");
    }

    #[test]
    fn test_get_unknown_returns_none() {
        let registry = EmbeddingRegistry::new();
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_list_returns_all_providers() {
        let mut registry = EmbeddingRegistry::new();
        registry.register(make_provider("alpha"));
        registry.register(make_provider("beta"));
        registry.register(make_provider("gamma"));

        let list = registry.list();
        assert_eq!(list.len(), 3);
        let ids: Vec<&str> = list.iter().map(|i| i.id.as_str()).collect();
        assert!(ids.contains(&"alpha"));
        assert!(ids.contains(&"beta"));
        assert!(ids.contains(&"gamma"));
    }

    #[test]
    fn test_list_preserves_insertion_order() {
        let mut registry = EmbeddingRegistry::new();
        registry.register(make_provider("first"));
        registry.register(make_provider("second"));
        registry.register(make_provider("third"));

        let ids: Vec<String> = registry.list().into_iter().map(|i| i.id).collect();
        assert_eq!(ids, vec!["first", "second", "third"]);
    }

    #[test]
    fn test_default_returns_first_registered() {
        let mut registry = EmbeddingRegistry::new();
        assert!(
            registry.default_provider().is_none(),
            "empty registry has no default"
        );

        registry.register(make_provider("first"));
        registry.register(make_provider("second"));

        let default = registry.default_provider().expect("should have a default");
        assert_eq!(default.provider_info().id, "first");
    }

    #[test]
    fn test_set_default_changes_default() {
        let mut registry = EmbeddingRegistry::new();
        registry.register(make_provider("alpha"));
        registry.register(make_provider("beta"));

        registry.set_default("beta").expect("beta is registered");

        let default = registry.default_provider().expect("should have a default");
        assert_eq!(default.provider_info().id, "beta");
    }

    #[test]
    fn test_set_default_unknown_returns_error() {
        let mut registry = EmbeddingRegistry::new();
        let result = registry.set_default("does-not-exist");
        assert!(result.is_err(), "unknown id should return an error");
    }

    #[test]
    fn test_count() {
        let mut registry = EmbeddingRegistry::new();
        assert_eq!(registry.count(), 0);

        registry.register(make_provider("a"));
        assert_eq!(registry.count(), 1);

        registry.register(make_provider("b"));
        assert_eq!(registry.count(), 2);
    }

    #[test]
    fn test_register_replaces_existing_id() {
        let mut registry = EmbeddingRegistry::new();
        registry.register(make_provider("a"));
        // Register a different provider under the same id.
        registry.register(Arc::new(MockProvider {
            info: EmbeddingProviderInfo {
                id: "a".to_string(),
                name: "Updated A".to_string(),
                model: "updated-model".to_string(),
                dimensions: 128,
                requires_api_key: true,
                is_local: false,
            },
        }));

        // Count must remain 1 (no duplicate id).
        assert_eq!(registry.count(), 1);

        let info = registry.get("a").unwrap().provider_info();
        assert_eq!(info.name, "Updated A");
        assert_eq!(info.dimensions, 128);
    }

    #[test]
    fn test_provider_info_fields() {
        let info = EmbeddingProviderInfo {
            id: "test".to_string(),
            name: "Test Provider".to_string(),
            model: "test-model-v1".to_string(),
            dimensions: 256,
            requires_api_key: true,
            is_local: false,
        };
        assert_eq!(info.id, "test");
        assert_eq!(info.dimensions, 256);
        assert!(info.requires_api_key);
        assert!(!info.is_local);
    }

    #[test]
    fn test_embed_via_registry_provider() {
        let mut registry = EmbeddingRegistry::new();
        registry.register(make_provider("mock"));

        let provider = registry.get("mock").expect("mock is registered");
        let embedding = provider.embed("hello world").expect("embed should succeed");
        assert_eq!(embedding.len(), 64);
    }
}
