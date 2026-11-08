//! Recursive-descent parser with error recovery.
//!
//! PAPAN is line-oriented at statement level and brace-oriented for blocks.
//! The parser never panics on invalid input: it emits diagnostics and recovers
//! at the next line or block boundary.

use papan_ast::{
    Block, BoardDecl, ComponentDecl, ComponentKind, Item, ModuleDecl, NetDecl, PlaceTarget,
    Program, Property, QuantityLit, RouteDecl, Value, ViaDecl,
};
use papan_lexer::{lex, Token, TokenKind};
use papan_types::{Diagnostics, Quantity, Span};

#[derive(Clone, Debug, Default)]
pub struct ParseOutput {
    pub program: Program,
    pub diagnostics: Diagnostics,
}

pub fn parse(source: &str) -> ParseOutput {
    let lexed = lex(source);
    let mut parser = Parser {
        tokens: lexed.tokens,
        index: 0,
        diagnostics: lexed.diagnostics,
    };
    let program = parser.parse_program();
    ParseOutput {
        program,
        diagnostics: parser.diagnostics,
    }
}

struct Parser {
    tokens: Vec<Token>,
    index: usize,
    diagnostics: Diagnostics,
}

impl Parser {
    fn parse_program(&mut self) -> Program {
        let mut program = Program::default();
        let start = self.peek().span.start;
        loop {
            self.skip_separators();
            if self.at_eof() {
                break;
            }
            if self.at_rbrace() {
                self.diagnostics.push(
                    papan_types::Diagnostic::error("PARSE_002", "unexpected '}'")
                        .with_span(self.peek().span),
                );
                self.bump();
                continue;
            }
            if self.at_keyword("board") {
                if program.board.is_some() {
                    self.diagnostics.push(
                        papan_types::Diagnostic::error(
                            "PARSE_003",
                            "a source file may declare only one board",
                        )
                        .with_span(self.peek().span),
                    );
                }
                if let Some((board, body)) = self.parse_board() {
                    program.board = Some(board);
                    program.items.extend(body);
                }
            } else if let Some(item) = self.parse_item() {
                program.items.push(item);
            }
        }
        program.span = Span::new(start, self.peek().span.end);
        program
    }

    fn parse_board(&mut self) -> Option<(BoardDecl, Vec<Item>)> {
        let start = self.bump().span.start;
        let name = match self.peek().kind.clone() {
            TokenKind::Str(value) => {
                self.bump();
                value
            }
            TokenKind::Ident(value) => {
                self.diagnostics.push(
                    papan_types::Diagnostic::warning(
                        "PARSE_004",
                        "board name should be a quoted string",
                    )
                    .with_span(self.peek().span),
                );
                self.bump();
                value
            }
            _ => {
                self.expected("a board name string");
                self.recover_line();
                return None;
            }
        };

        let mut body = Vec::new();
        let mut end = self.previous().span.end;
        if self.peek_kind() == Some(&TokenKind::LBrace) {
            let block_start = self.bump().span.start;
            loop {
                self.skip_separators();
                if self.at_eof() {
                    self.diagnostics.push(
                        papan_types::Diagnostic::error(
                            "PARSE_102",
                            "unterminated board block",
                        )
                        .with_span(Span::new(block_start, self.peek().span.end)),
                    );
                    break;
                }
                if self.at_rbrace() {
                    end = self.bump().span.end;
                    break;
                }
                if self.at_keyword("board") {
                    self.diagnostics.push(
                        papan_types::Diagnostic::error(
                            "PARSE_003",
                            "nested board declaration is not allowed",
                        )
                        .with_span(self.peek().span),
                    );
                    self.recover_line();
                } else if let Some(item) = self.parse_item() {
                    body.push(item);
                }
            }
        }
        Some((
            BoardDecl {
                name,
                span: Span::new(start, end),
            },
            body,
        ))
    }

    fn parse_item(&mut self) -> Option<Item> {
        let token = self.peek().clone();
        let keyword = match &token.kind {
            TokenKind::Ident(value) => value.to_ascii_lowercase(),
            _ => {
                self.expected("a statement keyword");
                self.recover_line();
                return None;
            }
        };
        match keyword.as_str() {
            "size" => self.parse_size(),
            "thickness" => self.parse_thickness(),
            "layers" => self.parse_layers(),
            "component" => self.parse_component(),
            "net" => self.parse_net(),
            "connect" => self.parse_connect(),
            "place" => self.parse_place(),
            "rotate" => self.parse_rotate(),
            "route" => self.parse_route(),
            "via" => self.parse_via(),
            "constraints" => self.parse_constraints(),
            "check" => self.parse_check(),
            "export" => self.parse_export(),
            "module" => self.parse_module(),
            "input" => self.parse_module_port("input"),
            "output" => self.parse_module_port("output"),
            other => {
                self.diagnostics.push(
                    papan_types::Diagnostic::error(
                        "PARSE_005",
                        format!("unknown statement '{other}'"),
                    )
                    .with_span(token.span),
                );
                self.bump();
                self.recover_line();
                None
            }
        }
    }

    fn parse_size(&mut self) -> Option<Item> {
        let start = self.bump().span.start;
        let width = self.parse_quantity()?;
        if !self.at_keyword("x") {
            self.expected("'x' between width and height");
            self.recover_line();
            return None;
        }
        self.bump();
        let height = self.parse_quantity()?;
        Some(Item::Size {
            width,
            height,
            span: self.span_from(start),
        })
    }

    fn parse_thickness(&mut self) -> Option<Item> {
        let start = self.bump().span.start;
        let value = self.parse_quantity()?;
        Some(Item::Thickness {
            value,
            span: self.span_from(start),
        })
    }

    fn parse_layers(&mut self) -> Option<Item> {
        let start = self.bump().span.start;
        let token = self.bump().clone();
        let raw = match token.kind {
            TokenKind::Number(value) => value,
            _ => {
                self.diagnostics.push(
                    papan_types::Diagnostic::error("PARSE_103", "layer count must be a number")
                        .with_span(token.span),
                );
                self.recover_line();
                return None;
            }
        };
        match raw.parse::<u32>() {
            Ok(count) => Some(Item::Layers {
                count,
                span: Span::new(start, token.span.end),
            }),
            Err(_) => {
                self.diagnostics.push(
                    papan_types::Diagnostic::error(
                        "PARSE_103",
                        format!("invalid layer count '{raw}'"),
                    )
                    .with_span(token.span),
                );
                None
            }
        }
    }

    fn parse_component(&mut self) -> Option<Item> {
        let start = self.bump().span.start;
        let (name, _) = self.expect_ident("a component designator")?;
        if self.peek_kind() != Some(&TokenKind::Colon) {
            self.expected("':' after component designator");
            self.recover_line();
            return None;
        }
        self.bump();
        let kind = match self.peek().kind.clone() {
            TokenKind::Str(part) => {
                self.bump();
                ComponentKind::Part(part)
            }
            TokenKind::Ident(value) => {
                self.bump();
                ComponentKind::Named(value)
            }
            _ => {
                self.expected("a component type or quoted part name");
                self.recover_line();
                return None;
            }
        };
        let value = if matches!(self.peek().kind, TokenKind::Number(_)) {
            Some(self.parse_quantity()?)
        } else {
            None
        };
        let mut properties = Vec::new();
        if self.peek_kind() == Some(&TokenKind::LBrace) {
            let block = self.parse_block();
            properties = block.properties;
        }
        Some(Item::Component(ComponentDecl {
            name,
            kind,
            value,
            properties,
            span: self.span_from(start),
        }))
    }

    fn parse_net(&mut self) -> Option<Item> {
        let start = self.bump().span.start;
        let (name, _) = self.expect_ident("a net name")?;
        let mut properties = Vec::new();
        if self.peek_kind() == Some(&TokenKind::LBrace) {
            properties = self.parse_block().properties;
        }
        Some(Item::Net(NetDecl {
            name,
            properties,
            span: self.span_from(start),
        }))
    }

    fn parse_connect(&mut self) -> Option<Item> {
        let start = self.bump().span.start;
        let from = self.parse_path()?;
        if self.peek_kind() != Some(&TokenKind::Arrow) {
            self.expected("'->' in connection");
            self.recover_line();
            return None;
        }
        self.bump();
        let to = self.parse_path()?;
        Some(Item::Connect {
            from,
            to,
            span: self.span_from(start),
        })
    }

    fn parse_place(&mut self) -> Option<Item> {
        let start = self.bump().span.start;
        let (component, _) = self.expect_ident("a component designator")?;
        let (keyword, _) = self.expect_ident("'at' or 'near'")?;
        let target = if keyword.eq_ignore_ascii_case("at") {
            if self.peek_kind() == Some(&TokenKind::LParen) {
                self.bump();
                let x = self.parse_quantity()?;
                if self.peek_kind() == Some(&TokenKind::Comma) {
                    self.bump();
                }
                let y = self.parse_quantity()?;
                if self.peek_kind() != Some(&TokenKind::RParen) {
                    self.expected("')' after coordinates");
                } else {
                    self.bump();
                }
                PlaceTarget::At { x, y }
            } else {
                PlaceTarget::Anchor(self.parse_path()?)
            }
        } else if keyword.eq_ignore_ascii_case("near") {
            let reference = self.parse_path()?;
            if !self.at_keyword("distance") {
                self.expected("'distance' after relative placement reference");
                self.recover_line();
                return None;
            }
            self.bump();
            let distance = self.parse_quantity()?;
            PlaceTarget::Near {
                reference,
                distance,
            }
        } else {
            self.diagnostics.push(
                papan_types::Diagnostic::error(
                    "PARSE_104",
                    format!("invalid placement target '{keyword}'"),
                )
                .with_span(self.previous().span),
            );
            self.recover_line();
            return None;
        };
        Some(Item::Place {
            component,
            target,
            span: self.span_from(start),
        })
    }

    fn parse_rotate(&mut self) -> Option<Item> {
        let start = self.bump().span.start;
        let (component, _) = self.expect_ident("a component designator")?;
        let angle = self.parse_quantity()?;
        Some(Item::Rotate {
            component,
            angle,
            span: self.span_from(start),
        })
    }

    fn parse_route(&mut self) -> Option<Item> {
        let start = self.bump().span.start;
        let (net, _) = self.expect_ident("a net name")?;
        let properties = if self.peek_kind() == Some(&TokenKind::LBrace) {
            self.parse_block().properties
        } else {
            Vec::new()
        };
        Some(Item::Route(RouteDecl {
            net,
            properties,
            span: self.span_from(start),
        }))
    }

    fn parse_via(&mut self) -> Option<Item> {
        let start = self.bump().span.start;
        let properties = if self.peek_kind() == Some(&TokenKind::LBrace) {
            self.parse_block().properties
        } else {
            Vec::new()
        };
        Some(Item::Via(ViaDecl {
            properties,
            span: self.span_from(start),
        }))
    }

    fn parse_constraints(&mut self) -> Option<Item> {
        let start = self.bump().span.start;
        let block = self.parse_block();
        Some(Item::Constraints(Block {
            properties: block.properties,
            span: Span::new(start, block.span.end),
        }))
    }

    fn parse_check(&mut self) -> Option<Item> {
        let start = self.bump().span.start;
        let (rule, _) = self.expect_ident("a check name")?;
        Some(Item::Check {
            rule,
            span: self.span_from(start),
        })
    }

    fn parse_export(&mut self) -> Option<Item> {
        let start = self.bump().span.start;
        let target = match self.peek().kind.clone() {
            TokenKind::Ident(value) => {
                self.bump();
                value
            }
            TokenKind::Str(value) => {
                self.bump();
                value
            }
            _ => {
                self.expected("an export target");
                self.recover_line();
                return None;
            }
        };
        Some(Item::Export {
            target,
            span: self.span_from(start),
        })
    }

    fn parse_module(&mut self) -> Option<Item> {
        let start = self.bump().span.start;
        let (name, _) = self.expect_ident("a module name")?;
        if self.peek_kind() != Some(&TokenKind::LBrace) {
            self.expected("'{' after module name");
            self.recover_line();
            return None;
        }
        let block_start = self.bump().span.start;
        let mut items = Vec::new();
        loop {
            self.skip_separators();
            if self.at_eof() {
                self.diagnostics.push(
                    papan_types::Diagnostic::error("PARSE_102", "unterminated module block")
                        .with_span(Span::new(block_start, self.peek().span.end)),
                );
                break;
            }
            if self.at_rbrace() {
                self.bump();
                break;
            }
            if let Some(item) = self.parse_item() {
                items.push(item);
            }
        }
        Some(Item::Module(ModuleDecl {
            name,
            items,
            span: self.span_from(start),
        }))
    }

    fn parse_module_port(&mut self, direction: &str) -> Option<Item> {
        let start = self.bump().span.start;
        let (name, _) = self.expect_ident("a module port name")?;
        let value = if self.peek_kind() == Some(&TokenKind::Colon) {
            self.bump();
            self.parse_value()
        } else {
            None
        };
        Some(Item::ModulePort {
            direction: direction.to_string(),
            name,
            value,
            span: self.span_from(start),
        })
    }

    fn parse_block(&mut self) -> Block {
        let start = self.peek().span.start;
        if self.peek_kind() != Some(&TokenKind::LBrace) {
            self.expected("'{'");
            return Block {
                properties: Vec::new(),
                span: self.peek().span,
            };
        }
        self.bump();
        let mut properties = Vec::new();
        loop {
            self.skip_separators();
            if self.at_eof() {
                self.diagnostics.push(
                    papan_types::Diagnostic::error("PARSE_102", "unterminated block")
                        .with_span(Span::new(start, self.peek().span.end)),
                );
                break;
            }
            if self.at_rbrace() {
                self.bump();
                break;
            }
            let key_token = self.peek().clone();
            let key = match key_token.kind.clone() {
                TokenKind::Ident(value) => {
                    self.bump();
                    value
                }
                _ => {
                    self.expected("a property name");
                    if !self.at_rbrace() && !self.at_eof() {
                        self.bump();
                    }
                    self.recover_line();
                    continue;
                }
            };
            if self.peek_kind() != Some(&TokenKind::Colon) {
                self.expected("':' after property name");
                self.recover_line();
                continue;
            }
            self.bump();
            let value = match self.parse_value() {
                Some(value) => value,
                None => {
                    self.recover_line();
                    continue;
                }
            };
            properties.push(Property {
                key,
                value,
                span: Span::new(key_token.span.start, self.previous().span.end),
            });
        }
        Block {
            properties,
            span: self.span_from(start),
        }
    }

    fn parse_value(&mut self) -> Option<Value> {
        let token = self.peek().clone();
        match token.kind.clone() {
            TokenKind::Number(raw) => {
                self.bump();
                let value = match Quantity::parse_literal(&raw) {
                    Ok(quantity) => Some(quantity),
                    Err(error) => {
                        self.diagnostics.push(
                            papan_types::Diagnostic::error(
                                "PARSE_101",
                                error.to_string(),
                            )
                            .with_span(token.span)
                            .with_suggestion("use a value with a known unit, e.g. 0.25mm or 10kΩ"),
                        );
                        None
                    }
                };
                Some(Value::Quantity(QuantityLit::new(raw, value, token.span)))
            }
            TokenKind::Str(value) => {
                self.bump();
                Some(Value::String(value))
            }
            TokenKind::Ident(value) => {
                self.bump();
                if value.eq_ignore_ascii_case("true") {
                    Some(Value::Bool(true))
                } else if value.eq_ignore_ascii_case("false") {
                    Some(Value::Bool(false))
                } else {
                    let mut value = value;
                    while self.peek_kind() == Some(&TokenKind::Dot) {
                        self.bump();
                        match self.expect_ident("an identifier after '.'") {
                            Some((part, _)) => {
                                value.push('.');
                                value.push_str(&part);
                            }
                            None => break,
                        }
                    }
                    Some(Value::Ident(value))
                }
            }
            TokenKind::LBracket => {
                self.bump();
                let mut values = Vec::new();
                loop {
                    self.skip_separators();
                    if self.at_eof() || self.at_rbrace() {
                        self.expected("']' to close array");
                        break;
                    }
                    if self.peek_kind() == Some(&TokenKind::RBracket) {
                        self.bump();
                        break;
                    }
                    match self.parse_value() {
                        Some(value) => values.push(value),
                        None => break,
                    }
                }
                Some(Value::Array(values))
            }
            _ => {
                self.expected("a property value");
                None
            }
        }
    }

    fn parse_quantity(&mut self) -> Option<QuantityLit> {
        let token = self.peek().clone();
        match token.kind.clone() {
            TokenKind::Number(raw) => {
                self.bump();
                let value = match Quantity::parse_literal(&raw) {
                    Ok(quantity) => Some(quantity),
                    Err(error) => {
                        self.diagnostics.push(
                            papan_types::Diagnostic::error(
                                "PARSE_101",
                                error.to_string(),
                            )
                            .with_span(token.span)
                            .with_suggestion("use a value with a known unit, e.g. 0.25mm"),
                        );
                        None
                    }
                };
                Some(QuantityLit::new(raw, value, token.span))
            }
            _ => {
                self.expected("a numeric quantity");
                self.recover_line();
                None
            }
        }
    }

    fn parse_path(&mut self) -> Option<Vec<String>> {
        let (first, _) = self.expect_ident("an identifier")?;
        let mut path = vec![first];
        loop {
            match self.peek_kind().cloned() {
                Some(TokenKind::Dot) => {
                    self.bump();
                    match self.expect_ident("an identifier after '.'") {
                        Some((part, _)) => path.push(part),
                        None => break,
                    }
                }
                // `R1.1` lexes the `.1` suffix as a numeric token because a dot
                // followed by a digit starts a decimal quantity. Paths still
                // need to accept numeric pin names, so split the suffix here.
                Some(TokenKind::Number(raw)) if raw.starts_with('.') => {
                    self.bump();
                    let part = raw.trim_start_matches('.').to_string();
                    if part.is_empty() {
                        break;
                    }
                    path.push(part);
                }
                _ => break,
            }
        }
        Some(path)
    }

    fn expect_ident(&mut self, what: &str) -> Option<(String, Span)> {
        let token = self.peek().clone();
        match token.kind {
            TokenKind::Ident(value) => {
                self.bump();
                Some((value, token.span))
            }
            _ => {
                self.expected(what);
                None
            }
        }
    }

    fn expected(&mut self, what: &str) {
        self.diagnostics.push(
            papan_types::Diagnostic::error(
                "PARSE_001",
                format!("expected {what}, found {}", self.peek().kind.describe()),
            )
            .with_span(self.peek().span),
        );
    }

    fn span_from(&self, start: usize) -> Span {
        Span::new(start, self.previous().span.end.max(start))
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.index.min(self.tokens.len().saturating_sub(1))]
    }

    fn peek_kind(&self) -> Option<&TokenKind> {
        Some(&self.peek().kind)
    }

    fn previous(&self) -> &Token {
        &self.tokens[self.index.saturating_sub(1).min(self.tokens.len() - 1)]
    }

    fn bump(&mut self) -> Token {
        let token = self.peek().clone();
        if self.index + 1 < self.tokens.len() {
            self.index += 1;
        }
        token
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Eof)
    }

    fn at_rbrace(&self) -> bool {
        matches!(self.peek().kind, TokenKind::RBrace)
    }

    fn at_keyword(&self, keyword: &str) -> bool {
        matches!(&self.peek().kind, TokenKind::Ident(value) if value.eq_ignore_ascii_case(keyword))
    }

    fn skip_separators(&mut self) {
        while matches!(
            self.peek().kind,
            TokenKind::Newline | TokenKind::Comma | TokenKind::Semicolon
        ) {
            self.bump();
        }
    }

    fn recover_line(&mut self) {
        while !self.at_eof() && self.peek_kind() != Some(&TokenKind::Newline) && !self.at_rbrace() {
            self.bump();
        }
        if self.peek_kind() == Some(&TokenKind::Newline) {
            self.bump();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use papan_ast::ComponentKind;

    #[test]
    fn parses_prd_example_shape() {
        let source = r#"
board "Mechanical Keyboard"

size 300mm x 120mm
thickness 1.6mm
layers 4

component MCU: "ATmega32U4"
component USB: "USB-C"
component R1: resistor {
    value: 10kΩ
    tolerance: 1%
    package: "0603"
}

net VCC { voltage: 5V type: power }
net GND { type: ground }

connect MCU.VCC -> VCC
connect MCU.GND -> GND

place MCU at (150mm, 60mm)
place USB at (150mm, 10mm)
place R1 near MCU.VCC distance 3mm
rotate MCU 90deg

route USB_DP { width: 0.15mm layer: F.Cu }
via { drill: 0.30mm diameter: 0.60mm }
constraints { min_trace_width: 0.15mm clearance: 0.20mm }

check DRC
check ERC
export gerber
"#;
        let output = parse(source);
        assert!(
            !output.diagnostics.has_errors(),
            "{:?}",
            output.diagnostics
        );
        assert_eq!(output.program.board.unwrap().name, "Mechanical Keyboard");
        assert!(output.program.items.len() >= 15);
        let component = output
            .program
            .items
            .iter()
            .find_map(|item| match item {
                Item::Component(component) if component.name == "R1" => Some(component),
                _ => None,
            })
            .unwrap();
        assert_eq!(component.kind, ComponentKind::Named("resistor".into()));
        assert_eq!(component.properties.len(), 3);
    }

    #[test]
    fn recovers_from_invalid_statement() {
        let output = parse("board \"x\"\nwat 42\nsize 10mm x 20mm\n");
        assert!(output.diagnostics.has_errors());
        assert!(output.program.items.iter().any(|item| matches!(item, Item::Size { .. })));
    }

    #[test]
    fn reports_invalid_quantity() {
        let output = parse("board \"x\"\nsize 10florps x 20mm\n");
        assert!(output.diagnostics.iter().any(|d| d.code == "PARSE_101"));
    }

    #[test]
    fn parses_numeric_pin_paths() {
        let output = parse("board \"x\"\nnet VCC\nconnect R1.1 -> VCC\nconnect U2.12 -> VCC\n");
        assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
        let paths: Vec<_> = output
            .program
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Connect { from, .. } => Some(from.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            paths,
            vec![
                vec!["R1".to_string(), "1".to_string()],
                vec!["U2".to_string(), "12".to_string()]
            ]
        );
    }

    #[test]
    fn parses_board_block_and_module() {
        let source = r#"
board "Nested" {
    size 50mm x 40mm
    module Power {
        input voltage: 5V
        output power: VCC
        component C1: capacitor 10uF
    }
}
"#;
        let output = parse(source);
        assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
        assert!(output
            .program
            .items
            .iter()
            .any(|item| matches!(item, Item::Module(_))));
    }
}
