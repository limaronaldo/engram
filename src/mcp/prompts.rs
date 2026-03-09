//! MCP Prompts implementation
//!
//! Provides guided workflow prompts for common Engram operations.

use serde_json::Value;

use super::protocol::{PromptArgument, PromptContent, PromptDefinition, PromptMessage};

/// Return all available prompt definitions.
pub fn list_prompts() -> Vec<PromptDefinition> {
    vec![
        PromptDefinition {
            name: "create-knowledge-base".to_string(),
            description: Some(
                "Scan a directory and create structured memories from its contents".to_string(),
            ),
            arguments: Some(vec![
                PromptArgument {
                    name: "path".to_string(),
                    description: Some("Directory path to scan".to_string()),
                    required: Some(true),
                },
                PromptArgument {
                    name: "workspace".to_string(),
                    description: Some("Target workspace for the imported memories".to_string()),
                    required: Some(false),
                },
            ]),
        },
        PromptDefinition {
            name: "daily-review".to_string(),
            description: Some(
                "Review today's memories, quality metrics, and lifecycle status".to_string(),
            ),
            arguments: Some(vec![PromptArgument {
                name: "workspace".to_string(),
                description: Some("Workspace to review (omit for all workspaces)".to_string()),
                required: Some(false),
            }]),
        },
        PromptDefinition {
            name: "search-and-organize".to_string(),
            description: Some(
                "Search memories and get suggestions for organizing results".to_string(),
            ),
            arguments: Some(vec![
                PromptArgument {
                    name: "query".to_string(),
                    description: Some("Search query".to_string()),
                    required: Some(true),
                },
                PromptArgument {
                    name: "workspace".to_string(),
                    description: Some("Workspace to search in".to_string()),
                    required: Some(false),
                },
            ]),
        },
        PromptDefinition {
            name: "seed-entity".to_string(),
            description: Some(
                "Create a structured knowledge seed for an entity with related facts".to_string(),
            ),
            arguments: Some(vec![
                PromptArgument {
                    name: "entity_name".to_string(),
                    description: Some("Name of the entity to seed".to_string()),
                    required: Some(true),
                },
                PromptArgument {
                    name: "entity_type".to_string(),
                    description: Some(
                        "Type of entity (default: concept)".to_string(),
                    ),
                    required: Some(false),
                },
            ]),
        },
    ]
}

/// Resolve a prompt by name, substitute arguments, and return messages.
///
/// Returns `Err(String)` if the prompt name is unknown or a required argument
/// is missing.
pub fn get_prompt(name: &str, arguments: &Value) -> Result<Vec<PromptMessage>, String> {
    let arg = |key: &str| -> Option<&str> {
        arguments.get(key).and_then(|v| v.as_str())
    };

    match name {
        "create-knowledge-base" => {
            let path = arg("path")
                .ok_or_else(|| "Missing required argument: path".to_string())?;
            let workspace = arg("workspace").unwrap_or("default");

            Ok(vec![
                PromptMessage {
                    role: "user".to_string(),
                    content: PromptContent {
                        content_type: "text".to_string(),
                        text: format!(
                            "I want to create a knowledge base from the directory at {path}"
                        ),
                    },
                },
                PromptMessage {
                    role: "assistant".to_string(),
                    content: PromptContent {
                        content_type: "text".to_string(),
                        text: format!(
                            "I'll scan {path} and create memories in workspace '{workspace}'. Here's my plan:\n\
                             1. Use `memory_scan_project` to discover project files\n\
                             2. Use `memory_ingest_document` for each document\n\
                             3. Use `memory_extract_entities` to build the knowledge graph\n\
                             4. Use `memory_search` to verify the imported content"
                        ),
                    },
                },
            ])
        }

        "daily-review" => {
            let workspace_note = match arg("workspace") {
                Some(ws) => format!(" in workspace '{ws}'"),
                None => String::new(),
            };

            Ok(vec![
                PromptMessage {
                    role: "user".to_string(),
                    content: PromptContent {
                        content_type: "text".to_string(),
                        text: "Give me a daily review of my memory system".to_string(),
                    },
                },
                PromptMessage {
                    role: "assistant".to_string(),
                    content: PromptContent {
                        content_type: "text".to_string(),
                        text: format!(
                            "I'll review the current state{workspace_note}. Here's what to check:\n\
                             1. Use `memory_list` with today's date filter\n\
                             2. Use `quality_report` for quality metrics\n\
                             3. Use `lifecycle_status` for aging stats\n\
                             4. Use `salience_top` to see most important memories\n\
                             5. Use `memory_find_semantic_duplicates` to check for duplicates"
                        ),
                    },
                },
            ])
        }

        "search-and-organize" => {
            let query = arg("query")
                .ok_or_else(|| "Missing required argument: query".to_string())?;
            let workspace_note = match arg("workspace") {
                Some(ws) => format!(" in workspace '{ws}'"),
                None => String::new(),
            };

            Ok(vec![
                PromptMessage {
                    role: "user".to_string(),
                    content: PromptContent {
                        content_type: "text".to_string(),
                        text: format!(
                            "Search for '{query}' and help me organize the results{workspace_note}"
                        ),
                    },
                },
                PromptMessage {
                    role: "assistant".to_string(),
                    content: PromptContent {
                        content_type: "text".to_string(),
                        text: format!(
                            "I'll search and help organize. Steps:\n\
                             1. Use `memory_search` for '{query}'\n\
                             2. Review results for duplicates with `memory_find_semantic_duplicates`\n\
                             3. Use `memory_extract_entities` on results to find connections\n\
                             4. Use `memory_link` to connect related memories\n\
                             5. Use `memory_suggest_tags` for better categorization"
                        ),
                    },
                },
            ])
        }

        "seed-entity" => {
            let entity_name = arg("entity_name")
                .ok_or_else(|| "Missing required argument: entity_name".to_string())?;
            let entity_type = arg("entity_type").unwrap_or("concept");

            Ok(vec![
                PromptMessage {
                    role: "user".to_string(),
                    content: PromptContent {
                        content_type: "text".to_string(),
                        text: format!("I want to create a knowledge seed for {entity_name}"),
                    },
                },
                PromptMessage {
                    role: "assistant".to_string(),
                    content: PromptContent {
                        content_type: "text".to_string(),
                        text: format!(
                            "I'll help build structured knowledge about {entity_name}. Steps:\n\
                             1. Use `memory_search` to find existing mentions\n\
                             2. Create a core memory with `memory_create` (type: note, tags: [{entity_type}, {entity_name}])\n\
                             3. Use `memory_extract_entities` to register the entity\n\
                             4. Create related fact memories and link them with `memory_link`\n\
                             5. Verify with `memory_search_entities` for '{entity_name}'"
                        ),
                    },
                },
            ])
        }

        _ => Err(format!("Prompt not found: {name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_list_prompts_returns_four_prompts() {
        let prompts = list_prompts();
        assert_eq!(prompts.len(), 4);
        let names: Vec<&str> = prompts.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"create-knowledge-base"));
        assert!(names.contains(&"daily-review"));
        assert!(names.contains(&"search-and-organize"));
        assert!(names.contains(&"seed-entity"));
    }

    #[test]
    fn test_list_prompts_have_descriptions() {
        for prompt in list_prompts() {
            assert!(
                prompt.description.is_some(),
                "Prompt '{}' missing description",
                prompt.name
            );
        }
    }

    #[test]
    fn test_create_knowledge_base_with_required_args() {
        let args = json!({"path": "/home/user/docs", "workspace": "kb"});
        let messages = get_prompt("create-knowledge-base", &args).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert!(messages[0].content.text.contains("/home/user/docs"));
        assert_eq!(messages[1].role, "assistant");
        assert!(messages[1].content.text.contains("/home/user/docs"));
        assert!(messages[1].content.text.contains("kb"));
    }

    #[test]
    fn test_create_knowledge_base_missing_path_returns_error() {
        let args = json!({});
        let result = get_prompt("create-knowledge-base", &args);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("path"));
    }

    #[test]
    fn test_create_knowledge_base_default_workspace() {
        let args = json!({"path": "/tmp/data"});
        let messages = get_prompt("create-knowledge-base", &args).unwrap();
        assert!(messages[1].content.text.contains("default"));
    }

    #[test]
    fn test_daily_review_no_workspace() {
        let args = json!({});
        let messages = get_prompt("daily-review", &args).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content.text, "Give me a daily review of my memory system");
    }

    #[test]
    fn test_daily_review_with_workspace() {
        let args = json!({"workspace": "notes"});
        let messages = get_prompt("daily-review", &args).unwrap();
        assert!(messages[1].content.text.contains("notes"));
    }

    #[test]
    fn test_search_and_organize_with_query() {
        let args = json!({"query": "machine learning", "workspace": "research"});
        let messages = get_prompt("search-and-organize", &args).unwrap();
        assert_eq!(messages.len(), 2);
        assert!(messages[0].content.text.contains("machine learning"));
        assert!(messages[1].content.text.contains("machine learning"));
    }

    #[test]
    fn test_search_and_organize_missing_query_returns_error() {
        let args = json!({});
        let result = get_prompt("search-and-organize", &args);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("query"));
    }

    #[test]
    fn test_seed_entity_with_all_args() {
        let args = json!({"entity_name": "Rust", "entity_type": "language"});
        let messages = get_prompt("seed-entity", &args).unwrap();
        assert_eq!(messages.len(), 2);
        assert!(messages[0].content.text.contains("Rust"));
        assert!(messages[1].content.text.contains("Rust"));
        assert!(messages[1].content.text.contains("language"));
    }

    #[test]
    fn test_seed_entity_default_entity_type() {
        let args = json!({"entity_name": "GPT-4"});
        let messages = get_prompt("seed-entity", &args).unwrap();
        assert!(messages[1].content.text.contains("concept"));
    }

    #[test]
    fn test_seed_entity_missing_entity_name_returns_error() {
        let args = json!({});
        let result = get_prompt("seed-entity", &args);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("entity_name"));
    }

    #[test]
    fn test_unknown_prompt_returns_error() {
        let args = json!({});
        let result = get_prompt("nonexistent-prompt", &args);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn test_prompt_content_type_is_text() {
        let args = json!({"path": "/tmp"});
        let messages = get_prompt("create-knowledge-base", &args).unwrap();
        for msg in &messages {
            assert_eq!(msg.content.content_type, "text");
        }
    }
}
