//! AST for the PAPAN declarative language.
//!
//! The AST is deliberately independent of the PCB IR. Semantic analysis lowers
//! it into the document model and IR, resolving IDs, types and references.

use papan_types::{Quantity, Span};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Program {
    pub board: Option<BoardDecl>,
    pub items: Vec<Item>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoardDecl {
    pub name: String,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QuantityLit {
    pub raw: String,
    pub value: Option<Quantity>,
    pub span: Span,
}

impl QuantityLit {
    pub fn new(raw: impl Into<String>, value: Option<Quantity>, span: Span) -> Self {
        Self {
            raw: raw.into(),
            value,
            span,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ComponentKind {
    /// Built-in named kind, e.g. `resistor` or `capacitor`.
    Named(String),
    /// Library part name, e.g. `"ATmega32U4"`.
    Part(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ComponentDecl {
    pub name: String,
    pub kind: ComponentKind,
    pub value: Option<QuantityLit>,
    pub properties: Vec<Property>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NetDecl {
    pub name: String,
    pub properties: Vec<Property>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Property {
    pub key: String,
    pub value: Value,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Quantity(QuantityLit),
    String(String),
    Ident(String),
    Bool(bool),
    Array(Vec<Value>),
}

impl Value {
    pub fn as_ident(&self) -> Option<&str> {
        match self {
            Value::Ident(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_string(&self) -> Option<&str> {
        match self {
            Value::String(value) => Some(value),
            Value::Ident(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_quantity(&self) -> Option<Quantity> {
        match self {
            Value::Quantity(literal) => literal.value,
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PlaceTarget {
    At {
        x: QuantityLit,
        y: QuantityLit,
    },
    Anchor(Vec<String>),
    Near {
        reference: Vec<String>,
        distance: QuantityLit,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RouteDecl {
    pub net: String,
    pub properties: Vec<Property>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViaDecl {
    pub properties: Vec<Property>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Block {
    pub properties: Vec<Property>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModuleDecl {
    pub name: String,
    pub items: Vec<Item>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Item {
    Size {
        width: QuantityLit,
        height: QuantityLit,
        span: Span,
    },
    Thickness {
        value: QuantityLit,
        span: Span,
    },
    Layers {
        count: u32,
        span: Span,
    },
    Component(ComponentDecl),
    Net(NetDecl),
    Connect {
        from: Vec<String>,
        to: Vec<String>,
        span: Span,
    },
    Place {
        component: String,
        target: PlaceTarget,
        span: Span,
    },
    Rotate {
        component: String,
        angle: QuantityLit,
        span: Span,
    },
    Route(RouteDecl),
    Via(ViaDecl),
    Constraints(Block),
    Check {
        rule: String,
        span: Span,
    },
    Export {
        target: String,
        span: Span,
    },
    Module(ModuleDecl),
    ModulePort {
        direction: String,
        name: String,
        value: Option<Value>,
        span: Span,
    },
}

impl Item {
    pub fn span(&self) -> Span {
        match self {
            Item::Size { span, .. }
            | Item::Thickness { span, .. }
            | Item::Layers { span, .. }
            | Item::Connect { span, .. }
            | Item::Place { span, .. }
            | Item::Rotate { span, .. }
            | Item::Constraints(Block { span, .. })
            | Item::Check { span, .. }
            | Item::Export { span, .. } => *span,
            Item::Component(decl) => decl.span,
            Item::Net(decl) => decl.span,
            Item::Route(decl) => decl.span,
            Item::Via(decl) => decl.span,
            Item::Module(decl) => decl.span,
            Item::ModulePort { span, .. } => *span,
        }
    }
}
