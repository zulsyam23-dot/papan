use serde::{Deserialize, Serialize};

/// A stable object identifier.
///
/// IDs are intentionally opaque strings. Stable IDs are generated from the
/// object kind and its user facing name, so moving or modifying an object does
/// not change its identity.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ObjectId(pub String);

impl ObjectId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Deterministic ID from a kind and a human readable name.
    pub fn stable(kind: &str, name: &str) -> Self {
        Self(format!("{kind}:{}", sanitize(name)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Display for ObjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for ObjectId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl From<String> for ObjectId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

fn sanitize(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
            out.push(ch);
        } else if ch.is_whitespace() {
            out.push('_');
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push_str("unnamed");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_ids_are_deterministic() {
        assert_eq!(
            ObjectId::stable("component", "MCU"),
            ObjectId::stable("component", "MCU")
        );
        assert_ne!(
            ObjectId::stable("component", "MCU"),
            ObjectId::stable("net", "MCU")
        );
    }

    #[test]
    fn spaces_are_normalized() {
        assert_eq!(ObjectId::stable("net", "VBUS OUT").as_str(), "net:VBUS_OUT");
    }
}
