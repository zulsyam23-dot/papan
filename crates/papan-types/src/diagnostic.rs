use serde::{Deserialize, Serialize};

use crate::source::Span;
use crate::units::Point;

/// Severity of a diagnostic. Waived rules are recorded but never block a
/// production-ready verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Error,
    Waived,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
            Severity::Waived => "waived",
        }
    }

    pub fn blocks_production(self) -> bool {
        self == Severity::Error
    }
}

/// One compiler, verifier, ERC or DRC message with a stable code.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: String,
    pub severity: Severity,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

impl Diagnostic {
    pub fn new(
        code: impl Into<String>,
        severity: Severity,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            severity,
            message: message.into(),
            span: None,
            object: None,
            location: None,
            suggestion: None,
        }
    }

    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(code, Severity::Error, message)
    }

    pub fn warning(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(code, Severity::Warning, message)
    }

    pub fn info(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(code, Severity::Info, message)
    }

    pub fn waived(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(code, Severity::Waived, message)
    }

    pub fn with_span(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }

    pub fn with_object(mut self, object: impl Into<String>) -> Self {
        self.object = Some(object.into());
        self
    }

    pub fn with_location(mut self, location: Point) -> Self {
        self.location = Some(location);
        self
    }

    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

/// Ordered collection of diagnostics.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Diagnostics(pub Vec<Diagnostic>);

impl Diagnostics {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.0.push(diagnostic);
    }

    pub fn extend(&mut self, other: impl IntoIterator<Item = Diagnostic>) {
        self.0.extend(other);
    }

    pub fn append(&mut self, other: &mut Diagnostics) {
        self.0.append(&mut other.0);
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Diagnostic> {
        self.0.iter()
    }

    pub fn as_slice(&self) -> &[Diagnostic] {
        &self.0
    }

    pub fn into_vec(self) -> Vec<Diagnostic> {
        self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn has_errors(&self) -> bool {
        self.0.iter().any(Diagnostic::is_error)
    }

    pub fn errors(&self) -> usize {
        self.0.iter().filter(|d| d.severity == Severity::Error).count()
    }

    pub fn warnings(&self) -> usize {
        self.0
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .count()
    }

    pub fn infos(&self) -> usize {
        self.0
            .iter()
            .filter(|d| d.severity == Severity::Info)
            .count()
    }

    pub fn waived(&self) -> usize {
        self.0
            .iter()
            .filter(|d| d.severity == Severity::Waived)
            .count()
    }

    /// Sort diagnostics deterministically by severity, code, span and message.
    pub fn sort(&mut self) {
        self.0.sort_by(|a, b| {
            b.severity
                .cmp(&a.severity)
                .then_with(|| a.code.cmp(&b.code))
                .then_with(|| a.span.map(|s| s.start).cmp(&b.span.map(|s| s.start)))
                .then_with(|| a.message.cmp(&b.message))
        });
    }
}

impl std::ops::Deref for Diagnostics {
    type Target = [Diagnostic];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl IntoIterator for Diagnostics {
    type Item = Diagnostic;
    type IntoIter = std::vec::IntoIter<Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a Diagnostics {
    type Item = &'a Diagnostic;
    type IntoIter = std::slice::Iter<'a, Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl FromIterator<Diagnostic> for Diagnostics {
    fn from_iter<T: IntoIterator<Item = Diagnostic>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl From<Vec<Diagnostic>> for Diagnostics {
    fn from(value: Vec<Diagnostic>) -> Self {
        Self(value)
    }
}

/// Report produced by the validation pipeline (ERC + DRC + verifier).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationReport {
    pub diagnostics: Diagnostics,
    pub production_ready: bool,
}

impl ValidationReport {
    pub fn from_diagnostics(diagnostics: Diagnostics) -> Self {
        let production_ready = !diagnostics.has_errors();
        Self {
            diagnostics,
            production_ready,
        }
    }

    pub fn empty() -> Self {
        Self {
            diagnostics: Diagnostics::new(),
            production_ready: true,
        }
    }

    pub fn merge(&mut self, other: ValidationReport) {
        self.diagnostics.extend(other.diagnostics);
        self.production_ready = self.production_ready && other.production_ready;
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics.has_errors()
    }

    pub fn counts(&self) -> (usize, usize, usize, usize) {
        (
            self.diagnostics.errors(),
            self.diagnostics.warnings(),
            self.diagnostics.infos(),
            self.diagnostics.waived(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_ready_only_without_errors() {
        let mut diagnostics = Diagnostics::new();
        diagnostics.push(Diagnostic::warning("W1", "warning"));
        let report = ValidationReport::from_diagnostics(diagnostics);
        assert!(report.production_ready);

        let mut diagnostics = Diagnostics::new();
        diagnostics.push(Diagnostic::error("E1", "error"));
        let report = ValidationReport::from_diagnostics(diagnostics);
        assert!(!report.production_ready);
    }

    #[test]
    fn waived_does_not_block() {
        let diagnostics = Diagnostics(vec![Diagnostic::waived("W", "waived")]);
        assert!(!diagnostics.has_errors());
    }
}
