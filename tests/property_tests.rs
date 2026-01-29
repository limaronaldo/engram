//! Property-based tests for engram
//!
//! These tests verify invariants that must hold for all inputs:
//! - Normalization is idempotent
//! - Parsers never panic
//! - Bounded operations stay bounded
//!
//! Run with: cargo test --test property_tests

use proptest::prelude::*;

// ============================================================================
// WORKSPACE NORMALIZATION TESTS
// ============================================================================

mod workspace_tests {
    use super::*;
    use engram::types::{normalize_workspace, WorkspaceError, MAX_WORKSPACE_LENGTH};

    proptest! {
        /// Invariant: normalize_workspace never panics on any string input
        #[test]
        fn never_panics(s in ".*") {
            let _ = normalize_workspace(&s);
        }

        /// Invariant: If normalization succeeds, applying it again yields the same result
        #[test]
        fn idempotent_when_valid(s in "[a-z0-9_-]{1,64}") {
            if let Ok(normalized) = normalize_workspace(&s) {
                let twice = normalize_workspace(&normalized);
                prop_assert_eq!(Ok(normalized.clone()), twice);
            }
        }

        /// Invariant: Normalized result only contains allowed characters
        #[test]
        fn output_charset(s in "\\PC{1,100}") {
            if let Ok(normalized) = normalize_workspace(&s) {
                prop_assert!(normalized.chars().all(|c|
                    c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_'
                ));
            }
        }

        /// Invariant: Normalized result respects max length
        #[test]
        fn respects_max_length(s in "\\PC{1,200}") {
            if let Ok(normalized) = normalize_workspace(&s) {
                prop_assert!(normalized.len() <= MAX_WORKSPACE_LENGTH);
            }
        }

        /// Invariant: Empty input always fails
        #[test]
        fn empty_fails(s in "\\s*") {
            let result = normalize_workspace(&s);
            if s.trim().is_empty() {
                prop_assert_eq!(result, Err(WorkspaceError::Empty));
            }
        }

        /// Invariant: Reserved names are rejected
        #[test]
        fn reserved_rejected(prefix in "_{1,5}", suffix in "[a-z0-9]{0,10}") {
            let input = format!("{}{}", prefix, suffix);
            let result = normalize_workspace(&input);
            prop_assert!(result.is_err());
        }
    }
}

// ============================================================================
// ALIAS NORMALIZATION TESTS
// ============================================================================

mod alias_tests {
    use super::*;
    use engram::storage::identity_links::normalize_alias;

    proptest! {
        /// Invariant: normalize_alias never panics on any input
        #[test]
        fn never_panics(s in ".*") {
            let _ = normalize_alias(&s);
        }

        /// Invariant: Normalization is idempotent
        #[test]
        fn idempotent(s in "\\PC{0,100}") {
            let once = normalize_alias(&s);
            let twice = normalize_alias(&once);
            prop_assert_eq!(once, twice);
        }

        /// Invariant: Output is lowercase
        #[test]
        fn lowercase_output(s in "\\PC{1,50}") {
            let normalized = normalize_alias(&s);
            prop_assert!(normalized.chars().all(|c| !c.is_ascii_uppercase()));
        }

        /// Invariant: No leading/trailing whitespace
        #[test]
        fn no_boundary_whitespace(s in "\\PC{1,50}") {
            let normalized = normalize_alias(&s);
            let trimmed = normalized.trim().to_string();
            prop_assert_eq!(normalized, trimmed);
        }

        /// Invariant: No multiple consecutive spaces
        #[test]
        fn no_multiple_spaces(s in "\\PC{1,50}") {
            let normalized = normalize_alias(&s);
            prop_assert!(!normalized.contains("  "));
        }
    }
}

// ============================================================================
// ENTITY EXTRACTION TESTS
// ============================================================================

mod extraction_tests {
    use super::*;
    use engram::intelligence::entity_extraction::{extract_entities, ExtractionConfig};

    proptest! {
        /// Invariant: Extraction never panics on any input
        #[test]
        fn never_panics(s in "\\PC{0,1000}") {
            let config = ExtractionConfig {
                lookup_aliases: false,
                ..Default::default()
            };
            let _ = extract_entities(&s, &config, None);
        }

        /// Invariant: Result count is bounded
        #[test]
        fn bounded_results(s in "\\PC{0,500}", max in 1usize..50) {
            let config = ExtractionConfig {
                lookup_aliases: false,
                max_entities: max,
                ..Default::default()
            };
            let result = extract_entities(&s, &config, None);
            prop_assert!(result.entities.len() <= max);
        }

        /// Invariant: Empty input yields empty results
        #[test]
        fn empty_input_empty_result(s in "\\s*") {
            let config = ExtractionConfig {
                lookup_aliases: false,
                ..Default::default()
            };
            let result = extract_entities(&s, &config, None);
            prop_assert!(result.entities.is_empty());
        }

        /// Invariant: Each entity has non-empty mention text
        #[test]
        fn entities_have_text(s in "@[a-z]{1,10}( @[a-z]{1,10})*") {
            let config = ExtractionConfig {
                lookup_aliases: false,
                ..Default::default()
            };
            let result = extract_entities(&s, &config, None);
            for entity in &result.entities {
                prop_assert!(!entity.mention_text.is_empty());
            }
        }
    }
}

// ============================================================================
// MEMORY TIER TESTS
// ============================================================================

mod tier_tests {
    use super::*;
    use engram::types::MemoryTier;

    proptest! {
        /// Invariant: MemoryTier round-trips through string
        #[test]
        fn tier_roundtrip(tier in prop_oneof![Just(MemoryTier::Permanent), Just(MemoryTier::Daily)]) {
            let s = tier.as_str();
            let parsed: MemoryTier = s.parse().unwrap();
            prop_assert_eq!(tier, parsed);
        }

        /// Invariant: Unknown tier strings fail parsing
        #[test]
        fn unknown_tier_fails(s in "[a-z]{5,20}") {
            if s != "permanent" && s != "daily" {
                let result: Result<MemoryTier, _> = s.parse();
                prop_assert!(result.is_err());
            }
        }
    }
}

// ============================================================================
// SESSION CHUNKING TESTS
// ============================================================================

mod chunking_tests {
    use super::*;
    use chrono::Utc;
    use engram::intelligence::session_indexing::{chunk_conversation, ChunkingConfig, Message};

    fn make_messages(count: usize, content_len: usize) -> Vec<Message> {
        (0..count)
            .map(|i| Message {
                role: if i % 2 == 0 {
                    "user".to_string()
                } else {
                    "assistant".to_string()
                },
                content: "x".repeat(content_len),
                timestamp: Utc::now(),
                id: None,
            })
            .collect()
    }

    proptest! {
        /// Invariant: Chunking never panics
        #[test]
        fn never_panics(msg_count in 0usize..100, content_len in 1usize..500) {
            let messages = make_messages(msg_count, content_len);
            let config = ChunkingConfig::default();
            let _ = chunk_conversation(&messages, &config);
        }

        /// Invariant: Each chunk has at most max_messages
        #[test]
        fn respects_max_messages(msg_count in 1usize..50, max_msgs in 1usize..20) {
            let messages = make_messages(msg_count, 100);
            let config = ChunkingConfig {
                max_messages: max_msgs,
                ..Default::default()
            };
            let chunks = chunk_conversation(&messages, &config);
            for chunk in &chunks {
                prop_assert!(chunk.messages.len() <= max_msgs);
            }
        }

        /// Invariant: Empty input yields empty chunks
        #[test]
        fn empty_input_empty_chunks(_unused: u8) {
            let messages: Vec<Message> = vec![];
            let config = ChunkingConfig::default();
            let chunks = chunk_conversation(&messages, &config);
            prop_assert!(chunks.is_empty());
        }
    }
}

// ============================================================================
// MEMORY TYPE TESTS
// ============================================================================

mod memory_type_tests {
    use super::*;
    use engram::types::MemoryType;

    proptest! {
        /// Invariant: All memory types round-trip
        #[test]
        fn roundtrip(memory_type in prop_oneof![
            Just(MemoryType::Note),
            Just(MemoryType::Todo),
            Just(MemoryType::Issue),
            Just(MemoryType::Decision),
            Just(MemoryType::Preference),
            Just(MemoryType::Learning),
            Just(MemoryType::Context),
            Just(MemoryType::Credential),
            Just(MemoryType::Custom),
            Just(MemoryType::TranscriptChunk),
        ]) {
            let s = memory_type.as_str();
            let parsed: MemoryType = s.parse().unwrap();
            prop_assert_eq!(memory_type, parsed);
        }
    }
}

// ============================================================================
// EDGE TYPE TESTS
// ============================================================================

mod edge_type_tests {
    use super::*;
    use engram::types::EdgeType;

    proptest! {
        /// Invariant: All edge types round-trip
        #[test]
        fn roundtrip(edge_type in prop_oneof![
            Just(EdgeType::RelatedTo),
            Just(EdgeType::DependsOn),
            Just(EdgeType::References),
            Just(EdgeType::Blocks),
            Just(EdgeType::FollowsUp),
            Just(EdgeType::Supersedes),
            Just(EdgeType::Contradicts),
            Just(EdgeType::Implements),
            Just(EdgeType::Extends),
        ]) {
            let s = edge_type.as_str();
            let parsed: EdgeType = s.parse().unwrap();
            prop_assert_eq!(edge_type, parsed);
        }
    }
}
