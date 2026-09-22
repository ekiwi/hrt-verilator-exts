use std::{
    collections::BTreeMap,
    error::Error,
    fs::File,
    io::{self, BufReader, Read},
    path::Path,
};

use serde::{Deserialize, Deserializer, de::Error as _};
use serde_json::{Number, Value};

/// A complete Verilator AST dump.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(transparent)]
pub struct AstDocument {
    pub root: AstNode,
}

/// A node in Verilator's AST.
///
/// Verilator's JSON schema evolves with its internal AST. Common fields are
/// typed explicitly, while node-specific fields remain available in `fields`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AstNode {
    #[serde(rename = "type")]
    pub node_type: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub addr: Option<String>,
    #[serde(default)]
    pub loc: Option<String>,
    #[serde(flatten)]
    pub fields: BTreeMap<String, AstValue>,
}

/// A node-specific JSON value in a Verilator AST dump.
///
/// Objects containing a `type` field deserialize as [`AstNode`] values, so
/// child nodes stay typed and can be traversed without inspecting raw JSON.
#[derive(Debug, Clone, PartialEq)]
pub enum AstValue {
    Node(AstNode),
    Array(Vec<AstValue>),
    Object(BTreeMap<String, AstValue>),
    String(String),
    Bool(bool),
    Number(Number),
    Null,
}

impl<'de> Deserialize<'de> for AstValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match Value::deserialize(deserializer)? {
            Value::Object(fields) if fields.contains_key("type") => {
                serde_json::from_value(Value::Object(fields))
                    .map(Self::Node)
                    .map_err(D::Error::custom)
            }
            Value::Object(fields) => fields
                .into_iter()
                .map(|(name, value)| {
                    serde_json::from_value(value)
                        .map(|value| (name, value))
                        .map_err(D::Error::custom)
                })
                .collect::<Result<BTreeMap<_, _>, _>>()
                .map(Self::Object),
            Value::Array(values) => values
                .into_iter()
                .map(|value| serde_json::from_value(value).map_err(D::Error::custom))
                .collect::<Result<Vec<_>, _>>()
                .map(Self::Array),
            Value::String(value) => Ok(Self::String(value)),
            Value::Bool(value) => Ok(Self::Bool(value)),
            Value::Number(value) => Ok(Self::Number(value)),
            Value::Null => Ok(Self::Null),
        }
    }
}

#[derive(Debug)]
pub enum LoadError {
    Io(io::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "could not read AST JSON: {error}"),
            Self::Json(error) => write!(formatter, "could not parse AST JSON: {error}"),
        }
    }
}

impl Error for LoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
        }
    }
}

impl From<io::Error> for LoadError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for LoadError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl AstDocument {
    pub fn from_reader(reader: impl Read) -> Result<Self, serde_json::Error> {
        let mut de = serde_json::Deserializer::from_reader(reader);
        de.disable_recursion_limit();
        let value: Self = serde::Deserialize::deserialize(&mut de)?;
        de.end()?;
        Ok(value)
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, LoadError> {
        let file = File::open(path)?;
        Ok(Self::from_reader(BufReader::new(file))?)
    }

    pub fn node_count(&self) -> usize {
        let mut count = 0;
        self.visit_nodes(&mut |_| count += 1);
        count
    }

    pub fn node_type_counts(&self) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();
        self.visit_nodes(&mut |node| {
            *counts.entry(node.node_type.clone()).or_default() += 1;
        });
        counts
    }

    pub fn visit_nodes(&self, visitor: &mut impl FnMut(&AstNode)) {
        self.root.visit_nodes(visitor);
    }

    pub fn nodes(&self) -> Vec<&AstNode> {
        let mut nodes = Vec::new();
        self.root.collect_nodes(&mut nodes);
        nodes
    }
}

impl AstNode {
    pub fn field(&self, name: &str) -> Option<&AstValue> {
        self.fields.get(name)
    }

    pub fn string(&self, name: &str) -> Option<&str> {
        match name {
            "name" => return self.name.as_deref(),
            "addr" => return self.addr.as_deref(),
            "loc" => return self.loc.as_deref(),
            _ => {}
        }
        match self.field(name) {
            Some(AstValue::String(value)) => Some(value),
            _ => None,
        }
    }

    pub fn boolean(&self, name: &str) -> bool {
        self.field(name) == Some(&AstValue::Bool(true))
    }

    pub fn child(&self, name: &str) -> Option<&AstNode> {
        self.children(name).into_iter().next()
    }

    pub fn children(&self, name: &str) -> Vec<&AstNode> {
        match self.field(name) {
            Some(AstValue::Node(node)) => vec![node],
            Some(AstValue::Array(values)) => values
                .into_iter()
                .filter_map(|value| match value {
                    AstValue::Node(node) => Some(node),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    fn collect_nodes<'a>(&'a self, nodes: &mut Vec<&'a AstNode>) {
        nodes.push(self);
        for value in self.fields.values() {
            value.collect_nodes(nodes);
        }
    }
}

trait VisitNodes {
    fn visit_nodes(&self, visitor: &mut impl FnMut(&AstNode));
}

impl VisitNodes for AstNode {
    fn visit_nodes(&self, visitor: &mut impl FnMut(&AstNode)) {
        visitor(self);
        for value in self.fields.values() {
            value.visit_nodes(visitor);
        }
    }
}

impl VisitNodes for AstValue {
    fn visit_nodes(&self, visitor: &mut impl FnMut(&AstNode)) {
        match self {
            Self::Node(node) => node.visit_nodes(visitor),
            Self::Array(values) => {
                for value in values {
                    value.visit_nodes(visitor);
                }
            }
            Self::Object(fields) => {
                for value in fields.values() {
                    value.visit_nodes(visitor);
                }
            }
            Self::String(_) | Self::Bool(_) | Self::Number(_) | Self::Null => {}
        }
    }
}

impl AstValue {
    fn collect_nodes<'a>(&'a self, nodes: &mut Vec<&'a AstNode>) {
        match self {
            Self::Node(node) => node.collect_nodes(nodes),
            Self::Array(values) => {
                for value in values {
                    value.collect_nodes(nodes);
                }
            }
            Self::Object(fields) => {
                for value in fields.values() {
                    value.collect_nodes(nodes);
                }
            }
            Self::String(_) | Self::Bool(_) | Self::Number(_) | Self::Null => {}
        }
    }
}
