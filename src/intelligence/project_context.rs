//! Project Context Discovery & Ingestion
//!
//! Auto-discovers and ingests AI instruction files from the market ecosystem:
//! - CLAUDE.md (Claude Code)
//! - AGENTS.md (Various AI agents)
//! - .cursorrules (Cursor IDE)
//! - .github/copilot-instructions.md (GitHub Copilot)
//! - .aider.conf.yml (Aider)
//! - GEMINI.md (Gemini tools)
//! - .windsurfrules (Windsurf IDE)
//! - CONVENTIONS.md (General)
//!
//! Creates a universal AI project context layer with:
//! - Parent memory for each file (type: Context)
//! - Child memories for each section (linked via cross-references)
//! - Idempotent updates based on content hashing
//! - Search boost for current project context

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::error::Result;
use crate::types::{Memory, MemoryScope, MemoryType, Visibility};

// =============================================================================
// Configuration
// =============================================================================

/// Configuration for project context discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectContextConfig {
    /// Enable project context discovery
    pub enabled: bool,
    /// Maximum file size in bytes (skip larger files)
    pub max_file_size: u64,
    /// Extract sections as child memories
    pub extract_sections: bool,
    /// Scan parent directories (security: false by default)
    pub scan_parents: bool,
    /// Directories to ignore during scan
    pub ignore_dirs: Vec<String>,
    /// File patterns to ignore (glob-style)
    pub ignore_files: Vec<String>,
    /// Default visibility for created memories
    pub default_visibility: Visibility,
    /// Search boost factor for project context (0.0 - 1.0)
    pub search_boost: f32,
}

impl Default for ProjectContextConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_file_size: 1024 * 1024, // 1MB
            extract_sections: true,
            scan_parents: false,
            ignore_dirs: vec![
                ".git".to_string(),
                "target".to_string(),
                "node_modules".to_string(),
                "vendor".to_string(),
                ".venv".to_string(),
                "__pycache__".to_string(),
                "dist".to_string(),
                "build".to_string(),
            ],
            ignore_files: vec![
                ".env*".to_string(),
                "*.key".to_string(),
                "*.pem".to_string(),
                "*.p12".to_string(),
                "secrets/*".to_string(),
            ],
            default_visibility: Visibility::Private,
            search_boost: 0.2,
        }
    }
}

// =============================================================================
// Core Instruction Files (Phase 1)
// =============================================================================

/// Known instruction file patterns
pub const CORE_INSTRUCTION_FILES: &[&str] = &[
    "CLAUDE.md",
    "AGENTS.md",
    ".cursorrules",
    ".github/copilot-instructions.md",
    ".aider.conf.yml",
    "GEMINI.md",
    ".windsurfrules",
    "CONVENTIONS.md",
    "CODING_GUIDELINES.md",
];

// =============================================================================
// Types
// =============================================================================

/// Type of instruction file
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstructionFileType {
    ClaudeMd,
    AgentsMd,
    CursorRules,
    CopilotInstructions,
    GeminiMd,
    AiderConf,
    ConventionsMd,
    WindsurfRules,
    CodingGuidelines,
    Custom,
}

impl InstructionFileType {
    /// Detect file type from filename
    pub fn from_filename(filename: &str) -> Self {
        match filename.to_lowercase().as_str() {
            "claude.md" => Self::ClaudeMd,
            "agents.md" => Self::AgentsMd,
            ".cursorrules" => Self::CursorRules,
            "copilot-instructions.md" => Self::CopilotInstructions,
            "gemini.md" => Self::GeminiMd,
            ".aider.conf.yml" => Self::AiderConf,
            "conventions.md" => Self::ConventionsMd,
            ".windsurfrules" => Self::WindsurfRules,
            "coding_guidelines.md" | "coding-guidelines.md" => Self::CodingGuidelines,
            _ => Self::Custom,
        }
    }

    /// Get tag name for this file type
    pub fn as_tag(&self) -> &'static str {
        match self {
            Self::ClaudeMd => "claude-md",
            Self::AgentsMd => "agents-md",
            Self::CursorRules => "cursorrules",
            Self::CopilotInstructions => "copilot-instructions",
            Self::GeminiMd => "gemini-md",
            Self::AiderConf => "aider-conf",
            Self::ConventionsMd => "conventions-md",
            Self::WindsurfRules => "windsurfrules",
            Self::CodingGuidelines => "coding-guidelines",
            Self::Custom => "custom-instructions",
        }
    }
}

/// File format for parsing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileFormat {
    Markdown,
    Yaml,
    PlainText,
}

impl FileFormat {
    /// Detect format from filename
    pub fn from_filename(filename: &str) -> Self {
        let lower = filename.to_lowercase();
        if lower.ends_with(".md") {
            Self::Markdown
        } else if lower.ends_with(".yml") || lower.ends_with(".yaml") {
            Self::Yaml
        } else {
            Self::PlainText
        }
    }
}

/// A discovered instruction file
#[derive(Debug, Clone)]
pub struct DiscoveredFile {
    /// Full path to the file
    pub path: PathBuf,
    /// Filename only
    pub filename: String,
    /// File size in bytes
    pub size: u64,
    /// File content
    pub content: String,
    /// File type
    pub file_type: InstructionFileType,
    /// File format (for parsing)
    pub format: FileFormat,
    /// SHA-256 hash of content
    pub content_hash: String,
    /// Last modified time
    pub mtime: SystemTime,
    /// Project path (directory containing the file)
    pub project_path: PathBuf,
}

/// Parsed instructions from a file
#[derive(Debug, Clone)]
pub struct ParsedInstructions {
    /// Parsed sections
    pub sections: Vec<ParsedSection>,
    /// Raw file content
    pub raw_content: String,
    /// Content hash
    pub file_hash: String,
}

/// A parsed section from an instruction file
#[derive(Debug, Clone)]
pub struct ParsedSection {
    /// Section title (heading text)
    pub title: String,
    /// Section content (without heading)
    pub content: String,
    /// Full section path (e.g., "Guidelines > Testing > Unit")
    pub section_path: String,
    /// Index in file (0-based)
    pub section_index: usize,
    /// Heading level (1-6 for markdown)
    pub heading_level: usize,
    /// URL-safe anchor (e.g., "unit-testing")
    pub heading_anchor: String,
    /// SHA-256 hash of section content
    pub content_hash: String,
}

/// Result of a project scan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    /// Project path that was scanned
    pub project_path: String,
    /// Number of files discovered
    pub files_found: usize,
    /// Number of memories created
    pub memories_created: usize,
    /// Number of memories updated
    pub memories_updated: usize,
    /// Number of files skipped (too large, etc.)
    pub files_skipped: usize,
    /// Errors encountered (non-fatal)
    pub errors: Vec<String>,
    /// Timestamp of scan
    pub scanned_at: DateTime<Utc>,
}

// =============================================================================
// Parsers
// =============================================================================

/// Trait for parsing instruction files
pub trait InstructionFileParser: Send + Sync {
    /// Parse file content into sections
    fn parse(&self, content: &str) -> Result<ParsedInstructions>;
}

/// Markdown parser - extracts sections by headings
pub struct MarkdownParser;

impl InstructionFileParser for MarkdownParser {
    fn parse(&self, content: &str) -> Result<ParsedInstructions> {
        let file_hash = hash_content(content);
        let mut sections = Vec::new();
        let mut current_section: Option<(String, String, usize, Vec<String>)> = None;
        let mut section_index = 0;
        let mut heading_stack: Vec<(usize, String)> = Vec::new();

        for line in content.lines() {
            if let Some((level, title)) = parse_markdown_heading(line) {
                // Save previous section if exists
                if let Some((title, content, level, path_parts)) = current_section.take() {
                    if !content.trim().is_empty() {
                        let section_path = path_parts.join(" > ");
                        sections.push(ParsedSection {
                            title: title.clone(),
                            content: content.trim().to_string(),
                            section_path,
                            section_index,
                            heading_level: level,
                            heading_anchor: slugify(&title),
                            content_hash: hash_content(&content),
                        });
                        section_index += 1;
                    }
                }

                // Update heading stack
                while heading_stack
                    .last()
                    .map(|(l, _)| *l >= level)
                    .unwrap_or(false)
                {
                    heading_stack.pop();
                }
                heading_stack.push((level, title.clone()));

                // Build path from stack
                let path_parts: Vec<String> =
                    heading_stack.iter().map(|(_, t)| t.clone()).collect();

                current_section = Some((title, String::new(), level, path_parts));
            } else if let Some((_, ref mut content, _, _)) = current_section {
                content.push_str(line);
                content.push('\n');
            }
        }

        // Don't forget last section
        if let Some((title, content, level, path_parts)) = current_section {
            if !content.trim().is_empty() {
                let section_path = path_parts.join(" > ");
                sections.push(ParsedSection {
                    title: title.clone(),
                    content: content.trim().to_string(),
                    section_path,
                    section_index,
                    heading_level: level,
                    heading_anchor: slugify(&title),
                    content_hash: hash_content(&content),
                });
            }
        }

        Ok(ParsedInstructions {
            sections,
            raw_content: content.to_string(),
            file_hash,
        })
    }
}

/// YAML parser - treats whole file as single section, extracts keys as metadata
pub struct YamlParser;

impl InstructionFileParser for YamlParser {
    fn parse(&self, content: &str) -> Result<ParsedInstructions> {
        let file_hash = hash_content(content);

        // For YAML, we create a single section with the whole content
        // Future: parse YAML structure and create sections per top-level key
        let sections = vec![ParsedSection {
            title: "Configuration".to_string(),
            content: content.to_string(),
            section_path: "Configuration".to_string(),
            section_index: 0,
            heading_level: 1,
            heading_anchor: "configuration".to_string(),
            content_hash: file_hash.clone(),
        }];

        Ok(ParsedInstructions {
            sections,
            raw_content: content.to_string(),
            file_hash,
        })
    }
}

/// Plain text parser - treats whole file as single section
pub struct PlainTextParser;

impl InstructionFileParser for PlainTextParser {
    fn parse(&self, content: &str) -> Result<ParsedInstructions> {
        let file_hash = hash_content(content);

        let sections = vec![ParsedSection {
            title: "Instructions".to_string(),
            content: content.to_string(),
            section_path: "Instructions".to_string(),
            section_index: 0,
            heading_level: 1,
            heading_anchor: "instructions".to_string(),
            content_hash: file_hash.clone(),
        }];

        Ok(ParsedInstructions {
            sections,
            raw_content: content.to_string(),
            file_hash,
        })
    }
}

// =============================================================================
// Project Context Engine
// =============================================================================

/// Engine for discovering and ingesting project context
pub struct ProjectContextEngine {
    config: ProjectContextConfig,
    markdown_parser: MarkdownParser,
    yaml_parser: YamlParser,
    plaintext_parser: PlainTextParser,
}

impl ProjectContextEngine {
    /// Create a new engine with default config
    pub fn new() -> Self {
        Self::with_config(ProjectContextConfig::default())
    }

    /// Create a new engine with custom config
    pub fn with_config(config: ProjectContextConfig) -> Self {
        Self {
            config,
            markdown_parser: MarkdownParser,
            yaml_parser: YamlParser,
            plaintext_parser: PlainTextParser,
        }
    }

    /// Get the parser for a file format
    fn get_parser(&self, format: FileFormat) -> &dyn InstructionFileParser {
        match format {
            FileFormat::Markdown => &self.markdown_parser,
            FileFormat::Yaml => &self.yaml_parser,
            FileFormat::PlainText => &self.plaintext_parser,
        }
    }

    /// Scan a directory for instruction files
    /// Scan a directory for instruction files
    /// Returns (discovered_files, skipped_count)
    pub fn scan_directory(&self, path: &Path) -> Result<Vec<DiscoveredFile>> {
        let (files, _skipped) = self.scan_directory_with_stats(path)?;
        Ok(files)
    }

    /// Scan a directory for instruction files with statistics
    /// Returns (discovered_files, skipped_count)
    pub fn scan_directory_with_stats(&self, path: &Path) -> Result<(Vec<DiscoveredFile>, usize)> {
        if !self.config.enabled {
            return Ok((Vec::new(), 0));
        }

        let mut discovered = Vec::new();
        let mut skipped = 0;
        let project_path = path.to_path_buf();

        // Scan for each known instruction file
        for pattern in CORE_INSTRUCTION_FILES {
            let file_path = path.join(pattern);
            if file_path.exists() && file_path.is_file() {
                match self.read_file(&file_path, &project_path) {
                    Ok(Some(file)) => discovered.push(file),
                    Ok(None) => skipped += 1, // Skipped (too large, etc.)
                    Err(e) => {
                        tracing::warn!("Error reading {}: {}", file_path.display(), e);
                    }
                }
            }
        }

        // Optionally scan parent directories
        if self.config.scan_parents {
            if let Some(parent) = path.parent() {
                if parent != path {
                    let (parent_files, parent_skipped) = self.scan_directory_with_stats(parent)?;
                    discovered.extend(parent_files);
                    skipped += parent_skipped;
                }
            }
        }

        Ok((discovered, skipped))
    }

    /// Read and validate a single file
    fn read_file(&self, path: &Path, project_path: &Path) -> Result<Option<DiscoveredFile>> {
        let metadata = fs::metadata(path)?;
        let size = metadata.len();

        // Skip if too large
        if size > self.config.max_file_size {
            tracing::info!(
                "Skipping {} (size {} > max {})",
                path.display(),
                size,
                self.config.max_file_size
            );
            return Ok(None);
        }

        let content = fs::read_to_string(path)?;
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let file_type = InstructionFileType::from_filename(&filename);
        let format = FileFormat::from_filename(&filename);
        let content_hash = hash_content(&content);
        let mtime = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);

        Ok(Some(DiscoveredFile {
            path: path.to_path_buf(),
            filename,
            size,
            content,
            file_type,
            format,
            content_hash,
            mtime,
            project_path: project_path.to_path_buf(),
        }))
    }

    /// Parse a discovered file into instructions
    pub fn parse_file(&self, file: &DiscoveredFile) -> Result<ParsedInstructions> {
        let parser = self.get_parser(file.format);
        parser.parse(&file.content)
    }

    /// Convert a discovered file to a parent memory
    pub fn file_to_memory(&self, file: &DiscoveredFile) -> Memory {
        let mut metadata = HashMap::new();
        metadata.insert(
            "source_file".to_string(),
            serde_json::Value::String(file.path.to_string_lossy().to_string()),
        );
        metadata.insert(
            "file_type".to_string(),
            serde_json::Value::String(file.file_type.as_tag().to_string()),
        );
        metadata.insert(
            "project_path".to_string(),
            serde_json::Value::String(file.project_path.to_string_lossy().to_string()),
        );
        metadata.insert(
            "file_hash".to_string(),
            serde_json::Value::String(file.content_hash.clone()),
        );
        // Convert SystemTime to RFC3339 format
        let mtime_rfc3339 = file
            .mtime
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| DateTime::<Utc>::from(std::time::UNIX_EPOCH + d).to_rfc3339())
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string());
        metadata.insert(
            "file_mtime".to_string(),
            serde_json::Value::String(mtime_rfc3339),
        );

        Memory {
            id: 0,
            content: file.content.clone(),
            memory_type: MemoryType::Context,
            tags: vec![
                "project-context".to_string(),
                file.file_type.as_tag().to_string(),
            ],
            metadata,
            importance: 0.8, // High importance for project context
            access_count: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_accessed_at: None,
            owner_id: None,
            visibility: self.config.default_visibility,
            scope: MemoryScope::Global,
            version: 1,
            has_embedding: false,
            expires_at: None,
            content_hash: None, // Will be computed on storage
        }
    }

    /// Convert a parsed section to a child memory
    pub fn section_to_memory(
        &self,
        section: &ParsedSection,
        file: &DiscoveredFile,
        parent_id: i64,
    ) -> Memory {
        let mut metadata = HashMap::new();
        metadata.insert(
            "source_file".to_string(),
            serde_json::Value::String(file.path.to_string_lossy().to_string()),
        );
        metadata.insert(
            "file_type".to_string(),
            serde_json::Value::String(file.file_type.as_tag().to_string()),
        );
        metadata.insert(
            "project_path".to_string(),
            serde_json::Value::String(file.project_path.to_string_lossy().to_string()),
        );
        metadata.insert(
            "section_path".to_string(),
            serde_json::Value::String(section.section_path.clone()),
        );
        metadata.insert(
            "section_index".to_string(),
            serde_json::json!(section.section_index),
        );
        metadata.insert(
            "content_hash".to_string(),
            serde_json::Value::String(section.content_hash.clone()),
        );
        metadata.insert(
            "heading_anchor".to_string(),
            serde_json::Value::String(section.heading_anchor.clone()),
        );
        metadata.insert("parent_memory_id".to_string(), serde_json::json!(parent_id));

        // Create content with section title
        let content = format!("# {}\n\n{}", section.title, section.content);

        Memory {
            id: 0,
            content,
            memory_type: MemoryType::Context,
            tags: vec![
                "project-context".to_string(),
                "section".to_string(),
                file.file_type.as_tag().to_string(),
            ],
            metadata,
            importance: 0.7,
            access_count: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_accessed_at: None,
            owner_id: None,
            visibility: self.config.default_visibility,
            scope: MemoryScope::Global,
            version: 1,
            has_embedding: false,
            expires_at: None,
            content_hash: None, // Will be computed on storage
        }
    }

    /// Get config reference
    pub fn config(&self) -> &ProjectContextConfig {
        &self.config
    }
}

impl Default for ProjectContextEngine {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Compute SHA-256 hash of content
fn hash_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

/// Parse a markdown heading line
fn parse_markdown_heading(line: &str) -> Option<(usize, String)> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('#') {
        return None;
    }

    let level = trimmed.chars().take_while(|&c| c == '#').count();
    if level == 0 || level > 6 {
        return None;
    }

    let title = trimmed[level..].trim().to_string();
    if title.is_empty() {
        return None;
    }

    Some((level, title))
}

/// Convert title to URL-safe slug
fn slugify(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instruction_file_type_detection() {
        assert_eq!(
            InstructionFileType::from_filename("CLAUDE.md"),
            InstructionFileType::ClaudeMd
        );
        assert_eq!(
            InstructionFileType::from_filename(".cursorrules"),
            InstructionFileType::CursorRules
        );
        assert_eq!(
            InstructionFileType::from_filename(".aider.conf.yml"),
            InstructionFileType::AiderConf
        );
        assert_eq!(
            InstructionFileType::from_filename("random.txt"),
            InstructionFileType::Custom
        );
    }

    #[test]
    fn test_file_format_detection() {
        assert_eq!(FileFormat::from_filename("CLAUDE.md"), FileFormat::Markdown);
        assert_eq!(
            FileFormat::from_filename(".aider.conf.yml"),
            FileFormat::Yaml
        );
        assert_eq!(
            FileFormat::from_filename(".cursorrules"),
            FileFormat::PlainText
        );
    }

    #[test]
    fn test_markdown_heading_parsing() {
        assert_eq!(
            parse_markdown_heading("# Title"),
            Some((1, "Title".to_string()))
        );
        assert_eq!(
            parse_markdown_heading("## Subtitle"),
            Some((2, "Subtitle".to_string()))
        );
        assert_eq!(
            parse_markdown_heading("### Deep Heading"),
            Some((3, "Deep Heading".to_string()))
        );
        assert_eq!(parse_markdown_heading("Not a heading"), None);
        assert_eq!(parse_markdown_heading("#"), None); // Empty title
    }

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("Hello World"), "hello-world");
        assert_eq!(slugify("Unit Testing"), "unit-testing");
        assert_eq!(slugify("API & REST"), "api-rest");
        assert_eq!(slugify("  Multiple   Spaces  "), "multiple-spaces");
    }

    #[test]
    fn test_hash_content() {
        let hash1 = hash_content("hello");
        let hash2 = hash_content("hello");
        let hash3 = hash_content("world");

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
        assert!(hash1.starts_with("sha256:"));
    }

    #[test]
    fn test_markdown_parser() {
        let content = r#"# Main Title

Some intro text.

## Section One

Content of section one.

## Section Two

Content of section two.

### Subsection

Nested content.
"#;

        let parser = MarkdownParser;
        let result = parser.parse(content).unwrap();

        assert_eq!(result.sections.len(), 4);
        assert_eq!(result.sections[0].title, "Main Title");
        assert_eq!(result.sections[0].section_path, "Main Title");
        assert_eq!(result.sections[1].title, "Section One");
        assert_eq!(result.sections[1].section_path, "Main Title > Section One");
        assert_eq!(result.sections[2].title, "Section Two");
        assert_eq!(result.sections[3].title, "Subsection");
        assert_eq!(
            result.sections[3].section_path,
            "Main Title > Section Two > Subsection"
        );
    }

    #[test]
    fn test_yaml_parser() {
        let content = "key: value\nother: data";
        let parser = YamlParser;
        let result = parser.parse(content).unwrap();

        assert_eq!(result.sections.len(), 1);
        assert_eq!(result.sections[0].title, "Configuration");
    }

    #[test]
    fn test_plaintext_parser() {
        let content = "Some plain text instructions";
        let parser = PlainTextParser;
        let result = parser.parse(content).unwrap();

        assert_eq!(result.sections.len(), 1);
        assert_eq!(result.sections[0].title, "Instructions");
    }

    #[test]
    fn test_engine_default_config() {
        let engine = ProjectContextEngine::new();
        assert!(engine.config().enabled);
        assert_eq!(engine.config().max_file_size, 1024 * 1024);
        assert!(!engine.config().scan_parents);
    }

    #[test]
    fn test_file_to_memory() {
        let engine = ProjectContextEngine::new();
        let file = DiscoveredFile {
            path: PathBuf::from("/project/CLAUDE.md"),
            filename: "CLAUDE.md".to_string(),
            size: 100,
            content: "# Test\n\nContent".to_string(),
            file_type: InstructionFileType::ClaudeMd,
            format: FileFormat::Markdown,
            content_hash: "sha256:abc123".to_string(),
            mtime: SystemTime::UNIX_EPOCH,
            project_path: PathBuf::from("/project"),
        };

        let memory = engine.file_to_memory(&file);

        assert_eq!(memory.memory_type, MemoryType::Context);
        assert!(memory.tags.contains(&"project-context".to_string()));
        assert!(memory.tags.contains(&"claude-md".to_string()));
        assert_eq!(memory.importance, 0.8);
    }

    #[test]
    fn test_section_to_memory() {
        let engine = ProjectContextEngine::new();
        let file = DiscoveredFile {
            path: PathBuf::from("/project/CLAUDE.md"),
            filename: "CLAUDE.md".to_string(),
            size: 100,
            content: "# Test".to_string(),
            file_type: InstructionFileType::ClaudeMd,
            format: FileFormat::Markdown,
            content_hash: "sha256:abc".to_string(),
            mtime: SystemTime::UNIX_EPOCH,
            project_path: PathBuf::from("/project"),
        };

        let section = ParsedSection {
            title: "Guidelines".to_string(),
            content: "Follow these rules".to_string(),
            section_path: "Main > Guidelines".to_string(),
            section_index: 1,
            heading_level: 2,
            heading_anchor: "guidelines".to_string(),
            content_hash: "sha256:def".to_string(),
        };

        let memory = engine.section_to_memory(&section, &file, 123);

        assert!(memory.content.contains("# Guidelines"));
        assert!(memory.tags.contains(&"section".to_string()));
        assert_eq!(
            memory.metadata.get("parent_memory_id"),
            Some(&serde_json::Value::Number(123.into()))
        );
    }
}
