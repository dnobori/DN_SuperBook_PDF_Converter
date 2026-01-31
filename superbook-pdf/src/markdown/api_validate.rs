//! External API Validation module
//!
//! Provides optional validation of Markdown output using external LLM APIs.

use std::path::Path;

use super::types::{MarkdownError, Result};

// ============================================================
// Types
// ============================================================

/// API provider for validation
#[derive(Debug, Clone)]
pub enum ValidationProvider {
    /// Claude API
    Claude { api_key: String },
    /// OpenAI API
    OpenAI { api_key: String },
    /// Local LLM endpoint
    LocalLLM { endpoint: String },
}

impl ValidationProvider {
    /// Create a Claude provider
    pub fn claude(api_key: impl Into<String>) -> Self {
        ValidationProvider::Claude {
            api_key: api_key.into(),
        }
    }

    /// Create an OpenAI provider
    pub fn openai(api_key: impl Into<String>) -> Self {
        ValidationProvider::OpenAI {
            api_key: api_key.into(),
        }
    }

    /// Create a local LLM provider
    pub fn local(endpoint: impl Into<String>) -> Self {
        ValidationProvider::LocalLLM {
            endpoint: endpoint.into(),
        }
    }

    /// Get provider name
    pub fn name(&self) -> &str {
        match self {
            ValidationProvider::Claude { .. } => "claude",
            ValidationProvider::OpenAI { .. } => "openai",
            ValidationProvider::LocalLLM { .. } => "local",
        }
    }
}

/// Result of validation
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// Overall validation passed
    pub valid: bool,

    /// Confidence score (0.0-1.0)
    pub confidence: f64,

    /// Issues found
    pub issues: Vec<ValidationIssue>,

    /// Suggestions for improvement
    pub suggestions: Vec<String>,

    /// Provider used
    pub provider: String,
}

impl ValidationResult {
    /// Create a successful validation result
    pub fn success() -> Self {
        Self {
            valid: true,
            confidence: 1.0,
            issues: Vec::new(),
            suggestions: Vec::new(),
            provider: String::new(),
        }
    }

    /// Create a failed validation result
    pub fn failed(issues: Vec<ValidationIssue>) -> Self {
        Self {
            valid: false,
            confidence: 0.0,
            issues,
            suggestions: Vec::new(),
            provider: String::new(),
        }
    }
}

/// A validation issue
#[derive(Debug, Clone)]
pub struct ValidationIssue {
    /// Issue severity
    pub severity: IssueSeverity,

    /// Description of the issue
    pub description: String,

    /// Location in the document (optional)
    pub location: Option<String>,

    /// Suggested fix
    pub fix: Option<String>,
}

impl ValidationIssue {
    /// Create a new issue
    pub fn new(severity: IssueSeverity, description: impl Into<String>) -> Self {
        Self {
            severity,
            description: description.into(),
            location: None,
            fix: None,
        }
    }

    /// Set location
    pub fn with_location(mut self, location: impl Into<String>) -> Self {
        self.location = Some(location.into());
        self
    }

    /// Set fix
    pub fn with_fix(mut self, fix: impl Into<String>) -> Self {
        self.fix = Some(fix.into());
        self
    }
}

/// Severity of a validation issue
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSeverity {
    /// Informational
    Info,
    /// Warning - may need attention
    Warning,
    /// Error - definitely needs fixing
    Error,
}

// ============================================================
// API Validator
// ============================================================

/// Validator using external APIs
pub struct ApiValidator {
    provider: ValidationProvider,
}

impl ApiValidator {
    /// Create a new validator
    pub fn new(provider: ValidationProvider) -> Self {
        Self { provider }
    }

    /// Validate Markdown content
    pub fn validate(&self, markdown: &str) -> Result<ValidationResult> {
        // For now, perform local validation only
        // External API integration would be added in a future iteration
        self.validate_locally(markdown)
    }

    /// Validate a Markdown file
    pub fn validate_file(&self, path: &Path) -> Result<ValidationResult> {
        let content =
            std::fs::read_to_string(path).map_err(|e| MarkdownError::IoError(e))?;

        self.validate(&content)
    }

    /// Perform local validation (no API call)
    fn validate_locally(&self, markdown: &str) -> Result<ValidationResult> {
        let mut issues = Vec::new();
        let mut suggestions = Vec::new();

        // Check for common issues
        self.check_empty_content(markdown, &mut issues);
        self.check_heading_structure(markdown, &mut issues, &mut suggestions);
        self.check_broken_links(markdown, &mut issues);
        self.check_unclosed_code_blocks(markdown, &mut issues);
        self.check_table_structure(markdown, &mut issues);

        let valid = issues.iter().all(|i| i.severity != IssueSeverity::Error);
        let error_count = issues.iter().filter(|i| i.severity == IssueSeverity::Error).count();
        let warning_count = issues.iter().filter(|i| i.severity == IssueSeverity::Warning).count();

        let confidence = if issues.is_empty() {
            1.0
        } else {
            (1.0 - (error_count as f64 * 0.2 + warning_count as f64 * 0.05)).max(0.0)
        };

        Ok(ValidationResult {
            valid,
            confidence,
            issues,
            suggestions,
            provider: self.provider.name().to_string(),
        })
    }

    /// Check for empty content
    fn check_empty_content(&self, markdown: &str, issues: &mut Vec<ValidationIssue>) {
        if markdown.trim().is_empty() {
            issues.push(ValidationIssue::new(
                IssueSeverity::Error,
                "Document is empty",
            ));
        }
    }

    /// Check heading structure
    fn check_heading_structure(
        &self,
        markdown: &str,
        issues: &mut Vec<ValidationIssue>,
        suggestions: &mut Vec<String>,
    ) {
        let mut last_level = 0u8;
        let mut has_h1 = false;

        for (line_num, line) in markdown.lines().enumerate() {
            if let Some(level) = Self::get_heading_level(line) {
                if level == 1 {
                    if has_h1 {
                        issues.push(
                            ValidationIssue::new(
                                IssueSeverity::Warning,
                                "Multiple H1 headings found",
                            )
                            .with_location(format!("Line {}", line_num + 1)),
                        );
                    }
                    has_h1 = true;
                }

                // Check for skipped levels
                if level > last_level + 1 && last_level > 0 {
                    issues.push(
                        ValidationIssue::new(
                            IssueSeverity::Warning,
                            format!("Skipped heading level (H{} to H{})", last_level, level),
                        )
                        .with_location(format!("Line {}", line_num + 1)),
                    );
                    suggestions.push(format!(
                        "Consider using H{} before H{} at line {}",
                        last_level + 1,
                        level,
                        line_num + 1
                    ));
                }

                last_level = level;
            }
        }

        if !has_h1 {
            suggestions.push("Consider adding an H1 heading at the beginning of the document".to_string());
        }
    }

    /// Check for broken image/link syntax
    fn check_broken_links(&self, markdown: &str, issues: &mut Vec<ValidationIssue>) {
        for (line_num, line) in markdown.lines().enumerate() {
            // Check for malformed image syntax
            if line.contains("![") && !line.contains("](") {
                issues.push(
                    ValidationIssue::new(
                        IssueSeverity::Warning,
                        "Possible malformed image/link syntax",
                    )
                    .with_location(format!("Line {}", line_num + 1))
                    .with_fix("Ensure images follow ![alt](url) format"),
                );
            }

            // Check for empty links
            if line.contains("[]()") {
                issues.push(
                    ValidationIssue::new(IssueSeverity::Warning, "Empty link found")
                        .with_location(format!("Line {}", line_num + 1)),
                );
            }
        }
    }

    /// Check for unclosed code blocks
    fn check_unclosed_code_blocks(&self, markdown: &str, issues: &mut Vec<ValidationIssue>) {
        let fence_count = markdown.matches("```").count();
        if fence_count % 2 != 0 {
            issues.push(ValidationIssue::new(
                IssueSeverity::Error,
                "Unclosed code block (mismatched ``` fences)",
            ));
        }
    }

    /// Check table structure
    fn check_table_structure(&self, markdown: &str, issues: &mut Vec<ValidationIssue>) {
        let mut in_table = false;
        let mut expected_cols: Option<usize> = None;

        for (line_num, line) in markdown.lines().enumerate() {
            let is_table_row = line.trim().starts_with('|') && line.trim().ends_with('|');

            if is_table_row {
                let col_count = line.matches('|').count() - 1; // -1 for the extra | at ends

                if !in_table {
                    in_table = true;
                    expected_cols = Some(col_count);
                } else if let Some(expected) = expected_cols {
                    if col_count != expected {
                        issues.push(
                            ValidationIssue::new(
                                IssueSeverity::Warning,
                                format!(
                                    "Table row has {} columns, expected {}",
                                    col_count, expected
                                ),
                            )
                            .with_location(format!("Line {}", line_num + 1)),
                        );
                    }
                }
            } else if in_table && !line.trim().is_empty() {
                in_table = false;
                expected_cols = None;
            }
        }
    }

    /// Get heading level from a line
    fn get_heading_level(line: &str) -> Option<u8> {
        let trimmed = line.trim();
        if !trimmed.starts_with('#') {
            return None;
        }

        let level = trimmed.chars().take_while(|&c| c == '#').count();
        if level > 0 && level <= 6 && trimmed.chars().nth(level) == Some(' ') {
            Some(level as u8)
        } else {
            None
        }
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_provider() {
        let claude = ValidationProvider::claude("key123");
        assert_eq!(claude.name(), "claude");

        let openai = ValidationProvider::openai("key456");
        assert_eq!(openai.name(), "openai");

        let local = ValidationProvider::local("http://localhost:8080");
        assert_eq!(local.name(), "local");
    }

    #[test]
    fn test_validation_result_success() {
        let result = ValidationResult::success();
        assert!(result.valid);
        assert_eq!(result.confidence, 1.0);
        assert!(result.issues.is_empty());
    }

    #[test]
    fn test_validation_issue() {
        let issue = ValidationIssue::new(IssueSeverity::Warning, "Test warning")
            .with_location("Line 5")
            .with_fix("Fix this");

        assert_eq!(issue.severity, IssueSeverity::Warning);
        assert_eq!(issue.description, "Test warning");
        assert_eq!(issue.location, Some("Line 5".to_string()));
        assert_eq!(issue.fix, Some("Fix this".to_string()));
    }

    #[test]
    fn test_get_heading_level() {
        assert_eq!(ApiValidator::get_heading_level("# Heading 1"), Some(1));
        assert_eq!(ApiValidator::get_heading_level("## Heading 2"), Some(2));
        assert_eq!(ApiValidator::get_heading_level("### Heading 3"), Some(3));
        assert_eq!(ApiValidator::get_heading_level("###### Heading 6"), Some(6));
        assert_eq!(ApiValidator::get_heading_level("####### Too deep"), None);
        assert_eq!(ApiValidator::get_heading_level("Not a heading"), None);
        assert_eq!(ApiValidator::get_heading_level("#NoSpace"), None);
    }

    #[test]
    fn test_validate_empty_content() {
        let validator = ApiValidator::new(ValidationProvider::local("test"));
        let result = validator.validate("").unwrap();

        assert!(!result.valid);
        assert!(result.issues.iter().any(|i| i.description.contains("empty")));
    }

    #[test]
    fn test_validate_unclosed_code_block() {
        let validator = ApiValidator::new(ValidationProvider::local("test"));
        let markdown = "```rust\nlet x = 1;\n// missing closing fence";
        let result = validator.validate(markdown).unwrap();

        assert!(!result.valid);
        assert!(result
            .issues
            .iter()
            .any(|i| i.description.contains("Unclosed code block")));
    }

    #[test]
    fn test_validate_skipped_heading_levels() {
        let validator = ApiValidator::new(ValidationProvider::local("test"));
        let markdown = "# Title\n\n### Section\n";
        let result = validator.validate(markdown).unwrap();

        // Should warn about skipping H2
        assert!(result
            .issues
            .iter()
            .any(|i| i.description.contains("Skipped heading level")));
    }

    #[test]
    fn test_validate_multiple_h1() {
        let validator = ApiValidator::new(ValidationProvider::local("test"));
        let markdown = "# Title\n\n# Another Title\n";
        let result = validator.validate(markdown).unwrap();

        assert!(result
            .issues
            .iter()
            .any(|i| i.description.contains("Multiple H1")));
    }

    #[test]
    fn test_validate_table_structure() {
        let validator = ApiValidator::new(ValidationProvider::local("test"));
        let markdown = "| A | B | C |\n| --- | --- | --- |\n| 1 | 2 |\n";
        let result = validator.validate(markdown).unwrap();

        assert!(result
            .issues
            .iter()
            .any(|i| i.description.contains("columns")));
    }

    #[test]
    fn test_validate_valid_document() {
        let validator = ApiValidator::new(ValidationProvider::local("test"));
        let markdown = r#"# Title

Some paragraph text.

## Section

More content here.

```rust
let x = 1;
```
"#;
        let result = validator.validate(markdown).unwrap();

        assert!(result.valid);
        assert!(result.confidence > 0.8);
    }
}
