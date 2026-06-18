
# Contributing to pkgtrace

Thank you for your interest in contributing to pkgtrace! This document provides guidelines and instructions for contributing to the project.

## Table of Contents
Hang on guys! These jump-links are currently NOT working(on my device)! I guess Spectra will help me rewrite them 🤣.

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [Project Structure](#project-structure)
- [Development Setup](#development-setup)
- [Coding Standards](#coding-standards)
- [Testing Guidelines](#testing-guidelines)
- [Pull Request Process](#pull-request-process)
- [Commit Guidelines](#commit-guidelines)
- [Issue Reporting](#issue-reporting)
- [Documentation](#documentation)
- [Release Process](#release-process)
- [Getting Help](#getting-help)

## Code of Conduct

This project adheres to a Code of Conduct that all contributors are expected to follow. Please treat others with respect and professionalism.

- Be welcoming and inclusive
- Be respectful of differing viewpoints
- Gracefully accept constructive criticism
- Focus on what is best for the community
- Show empathy towards other community members
- No racism or political wars of any kind

## Getting Started

### Prerequisites

- **Rust** (latest stable version) - [Install Rust](https://rustup.rs/)
- **Cargo** (included with Rust)
- **Git** for version control
- **Termux** (for testing on Android)
- **Graphviz** (optional, for dependency graph visualization)

### First-Time Contributors

1. **Fork the repository** on GitHub
2. **Clone your fork**:
   ```bash
   git clone https://github.com/your-username/pkgtrace.git && cd pkgtrace
   ```

3. Set up the development environment:
   ```bash
   rustup update && rustup component add clippy rustfmt
   ```
4. Build the project:
   ```bash
   cargo build
   ```
5. Run tests to ensure everything works:
   ```bash
   cargo test
   ```

Project Structure

Understanding the codebase is crucial for effective contribution:

```
src/
cmd/
   ├── mod.rs        # Command handlers   
├── main.rs          # CLI entry point 
├── tracker.rs       # Core package tracking logic
├── analyzer.rs      # Analysis and reporting logic
├── scanner.rs       # Multi-source package scanning
├── config.rs        # Configuration management
├── models.rs        # Data models and types
├── utils.rs         # Utility functions
├── logger.rs        # Logging system
├── cache.rs         # Cache management
└── lib.rs           # Library entry point (currently minimal)

Other files:
├── Cargo.toml       # Project dependencies and metadata
├── README.md        # User documentation
├── build.sh         # Build script
├── install.sh       # Installation script
├── LICENSE          # MIT License
└── .gitignore       # Git ignore rules
```

### Key Modules

###### Module Responsibility
- main.rs CLI parsing
- cmd/mod.rs command routing, user interaction
- tracker.rs Package database, usage tracking, CRUD operations
- analyzer.rs Unused package detection, dependency analysis, risk assessment
- scanner.rs Scanning packages from pkg, cargo, pip, npm, gem, and manual installs
- config.rs Configuration loading, validation, and management
- models.rs Core data structures (Package, PackageSource, UnusedPackage, etc.)
- utils.rs Helper functions (formatting, file operations, checksums)
- logger.rs Event logging, log rotation, querying log history
- cache.rs Package cache management, serialization, freshness checking

### Development Setup

###### Build and Run

```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release

# Run with specific command
cargo run -- scan
cargo run -- list --sizes
cargo run -- unused --deps --explain

# Run tests
cargo test

# Run specific test
cargo test test_config_default
```

###### Using Clippy

We use Clippy for linting. Run it before submitting:

```bash
cargo clippy -- -D warnings
```

###### Formatting

Use rustfmt to maintain consistent code style:

```bash
cargo fmt -- --check  # Check formatting
cargo fmt              # Apply formatting
```

###### Testing on Termux

For testing on actual Termux environment:

```bash
# Build for Termux
cargo build --release --target aarch64-linux-android

# Or install directly
./install.sh
```

### Coding Standards

#### Rust Guidelines

1. Use anyhow::Result for error handling in most cases:
   ```rust
   fn my_function() -> Result<()> {
       // implementation
   }
   ```
2. Use appropriate error types:
   · anyhow::Error for application-level errors
   · Custom error types for library-style code
   · Use anyhow::anyhow! for creating errors
3. Documentation comments for all public items:
   ```rust
   /// Returns the dependency graph for a package
   ///
   /// # Arguments
   /// * `package` - The package name to analyze
   ///
   /// # Returns
   /// A DependencyGraph struct or an error
   pub fn get_dependency_graph(&self, package: &str) -> Result<DependencyGraph> {
       // implementation
   }
   ```
4. Use descriptive variable names and avoid abbreviations:
   · package_count instead of pkg_cnt
   · installation_path instead of inst_path

You' re to break this rule only when it's logically understandable but also provide explanation for the abbreviation to avoid confusing the team.

5. Follow the RAII pattern and use smart pointers:
   · Use Arc for shared ownership
   · Use RwLock for concurrent access
6. Minimize unwrap and expect:
   · Use ? operator for propagation
   · Use unwrap_or, unwrap_or_else for defaults
   · Only use unwrap when you're certain it's safe

#### Naming Conventions

- Item Convention Example
- Types PascalCase PackageScanner, CacheManager
- Functions/Methods snake_case
- scan_all_packages(), load_cache()
- Variables snake_case total_packages, is_used
- Constants SCREAMING_SNAKE_CASE MAX_LOG_SIZE
- Modules snake_case tracker.rs, analyzer.rs

#### Code Organization

1. Keep functions focused: Each function should do one thing
2. Limit function length: Try to keep functions under 50 lines
3. Group related functions: Use impl blocks and module organization
4. Use early returns: Return early for error cases
5. Avoid deep nesting: Use guard clauses and early returns
6. Practice Idiomatic Rust As Much As You Can

#### Comments

- Write meaningful comments for complex logic
- Use // for line comments
- Use /// for documentation comments (rustdoc)
- Avoid commented-out code (use version control instead)

### Testing Guidelines

#### Writing Tests

1. Unit tests in the same file using #[cfg(test)]:
   ```rust
   #[cfg(test)]
   mod tests {
       use super::*;
       
       #[test]
       fn test_function_name() {
           // test implementation
       }
   }
   ```
2. Integration tests in tests/ directory
3. Coverage: Aim for >70% code coverage

#### Test Categories

Test Type Location Purpose
Unit tests In each module Test individual functions
Integration tests tests/ Test module interactions
Example tests Examples in docs Verify documentation examples

#### Running Tests

```bash
# All tests
cargo test

# Specific module
cargo test --lib tracker

# With output
cargo test -- --nocapture

# With coverage (requires cargo-tarpaulin)
cargo tarpaulin
```

### Pull Request Process

#### Before Submitting

1. Update your fork:
   ```bash
   git remote add upstream https://github.com/oboobotenefiok/pkgtrace.git
   git fetch upstream
   git rebase upstream/main
   ```
2. Run all checks:
   ```bash
   cargo fmt -- --check
   cargo clippy -- -D warnings
   cargo test
   ```
3. Update documentation if needed

#### PR Checklist

· Code follows style guidelines
· Tests pass and coverage is maintained
· Documentation is updated
· Commit messages follow convention
· No merge conflicts
· Changes are tested on Termux (if relevant)

#### PR Title Format

```
[type] brief description of changes
```

###### Types:

- feat: New feature
- fix: Bug fix
- docs: Documentation only
- style: Code style changes
- refactor: Code refactoring
- perf: Performance improvement
- test: Test additions
- chore: Maintenance tasks

#### PR Review Process

1. Two approvals required for merging
2. Address feedback promptly
3. Keep PRs focused: One logical change per PR
4. Large changes: Open a discussion first

### Commit Guidelines

#### Commit Message Format

```
<type>(<scope>): <subject>

<body>

<footer>
```

#### Types

Type Description
feat New feature
fix Bug fix
docs Documentation changes
style Code style/formatting
refactor Code refactoring
perf Performance improvements
test Test additions/modifications
chore Maintenance tasks

#### Examples

```bash
feat(scanner): add support for scanning npm global packages

Implement npm global package scanning using `npm list --global --depth=0`
with JSON output parsing.

Closes #42
```

```bash
fix(tracker): correctly handle dependency cycles

Prevent infinite recursion when resolving dependencies
by detecting cycles and marking them appropriately.
```

### Issue Reporting

#### Bug Reports

When reporting a bug, include:

1. Environment: OS, Termux version, Rust version
2. Steps to reproduce: Clear step-by-step instructions
3. Expected behavior: What should happen
4. Actual behavior: What actually happens
5. Logs/Output: Relevant error messages or logs

### Feature Requests

For feature requests, include:

1. Use case: Why is this needed?
2. Proposed solution: How should it work?
3. Alternatives: Other approaches considered
4. Impact: Who would benefit?

### Documentation

#### Code Documentation

- Use /// for public API documentation
- Include examples for complex functions
- Document error conditions and panics

#### User Documentation

Update README.md for:

- New features
- Changed behavior
- New configuration options

#### Updatege

For CLI changes, update the man page:

```bash
# Generate man page
pandoc -s -t man README.md -o pkgtrace.1
```

### Release Process

#### Follown Numbering

Follow Semantic Versioning:

- MAJOR: Incompatible API changes
- MINOR: Backward-compatible new features
- PATCH: Backward-compatible bug fixes

#### Release Checklist

1. Update version in Cargo.toml
2. Update CHANGELOG.md
3. Update README.md with new features
4. Run full test suite
5. Build release binary
6. Create GitHub release
7. Update package repositories

### Getting Help

#### Communication Channels

-  Issues: GitHub Issues
-  Discussions: GitHub Discussions
-  Termux Community: Termux Community

#### Good First Issues

###### Look for issues tagged:

- good-first-issue
- help-wanted
- beginner

### Development Mentorship

If you're new to Rust or the project, don't hesitate to ask for help. We're happy to mentor new contributors.

### Additional Resources

- Rust Book
- Rust API Guidelines
- Termux Documentation
- Clap Documentation
- Serde Documentation

---

Thank you for contributing to pkgtrace! Your efforts help make package management on Termux better for everyone.

Happy coding! 🦀

With Love,

- Obot & The Team