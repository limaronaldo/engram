# Contributing to Engram

Thank you for your interest in contributing to Engram! This document provides guidelines and instructions for contributing.

## Code of Conduct

By participating in this project, you agree to maintain a respectful and inclusive environment for everyone.

## How to Contribute

### Reporting Bugs

Before submitting a bug report:
1. Check existing issues to avoid duplicates
2. Use the bug report template
3. Include as much detail as possible:
   - Engram version (`engram-cli --version`)
   - Operating system and version
   - Steps to reproduce
   - Expected vs actual behavior
   - Relevant logs or error messages

### Suggesting Features

1. Check existing issues and discussions for similar ideas
2. Use the feature request template
3. Clearly describe the use case and benefits
4. Consider implementation complexity

### Pull Requests

#### Before Starting

1. Open an issue to discuss significant changes
2. Fork the repository
3. Create a feature branch from `main`

#### Development Setup

```bash
# Clone your fork
git clone https://github.com/YOUR_USERNAME/engram.git
cd engram

# Add upstream remote
git remote add upstream https://github.com/limaronaldo/engram.git

# Install Rust (if needed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Build the project
cargo build

# Run tests
cargo test
```

#### Making Changes

1. Create a feature branch:
   ```bash
   git checkout -b feature/your-feature-name
   ```

2. Make your changes following our coding standards

3. Add tests for new functionality

4. Run the test suite:
   ```bash
   cargo test
   cargo clippy
   cargo fmt --check
   ```

5. Commit with clear messages:
   ```bash
   git commit -m "feat: add support for custom edge types"
   ```

#### Commit Message Format

We follow [Conventional Commits](https://www.conventionalcommits.org/):

- `feat:` - New feature
- `fix:` - Bug fix
- `docs:` - Documentation changes
- `style:` - Code style changes (formatting, etc.)
- `refactor:` - Code refactoring
- `perf:` - Performance improvements
- `test:` - Adding or updating tests
- `chore:` - Maintenance tasks

Examples:
```
feat: add WebSocket support for real-time updates
fix: resolve memory leak in embedding queue
docs: update installation instructions for Windows
perf: optimize BM25 scoring algorithm
```

#### Submitting

1. Push your branch:
   ```bash
   git push origin feature/your-feature-name
   ```

2. Open a Pull Request against `main`

3. Fill out the PR template completely

4. Wait for review and address feedback

## Coding Standards

### Rust Style

- Follow the official [Rust Style Guide](https://doc.rust-lang.org/nightly/style-guide/)
- Use `cargo fmt` for formatting
- Use `cargo clippy` for linting
- Document public APIs with doc comments

### Code Organization

```rust
// 1. Module documentation
//! Brief description of the module

// 2. Imports (grouped and sorted)
use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::types::Memory;

// 3. Constants
const MAX_CONTENT_LENGTH: usize = 65536;

// 4. Type definitions
pub struct MyStruct { ... }

// 5. Implementations
impl MyStruct { ... }

// 6. Tests at the bottom
#[cfg(test)]
mod tests { ... }
```

### Documentation

- Add doc comments to all public items
- Include examples in doc comments where helpful
- Keep comments up-to-date with code changes

```rust
/// Creates a new memory with the given content.
///
/// # Arguments
///
/// * `content` - The text content of the memory
/// * `memory_type` - Classification of the memory
///
/// # Returns
///
/// Returns `Ok(Memory)` on success, or an error if validation fails.
///
/// # Example
///
/// ```
/// let memory = create_memory("Hello world", MemoryType::Note)?;
/// ```
pub fn create_memory(content: &str, memory_type: MemoryType) -> Result<Memory> {
    // ...
}
```

### Testing

- Write unit tests for all new functionality
- Use descriptive test names
- Test both success and error cases
- Keep tests focused and independent

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_memory_with_valid_content() {
        let memory = create_memory("test content", MemoryType::Note).unwrap();
        assert_eq!(memory.content, "test content");
    }

    #[test]
    fn test_create_memory_with_empty_content_fails() {
        let result = create_memory("", MemoryType::Note);
        assert!(result.is_err());
    }
}
```

## Project Structure

```
engram/
├── src/
│   ├── lib.rs           # Library entry point
│   ├── types.rs         # Core type definitions
│   ├── error.rs         # Error types
│   ├── bin/             # Binary entry points
│   ├── storage/         # Database layer
│   ├── search/          # Search functionality
│   ├── embedding/       # Vector embeddings
│   ├── sync/            # Cloud synchronization
│   ├── auth/            # Authentication
│   ├── intelligence/    # AI features
│   ├── graph/           # Knowledge graph
│   ├── realtime/        # WebSocket support
│   └── mcp/             # MCP protocol
├── benches/             # Performance benchmarks
├── tests/               # Integration tests
└── examples/            # Usage examples
```

## Getting Help

- Open a [Discussion](https://github.com/limaronaldo/engram/discussions) for questions
- Check existing issues and discussions
- Read the [README](README.md) and documentation

## Recognition

Contributors will be recognized in:
- The project's README
- Release notes for significant contributions
- The CHANGELOG for their changes

Thank you for contributing to Engram!
