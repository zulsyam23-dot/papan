use papan_backend::{BackendError, BackendResult};

// S-expression reader
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SExpr {
    Atom(String),
    List(Vec<SExpr>),
}

impl SExpr {
    pub(crate) fn atom(&self) -> Option<&str> {
        match self {
            SExpr::Atom(value) => Some(value),
            SExpr::List(_) => None,
        }
    }

    pub(crate) fn list(&self) -> Option<&[SExpr]> {
        match self {
            SExpr::List(values) => Some(values),
            SExpr::Atom(_) => None,
        }
    }

    pub(crate) fn head(&self) -> Option<&str> {
        self.list().and_then(|values| values.first()).and_then(SExpr::atom)
    }

    pub(crate) fn children(&self, head: &str) -> Vec<&SExpr> {
        self.list()
            .map(|values| {
                values
                    .iter()
                    .filter(|value| value.head().map(|value| value.eq_ignore_ascii_case(head)).unwrap_or(false))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn child(&self, head: &str) -> Option<&SExpr> {
        self.list()?.iter().find(|value| {
            value
                .head()
                .map(|value| value.eq_ignore_ascii_case(head))
                .unwrap_or(false)
        })
    }

    pub(crate) fn atoms(&self) -> Vec<&str> {
        match self {
            SExpr::Atom(value) => vec![value.as_str()],
            SExpr::List(values) => values.iter().filter_map(SExpr::atom).collect(),
        }
    }

}

pub(crate) fn parse_sexpr(text: &str) -> BackendResult<Vec<SExpr>> {
    let mut parser = SexprParser {
        chars: text.chars().collect(),
        index: 0,
    };
    parser.parse_all()
}

struct SexprParser {
    chars: Vec<char>,
    index: usize,
}

impl SexprParser {
    fn parse_all(&mut self) -> BackendResult<Vec<SExpr>> {
        let mut expressions = Vec::new();
        loop {
            self.skip_whitespace();
            if self.index >= self.chars.len() {
                break;
            }
            expressions.push(self.parse_expr()?);
        }
        Ok(expressions)
    }

    fn parse_expr(&mut self) -> BackendResult<SExpr> {
        self.skip_whitespace();
        let Some(ch) = self.chars.get(self.index).copied() else {
            return Err(BackendError::new(
                "KICAD_001",
                "unexpected end of KiCad file",
            ));
        };
        match ch {
            '(' => {
                self.index += 1;
                let mut values = Vec::new();
                loop {
                    self.skip_whitespace();
                    match self.chars.get(self.index).copied() {
                        Some(')') => {
                            self.index += 1;
                            break;
                        }
                        None => {
                            return Err(BackendError::new(
                                "KICAD_002",
                                "unbalanced '(' in KiCad file",
                            ))
                        }
                        _ => values.push(self.parse_expr()?),
                    }
                }
                Ok(SExpr::List(values))
            }
            ')' => Err(BackendError::new(
                "KICAD_003",
                "unexpected ')' in KiCad file",
            )),
            '"' => {
                self.index += 1;
                let mut value = String::new();
                let mut closed = false;
                while let Some(ch) = self.chars.get(self.index).copied() {
                    self.index += 1;
                    match ch {
                        '"' => {
                            closed = true;
                            break;
                        }
                        '\\' => {
                            if let Some(next) = self.chars.get(self.index).copied() {
                                self.index += 1;
                                value.push(match next {
                                    'n' => '\n',
                                    't' => '\t',
                                    other => other,
                                });
                            }
                        }
                        other => value.push(other),
                    }
                }
                if !closed {
                    return Err(BackendError::new(
                        "KICAD_004",
                        "unterminated string in KiCad file",
                    ));
                }
                Ok(SExpr::Atom(value))
            }
            _ => {
                let mut value = String::new();
                while let Some(ch) = self.chars.get(self.index).copied() {
                    if ch.is_whitespace() || ch == '(' || ch == ')' || ch == '"' {
                        break;
                    }
                    value.push(ch);
                    self.index += 1;
                }
                Ok(SExpr::Atom(value))
            }
        }
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.chars.get(self.index).copied() {
            if ch.is_whitespace() {
                self.index += 1;
            } else {
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------