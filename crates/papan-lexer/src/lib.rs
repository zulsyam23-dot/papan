//! Deterministic lexer for PAPAN source files.
//!
//! The lexer preserves source spans for every token and emits structured
//! diagnostics instead of panicking. Newlines are significant tokens because
//! PAPAN statements are line-oriented, while blocks use braces.

use papan_types::{Diagnostic, Diagnostics, Span};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Ident(String),
    /// Numeric literal including its optional unit suffix, e.g. `10kΩ`.
    Number(String),
    Str(String),
    Arrow,
    LBrace,
    RBrace,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Colon,
    Dot,
    Semicolon,
    Equals,
    Newline,
    Eof,
}

impl TokenKind {
    pub fn describe(&self) -> String {
        match self {
            TokenKind::Ident(value) => format!("identifier '{value}'"),
            TokenKind::Number(value) => format!("number '{value}'"),
            TokenKind::Str(value) => format!("string \"{value}\""),
            TokenKind::Arrow => "'->'".to_string(),
            TokenKind::LBrace => "'{'".to_string(),
            TokenKind::RBrace => "'}'".to_string(),
            TokenKind::LParen => "'('".to_string(),
            TokenKind::RParen => "')'".to_string(),
            TokenKind::LBracket => "'['".to_string(),
            TokenKind::RBracket => "']'".to_string(),
            TokenKind::Comma => "','".to_string(),
            TokenKind::Colon => "':'".to_string(),
            TokenKind::Dot => "'.'".to_string(),
            TokenKind::Semicolon => "';'".to_string(),
            TokenKind::Equals => "'='".to_string(),
            TokenKind::Newline => "end of line".to_string(),
            TokenKind::Eof => "end of file".to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Clone, Debug, Default)]
pub struct LexOutput {
    pub tokens: Vec<Token>,
    pub diagnostics: Diagnostics,
}

pub fn lex(source: &str) -> LexOutput {
    let mut output = LexOutput::default();
    let mut index = 0usize;
    let bytes = source.as_bytes();

    while index < bytes.len() {
        let ch = source[index..].chars().next().unwrap();
        let next = source[index + ch.len_utf8()..].chars().next();

        match ch {
            ' ' | '\t' | '\r' => {
                index += ch.len_utf8();
            }
            '\n' => {
                output.tokens.push(Token {
                    kind: TokenKind::Newline,
                    span: Span::new(index, index + 1),
                });
                index += 1;
            }
            '#' => {
                index = skip_line_comment(source, index);
            }
            '/' if next == Some('/') => {
                index = skip_line_comment(source, index);
            }
            '/' if next == Some('*') => {
                let start = index;
                index += 2;
                let mut closed = false;
                while index < bytes.len() {
                    if source[index..].starts_with("*/") {
                        index += 2;
                        closed = true;
                        break;
                    }
                    index += source[index..].chars().next().unwrap().len_utf8();
                }
                if !closed {
                    output.diagnostics.push(
                        Diagnostic::error("LEX_002", "unterminated block comment")
                            .with_span(Span::new(start, source.len()))
                            .with_suggestion("close the comment with */"),
                    );
                }
            }
            '"' => {
                let start = index;
                index += 1;
                let mut value = String::new();
                let mut closed = false;
                while index < bytes.len() {
                    let current = source[index..].chars().next().unwrap();
                    index += current.len_utf8();
                    match current {
                        '"' => {
                            closed = true;
                            break;
                        }
                        '\\' => {
                            if index >= bytes.len() {
                                break;
                            }
                            let escaped = source[index..].chars().next().unwrap();
                            index += escaped.len_utf8();
                            match escaped {
                                'n' => value.push('\n'),
                                'r' => value.push('\r'),
                                't' => value.push('\t'),
                                '"' => value.push('"'),
                                '\\' => value.push('\\'),
                                other => {
                                    value.push(other);
                                }
                            }
                        }
                        other => value.push(other),
                    }
                }
                if !closed {
                    output.diagnostics.push(
                        Diagnostic::error("LEX_003", "unterminated string literal")
                            .with_span(Span::new(start, source.len()))
                            .with_suggestion("add a closing '\"'"),
                    );
                }
                output.tokens.push(Token {
                    kind: TokenKind::Str(value),
                    span: Span::new(start, index),
                });
            }
            '-' if next == Some('>') => {
                output.tokens.push(Token {
                    kind: TokenKind::Arrow,
                    span: Span::new(index, index + 2),
                });
                index += 2;
            }
            '{' => {
                push_single(&mut output, TokenKind::LBrace, index, 1);
                index += 1;
            }
            '}' => {
                push_single(&mut output, TokenKind::RBrace, index, 1);
                index += 1;
            }
            '(' => {
                push_single(&mut output, TokenKind::LParen, index, 1);
                index += 1;
            }
            ')' => {
                push_single(&mut output, TokenKind::RParen, index, 1);
                index += 1;
            }
            '[' => {
                push_single(&mut output, TokenKind::LBracket, index, 1);
                index += 1;
            }
            ']' => {
                push_single(&mut output, TokenKind::RBracket, index, 1);
                index += 1;
            }
            ',' => {
                push_single(&mut output, TokenKind::Comma, index, 1);
                index += 1;
            }
            ':' => {
                push_single(&mut output, TokenKind::Colon, index, 1);
                index += 1;
            }
            '.' => {
                if next.map(|c| c.is_ascii_digit()).unwrap_or(false) {
                    index = lex_number(source, index, &mut output);
                } else {
                    push_single(&mut output, TokenKind::Dot, index, 1);
                    index += 1;
                }
            }
            ';' => {
                push_single(&mut output, TokenKind::Semicolon, index, 1);
                index += 1;
            }
            '=' => {
                push_single(&mut output, TokenKind::Equals, index, 1);
                index += 1;
            }
            _ if ch.is_ascii_digit() => {
                index = lex_number(source, index, &mut output);
            }
            _ if is_ident_start(ch) => {
                let start = index;
                index += ch.len_utf8();
                while index < bytes.len() {
                    let current = source[index..].chars().next().unwrap();
                    if is_ident_continue(current) {
                        index += current.len_utf8();
                    } else {
                        break;
                    }
                }
                output.tokens.push(Token {
                    kind: TokenKind::Ident(source[start..index].to_string()),
                    span: Span::new(start, index),
                });
            }
            other => {
                output.diagnostics.push(
                    Diagnostic::error(
                        "LEX_001",
                        format!("unexpected character '{other}'"),
                    )
                    .with_span(Span::new(index, index + other.len_utf8())),
                );
                index += other.len_utf8();
            }
        }
    }

    output.tokens.push(Token {
        kind: TokenKind::Eof,
        span: Span::empty(source.len()),
    });
    output
}

fn push_single(output: &mut LexOutput, kind: TokenKind, start: usize, len: usize) {
    output.tokens.push(Token {
        kind,
        span: Span::new(start, start + len),
    });
}

fn skip_line_comment(source: &str, start: usize) -> usize {
    let mut index = start;
    while index < source.len() {
        let ch = source[index..].chars().next().unwrap();
        if ch == '\n' {
            break;
        }
        index += ch.len_utf8();
    }
    index
}

fn lex_number(source: &str, start: usize, output: &mut LexOutput) -> usize {
    let mut index = start;
    let mut dot_seen = false;
    while index < source.len() {
        let ch = source[index..].chars().next().unwrap();
        if ch.is_ascii_digit() {
            index += 1;
        } else if ch == '.' && !dot_seen {
            dot_seen = true;
            index += 1;
        } else {
            break;
        }
    }
    // Unit suffix: letters, percent, ohm, micro and, for area units, `2`.
    let mut unit_started = false;
    while index < source.len() {
        let ch = source[index..].chars().next().unwrap();
        if ch.is_alphabetic() || matches!(ch, '%' | 'Ω' | 'µ' | 'μ' | '°' | '^') {
            unit_started = true;
            index += ch.len_utf8();
        } else if unit_started && ch.is_ascii_digit() {
            index += 1;
        } else {
            break;
        }
    }
    output.tokens.push(Token {
        kind: TokenKind::Number(source[start..index].to_string()),
        span: Span::new(start, index),
    });
    index
}

fn is_ident_start(ch: char) -> bool {
    ch == '_' || ch.is_alphabetic()
}

fn is_ident_continue(ch: char) -> bool {
    ch == '_' || ch.is_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<TokenKind> {
        lex(source)
            .tokens
            .into_iter()
            .map(|t| t.kind)
            .filter(|k| *k != TokenKind::Newline && *k != TokenKind::Eof)
            .collect()
    }

    #[test]
    fn lexes_board_program() {
        let source = "board \"Keyboard\"\nsize 300mm x 120mm\nconnect MCU.VCC -> VCC\n";
        let kinds = kinds(source);
        assert_eq!(kinds[0], TokenKind::Ident("board".into()));
        assert_eq!(kinds[1], TokenKind::Str("Keyboard".into()));
        assert_eq!(kinds[2], TokenKind::Ident("size".into()));
        assert_eq!(kinds[3], TokenKind::Number("300mm".into()));
        assert_eq!(kinds[4], TokenKind::Ident("x".into()));
        assert_eq!(kinds[5], TokenKind::Number("120mm".into()));
        assert!(kinds.contains(&TokenKind::Arrow));
    }

    #[test]
    fn lexes_units_and_comments() {
        let source = "# comment\nvalue: 10kΩ // trailing\nvoltage: 5V\ndrill: 0.3mm\n";
        let output = lex(source);
        assert!(output.diagnostics.is_empty());
        let kinds = kinds(source);
        assert!(kinds.contains(&TokenKind::Number("10kΩ".into())));
        assert!(kinds.contains(&TokenKind::Number("5V".into())));
    }

    #[test]
    fn reports_unterminated_string() {
        let output = lex("board \"oops\n");
        assert!(output.diagnostics.has_errors());
        assert_eq!(output.diagnostics[0].code, "LEX_003");
    }

    #[test]
    fn tracks_spans() {
        let output = lex("size 1mm\n");
        let size = &output.tokens[0];
        assert_eq!(size.span.start, 0);
        assert_eq!(size.span.end, 4);
    }
}
