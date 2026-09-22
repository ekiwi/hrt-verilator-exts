use std::{collections::BTreeMap, fmt};

use crate::{
    ast::{
        DataType, DataTypeId, DataTypeKind, DataTypeMember, EnumVariant, UnpackedLayout,
        range::Range,
    },
    document::AstNode,
    parser::{ParseError, expression::parse_literal, source_info},
};

pub(super) struct DataTypeResolver<'a> {
    nodes_by_addr: &'a BTreeMap<String, &'a AstNode>,
    ids: BTreeMap<String, DataTypeId>,
    data_types: Vec<DataType>,
    resolving: Vec<String>,
}

impl<'a> DataTypeResolver<'a> {
    pub(super) fn new(nodes_by_addr: &'a BTreeMap<String, &'a AstNode>) -> Self {
        Self {
            nodes_by_addr,
            ids: BTreeMap::new(),
            data_types: Vec::new(),
            resolving: Vec::new(),
        }
    }

    pub(super) fn finish(self) -> (BTreeMap<String, DataTypeId>, Vec<DataType>) {
        (self.ids, self.data_types)
    }

    pub(super) fn resolve_supported_declarations(&mut self) {
        let addresses = self
            .nodes_by_addr
            .values()
            .filter(|node| is_supported_dtype(&node.node_type))
            .filter_map(|node| node.addr.clone())
            .collect::<Vec<_>>();
        for address in addresses {
            let origin = self.nodes_by_addr[&address];
            let _ = self.resolve(&address, origin);
        }
    }

    pub(super) fn resolve(
        &mut self,
        address: &str,
        origin: &AstNode,
    ) -> Result<DataTypeId, ParseError> {
        if let Some(id) = self.ids.get(address) {
            return Ok(*id);
        }
        if (&self.resolving)
            .into_iter()
            .any(|active| active == address)
        {
            let mut chain = self.resolving.clone();
            chain.push(address.to_string());
            return Err(ParseError::node(
                origin,
                format!("dtype cycle: {}", chain.join(" -> ")),
            ));
        }
        let node = self.nodes_by_addr.get(address).copied().ok_or_else(|| {
            self.resolution_error(origin, format!("missing dtype address {address}"))
        })?;
        self.resolving.push(address.to_string());
        let result = self.resolve_node(node, origin);
        self.resolving.pop();
        let dtype = result?;
        if dtype.width == 0 {
            return Err(self.resolution_error(origin, "dtype has zero width"));
        }
        let id = DataTypeId(self.data_types.len());
        self.data_types.push(dtype);
        self.ids.insert(address.to_string(), id);
        Ok(id)
    }

    fn resolve_node(&mut self, node: &AstNode, origin: &AstNode) -> Result<DataType, ParseError> {
        match node.node_type.as_str() {
            "BASICDTYPE" => self.resolve_basic(node, origin),
            "REFDTYPE" | "PARAMTYPEDTYPE" | "CONSTDTYPE" | "DEFIMPLICITDTYPE" | "MEMBERDTYPE" => {
                self.resolve_alias(node, origin)
            }
            "ENUMDTYPE" => self.resolve_enum(node, origin),
            "PACKARRAYDTYPE" => self.resolve_array(node, origin, true),
            "UNPACKARRAYDTYPE" => self.resolve_array(node, origin, false),
            "STRUCTDTYPE" => self.resolve_struct(node, origin),
            "UNIONDTYPE" => self.resolve_union(node, origin),
            "ASSOCARRAYDTYPE"
            | "BRACKETARRAYDTYPE"
            | "CDTYPE"
            | "CLASSREFDTYPE"
            | "CONSTRAINTREFDTYPE"
            | "DYNARRAYDTYPE"
            | "EMPTYQUEUEDTYPE"
            | "IFACEGENERICDTYPE"
            | "IFACEREFDTYPE"
            | "NBACOMMITQUEUEDTYPE"
            | "PARSETYPEDTYPE"
            | "QUEUEDTYPE"
            | "REQUIREDTYPE"
            | "SAMPLEQUEUEDTYPE"
            | "STREAMDTYPE"
            | "UNSIZEDARRAYDTYPE"
            | "VOIDDTYPE"
            | "WILDCARDARRAYDTYPE" => Err(self.resolution_error(
                origin,
                format!("referenced runtime-only dtype {}", node.node_type),
            )),
            _ => {
                Err(self
                    .resolution_error(origin, format!("unsupported dtype node {}", node.node_type)))
            }
        }
    }

    fn resolve_basic(&self, node: &AstNode, origin: &AstNode) -> Result<DataType, ParseError> {
        let packed = if let Some(range) = node.string("range") {
            parse_range(node, range)?
        } else {
            let width = match node.string("keyword") {
                Some("bit" | "logic" | "reg" | "wire") => 1,
                Some("byte") => 8,
                Some("shortint") => 16,
                Some("int" | "integer") => 32,
                Some("longint" | "time") => 64,
                //
                Some("string") => {
                    return Err(
                        self.resolution_error(origin, "TODO: deal with variable length strings")
                    );
                }
                Some(keyword) => {
                    return Err(self.resolution_error(
                        origin,
                        format!("non-bit-vector BASICDTYPE keyword {keyword}"),
                    ));
                }
                None => 1,
            };
            Range {
                left: isize::try_from(width - 1).unwrap(),
                right: 0,
            }
        };
        let width = checked_range_len(node, packed)?;
        Ok(DataType {
            source: source_info(node),
            name: node.name.clone(),
            width,
            indices: packed.swap(),
            signed: node.boolean("signed"),
            unpacked: None,
            kind: DataTypeKind::Basic { packed },
        })
    }

    fn resolve_alias(&mut self, node: &AstNode, origin: &AstNode) -> Result<DataType, ParseError> {
        let target_address = dtype_target(node).ok_or_else(|| {
            self.resolution_error(origin, format!("{} has no dtype target", node.node_type))
        })?;
        let target = self.resolve(target_address, origin)?;
        let layout = &self.data_types[target.0];
        Ok(DataType {
            source: source_info(node),
            name: node.name.clone(),
            width: layout.width,
            indices: layout.indices,
            signed: layout.signed,
            unpacked: layout.unpacked.clone(),
            kind: DataTypeKind::Alias { target },
        })
    }

    fn resolve_enum(&mut self, node: &AstNode, origin: &AstNode) -> Result<DataType, ParseError> {
        let base_address = dtype_target(node)
            .ok_or_else(|| self.resolution_error(origin, "ENUMDTYPE has no base dtype"))?;
        let base = self.resolve(base_address, origin)?;
        let layout = self.data_types[base.0].clone();
        let mut variants = Vec::new();
        for item in node.children("itemsp") {
            let value = item
                .child("valuep")
                .ok_or_else(|| ParseError::node(item, "enum item has no valuep"))?;
            variants.push(EnumVariant {
                source: source_info(item),
                name: item
                    .name
                    .clone()
                    .ok_or_else(|| ParseError::node(item, "enum item has no name"))?,
                value: parse_literal(value)?,
            });
        }
        Ok(DataType {
            source: source_info(node),
            name: node.name.clone(),
            width: layout.width,
            indices: layout.indices,
            signed: layout.signed,
            unpacked: layout.unpacked,
            kind: DataTypeKind::Enum { base, variants },
        })
    }

    fn resolve_array(
        &mut self,
        node: &AstNode,
        origin: &AstNode,
        packed: bool,
    ) -> Result<DataType, ParseError> {
        let element_address = dtype_target(node).ok_or_else(|| {
            self.resolution_error(origin, format!("{} has no element dtype", node.node_type))
        })?;
        let element = self.resolve(element_address, origin)?;
        let declared = parse_decl_range(node)?;
        if !packed && (declared.left < 0 || declared.right < 0) {
            return Err(
                self.resolution_error(origin, "negative unpacked array indices are unsupported")
            );
        }
        let dimension = checked_range_len(node, declared)?;
        let element_layout = &self.data_types[element.0];
        let width = element_layout
            .width
            .checked_mul(dimension)
            .ok_or_else(|| self.resolution_error(origin, "array width exceeds usize"))?;
        let indices = flat_indices(node, width)?;
        let (signed, unpacked, kind) = if packed {
            (
                node.boolean("signed"),
                None,
                DataTypeKind::PackedArray { element, declared },
            )
        } else {
            (
                false,
                Some(UnpackedLayout {
                    element_width: element_layout.width,
                    indices: declared,
                }),
                DataTypeKind::UnpackedArray { element, declared },
            )
        };
        Ok(DataType {
            source: source_info(node),
            name: node.name.clone(),
            width,
            indices,
            signed,
            unpacked,
            kind,
        })
    }

    fn resolve_struct(&mut self, node: &AstNode, origin: &AstNode) -> Result<DataType, ParseError> {
        if !node.boolean("packed") {
            return Err(self.resolution_error(origin, "unpacked structs are unsupported"));
        }
        let member_nodes = node.children("membersp");
        let mut resolved = Vec::with_capacity(member_nodes.len());
        let mut width = 0usize;
        for member in &member_nodes {
            let address = member.addr.as_deref().ok_or_else(|| {
                self.resolution_error(origin, "struct member dtype has no address")
            })?;
            let dtype = self.resolve(address, origin)?;
            let member_width = self.data_types[dtype.0].width;
            width = width.checked_add(member_width).ok_or_else(|| {
                self.resolution_error(origin, "packed struct width exceeds usize")
            })?;
            resolved.push((member, dtype, member_width));
        }
        let mut lsb = 0usize;
        let mut members = Vec::with_capacity(resolved.len());
        for (member, dtype, member_width) in resolved.into_iter().rev() {
            members.push(DataTypeMember {
                source: source_info(member),
                name: member.name.clone().unwrap_or_default(),
                dtype,
                width: member_width,
                lsb,
            });
            lsb = lsb.checked_add(member_width).ok_or_else(|| {
                self.resolution_error(origin, "packed struct member offset exceeds usize")
            })?;
        }
        members.reverse();
        Ok(DataType {
            source: source_info(node),
            name: node.name.clone(),
            width,
            indices: flat_indices(node, width)?,
            signed: node.boolean("signed"),
            unpacked: None,
            kind: DataTypeKind::PackedStruct { members },
        })
    }

    fn resolve_union(&mut self, node: &AstNode, origin: &AstNode) -> Result<DataType, ParseError> {
        if !node.boolean("packed") {
            return Err(self.resolution_error(origin, "unpacked unions are unsupported"));
        }
        if node.boolean("isTagged") || node.boolean("tagged") {
            return Err(self.resolution_error(origin, "tagged unions are unsupported"));
        }
        let mut width = None;
        let mut members = Vec::new();
        for member in node.children("membersp") {
            let address = member.addr.as_deref().ok_or_else(|| {
                self.resolution_error(origin, "union member dtype has no address")
            })?;
            let dtype = self.resolve(address, origin)?;
            let member_width = self.data_types[dtype.0].width;
            if width.is_some_and(|expected| expected != member_width) {
                return Err(self.resolution_error(
                    origin,
                    format!(
                        "packed union members have unequal widths: {width:?} and {member_width}"
                    ),
                ));
            }
            width = Some(member_width);
            members.push(DataTypeMember {
                source: source_info(member),
                name: member.name.clone().unwrap_or_default(),
                dtype,
                width: member_width,
                lsb: 0,
            });
        }
        let width = width.ok_or_else(|| self.resolution_error(origin, "packed union is empty"))?;
        Ok(DataType {
            source: source_info(node),
            name: node.name.clone(),
            width,
            indices: flat_indices(node, width)?,
            signed: node.boolean("signed"),
            unpacked: None,
            kind: DataTypeKind::PackedUnion { members },
        })
    }

    fn resolution_error(&self, origin: &AstNode, message: impl fmt::Display) -> ParseError {
        let chain = if self.resolving.is_empty() {
            "none".to_string()
        } else {
            self.resolving.join(" -> ")
        };
        ParseError::node(
            origin,
            format!("{message}; dtype dependency chain: {chain}"),
        )
    }
}

fn is_supported_dtype(node_type: &str) -> bool {
    match node_type {
        "BASICDTYPE" | "REFDTYPE" | "PARAMTYPEDTYPE" | "CONSTDTYPE" | "DEFIMPLICITDTYPE"
        | "MEMBERDTYPE" | "ENUMDTYPE" | "PACKARRAYDTYPE" | "UNPACKARRAYDTYPE" | "STRUCTDTYPE"
        | "UNIONDTYPE" => true,
        _ => false,
    }
}

fn dtype_target(node: &AstNode) -> Option<&str> {
    let address = node.addr.as_deref();
    node.string("refDTypep")
        .filter(|target| Some(*target) != address && *target != "UNLINKED")
        .or_else(|| {
            node.string("dtypep")
                .filter(|target| Some(*target) != address && *target != "UNLINKED")
        })
        .or_else(|| {
            node.child("childDTypep")
                .and_then(|child| child.addr.as_deref())
        })
}

fn checked_range_len(node: &AstNode, range: Range) -> Result<usize, ParseError> {
    let distance = range.left.abs_diff(range.right);
    let length = distance
        .checked_add(1)
        .ok_or_else(|| ParseError::node(node, "range length exceeds usize"))?;
    Ok(length)
}

fn parse_decl_range(node: &AstNode) -> Result<Range, ParseError> {
    let text = node
        .string("declRange")
        .ok_or_else(|| ParseError::node(node, "array dtype has no declRange"))?;
    let range = text
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .ok_or_else(|| ParseError::node(node, format!("invalid range {text}")))?;
    parse_range(node, range)
}

fn flat_indices(node: &AstNode, width: usize) -> Result<Range, ParseError> {
    let last = width
        .checked_sub(1)
        .ok_or_else(|| ParseError::node(node, "dtype has zero width"))?;
    Ok(Range {
        left: 0,
        right: isize::try_from(last)
            .map_err(|_| ParseError::node(node, "dtype width exceeds isize"))?,
    })
}

fn parse_range(node: &AstNode, range: &str) -> Result<Range, ParseError> {
    let (left, right) = range
        .split_once(':')
        .ok_or_else(|| ParseError::node(node, format!("invalid range {range}")))?;
    let left = left
        .parse::<isize>()
        .map_err(|_| ParseError::node(node, format!("invalid range {range}")))?;
    let right = right
        .parse::<isize>()
        .map_err(|_| ParseError::node(node, format!("invalid range {range}")))?;
    Ok(Range { left, right })
}
