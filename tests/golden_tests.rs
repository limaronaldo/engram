//! Golden tests - fixture-based tests that lock expected behavior
//!
//! These tests use JSON fixtures to verify that critical functions produce
//! expected outputs. Any change in behavior will cause these tests to fail,
//! signaling a potential breaking change.
//!
//! Run with: cargo test --test golden_tests

use serde::Deserialize;
use std::fs;

// ============================================================================
// WORKSPACE NORMALIZATION GOLDEN TESTS
// ============================================================================

mod workspace_golden {
    use super::*;
    use engram::types::{normalize_workspace, WorkspaceError};

    #[derive(Debug, Deserialize)]
    struct TestCase {
        name: String,
        input: String,
        expected: Expected,
    }

    #[derive(Debug, Deserialize)]
    #[serde(untagged)]
    enum Expected {
        Ok { ok: String },
        Err { err: String },
    }

    #[derive(Debug, Deserialize)]
    struct Fixture {
        test_cases: Vec<TestCase>,
    }

    #[test]
    fn test_workspace_normalization_golden() {
        let fixture_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/workspace_normalization.json"
        );
        let content = fs::read_to_string(fixture_path)
            .expect("Failed to read workspace_normalization.json fixture");
        let fixture: Fixture =
            serde_json::from_str(&content).expect("Failed to parse fixture JSON");

        for case in fixture.test_cases {
            let result = normalize_workspace(&case.input);

            match case.expected {
                Expected::Ok { ok } => {
                    assert!(
                        result.is_ok(),
                        "Case '{}': expected Ok({:?}), got Err({:?})",
                        case.name,
                        ok,
                        result.err()
                    );
                    assert_eq!(
                        result.unwrap(),
                        ok,
                        "Case '{}': normalized value mismatch",
                        case.name
                    );
                }
                Expected::Err { err } => {
                    assert!(
                        result.is_err(),
                        "Case '{}': expected Err({}), got Ok({:?})",
                        case.name,
                        err,
                        result.ok()
                    );
                    let actual_err = match result.unwrap_err() {
                        WorkspaceError::Empty => "Empty",
                        WorkspaceError::TooLong => "TooLong",
                        WorkspaceError::InvalidChars => "InvalidChars",
                        WorkspaceError::Reserved => "Reserved",
                    };
                    assert_eq!(actual_err, err, "Case '{}': error type mismatch", case.name);
                }
            }
        }
    }
}

// ============================================================================
// ALIAS NORMALIZATION GOLDEN TESTS
// ============================================================================

mod alias_golden {
    use super::*;
    use engram::storage::identity_links::normalize_alias;

    #[derive(Debug, Deserialize)]
    struct TestCase {
        name: String,
        input: String,
        expected: String,
    }

    #[derive(Debug, Deserialize)]
    struct Fixture {
        test_cases: Vec<TestCase>,
    }

    #[test]
    fn test_alias_normalization_golden() {
        let fixture_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/alias_normalization.json"
        );
        let content = fs::read_to_string(fixture_path)
            .expect("Failed to read alias_normalization.json fixture");
        let fixture: Fixture =
            serde_json::from_str(&content).expect("Failed to parse fixture JSON");

        for case in fixture.test_cases {
            let result = normalize_alias(&case.input);
            assert_eq!(
                result, case.expected,
                "Case '{}': input={:?}, expected={:?}, got={:?}",
                case.name, case.input, case.expected, result
            );
        }
    }
}

// ============================================================================
// ENTITY EXTRACTION GOLDEN TESTS
// ============================================================================

mod extraction_golden {
    use super::*;
    use engram::intelligence::entity_extraction::{extract_entities, ExtractionConfig};

    #[derive(Debug, Deserialize)]
    struct TestCase {
        name: String,
        input: String,
        expected_entities: Vec<ExpectedEntity>,
    }

    #[derive(Debug, Deserialize)]
    struct ExpectedEntity {
        mention_text: String,
        entity_type: String,
        #[serde(default)]
        count: Option<usize>,
    }

    #[derive(Debug, Deserialize)]
    struct Fixture {
        test_cases: Vec<TestCase>,
    }

    #[test]
    fn test_entity_extraction_golden() {
        let fixture_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/entity_extraction.json"
        );
        let content = fs::read_to_string(fixture_path)
            .expect("Failed to read entity_extraction.json fixture");
        let fixture: Fixture =
            serde_json::from_str(&content).expect("Failed to parse fixture JSON");

        let config = ExtractionConfig {
            lookup_aliases: false,
            ..Default::default()
        };

        for case in fixture.test_cases {
            let result = extract_entities(&case.input, &config, None);

            assert_eq!(
                result.entities.len(),
                case.expected_entities.len(),
                "Case '{}': entity count mismatch. Expected {:?}, got {:?}",
                case.name,
                case.expected_entities
                    .iter()
                    .map(|e| &e.mention_text)
                    .collect::<Vec<_>>(),
                result
                    .entities
                    .iter()
                    .map(|e| &e.mention_text)
                    .collect::<Vec<_>>()
            );

            for (i, expected) in case.expected_entities.iter().enumerate() {
                let actual = &result.entities[i];

                assert_eq!(
                    actual.mention_text, expected.mention_text,
                    "Case '{}': entity {} mention_text mismatch",
                    case.name, i
                );

                let expected_type = format!("{:?}", actual.entity_type);
                assert_eq!(
                    expected_type, expected.entity_type,
                    "Case '{}': entity {} type mismatch",
                    case.name, i
                );

                if let Some(expected_count) = expected.count {
                    assert_eq!(
                        actual.count, expected_count,
                        "Case '{}': entity {} count mismatch",
                        case.name, i
                    );
                }
            }
        }
    }
}

// ============================================================================
// MEMORY TIER GOLDEN TESTS
// ============================================================================

mod tier_golden {
    use engram::types::MemoryTier;

    #[test]
    fn test_tier_string_representations() {
        // Lock the string representations
        assert_eq!(MemoryTier::Permanent.as_str(), "permanent");
        assert_eq!(MemoryTier::Daily.as_str(), "daily");

        // Lock the default TTLs
        assert_eq!(MemoryTier::Permanent.default_ttl_seconds(), None);
        assert_eq!(
            MemoryTier::Daily.default_ttl_seconds(),
            Some(24 * 60 * 60) // 24 hours
        );
    }

    #[test]
    fn test_tier_default() {
        // Lock the default tier
        assert_eq!(MemoryTier::default(), MemoryTier::Permanent);
    }
}

// ============================================================================
// MEMORY TYPE GOLDEN TESTS
// ============================================================================

mod memory_type_golden {
    use engram::types::MemoryType;

    #[test]
    fn test_memory_type_string_representations() {
        // Lock all string representations
        assert_eq!(MemoryType::Note.as_str(), "note");
        assert_eq!(MemoryType::Todo.as_str(), "todo");
        assert_eq!(MemoryType::Issue.as_str(), "issue");
        assert_eq!(MemoryType::Decision.as_str(), "decision");
        assert_eq!(MemoryType::Preference.as_str(), "preference");
        assert_eq!(MemoryType::Learning.as_str(), "learning");
        assert_eq!(MemoryType::Context.as_str(), "context");
        assert_eq!(MemoryType::Credential.as_str(), "credential");
        assert_eq!(MemoryType::Custom.as_str(), "custom");
        assert_eq!(MemoryType::TranscriptChunk.as_str(), "transcript_chunk");
    }

    #[test]
    fn test_memory_type_default() {
        // Lock the default type
        assert_eq!(MemoryType::default(), MemoryType::Note);
    }

    #[test]
    fn test_transcript_chunk_excluded_from_search() {
        // Lock this behavior - transcript chunks should be excluded by default
        assert!(MemoryType::TranscriptChunk.excluded_from_default_search());
        assert!(!MemoryType::Note.excluded_from_default_search());
        assert!(!MemoryType::Todo.excluded_from_default_search());
    }
}

// ============================================================================
// CHUNKING CONFIG GOLDEN TESTS
// ============================================================================

mod chunking_golden {
    use engram::intelligence::session_indexing::ChunkingConfig;

    #[test]
    fn test_chunking_config_defaults() {
        // Lock the default configuration values
        let config = ChunkingConfig::default();

        assert_eq!(config.max_messages, 10, "Default max_messages changed");
        assert_eq!(
            config.overlap_messages, 2,
            "Default overlap_messages changed"
        );
        assert_eq!(config.max_chars, 8000, "Default max_chars changed");
        assert_eq!(
            config.default_ttl_seconds,
            7 * 24 * 60 * 60,
            "Default TTL changed"
        ); // 7 days
    }
}
