// -*- coding: utf-8 -*-
// ------------------------------------------------------------------------------------------------
// Copyright © 2021, tree-sitter authors.
// Licensed under either of Apache License, Version 2.0, or MIT license, at your option.
// Please see the LICENSE-APACHE or LICENSE-MIT files in this distribution for license details.
// ------------------------------------------------------------------------------------------------

//! Defines data types for the graphs produced by the graph DSL

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::fmt;
use std::ops::Index;
use std::ops::IndexMut;

use smallvec::SmallVec;
use tree_sitter::Node;

use crate::execution::ExecutionError;
use crate::Context;
use crate::Identifier;

/// A graph produced by executing a graph DSL file.  Graphs include a lifetime parameter to ensure
/// that they don't outlive the tree-sitter syntax tree that they are generated from.
#[derive(Default)]
pub struct Graph<'tree> {
    syntax_nodes: HashMap<SyntaxNodeRef, Node<'tree>>,
    graph_nodes: Vec<GraphNode>,
}

type SyntaxNodeID = u32;
type GraphNodeID = u32;

impl<'tree> Graph<'tree> {
    /// Creates a new, empty graph.
    pub fn new() -> Graph<'tree> {
        Graph::default()
    }

    /// Adds a syntax node to the graph, returning a graph DSL reference to it.
    ///
    /// The graph won't contain _every_ syntax node in the parsed syntax tree; it will only contain
    /// those nodes that are referenced at some point during the execution of the graph DSL file.
    pub fn add_syntax_node(&mut self, node: Node<'tree>) -> SyntaxNodeRef {
        let index = SyntaxNodeRef(node.id() as SyntaxNodeID);
        self.syntax_nodes.insert(index, node);
        index
    }

    /// Adds a new graph node to the graph, returning a graph DSL reference to it.
    pub fn add_graph_node(&mut self) -> GraphNodeRef {
        let graph_node = GraphNode::new();
        let index = self.graph_nodes.len() as GraphNodeID;
        self.graph_nodes.push(graph_node);
        GraphNodeRef(index)
    }

    /// fmt::Displays the contents of this graph.
    pub fn display_with<'a>(&'a self, ctx: &'a Context) -> impl fmt::Display + 'a {
        struct DisplayGraph<'a, 'tree>(&'a Graph<'tree>, &'a Context);

        impl<'a, 'tree> fmt::Display for DisplayGraph<'a, 'tree> {
            fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                let graph = self.0;
                let ctx = self.1;
                for (node_index, node) in graph.graph_nodes.iter().enumerate() {
                    write!(
                        f,
                        "node {}\n{}",
                        node_index,
                        node.attributes.display_with(ctx, graph)
                    )?;
                    for (sink, edge) in &node.outgoing_edges {
                        write!(
                            f,
                            "edge {} -> {}\n{}",
                            node_index,
                            *sink,
                            edge.attributes.display_with(ctx, graph)
                        )?;
                    }
                }
                Ok(())
            }
        }

        DisplayGraph(self, ctx)
    }
}

impl<'tree> Index<SyntaxNodeRef> for Graph<'tree> {
    type Output = Node<'tree>;
    fn index(&self, index: SyntaxNodeRef) -> &Node<'tree> {
        &self.syntax_nodes[&index]
    }
}

impl Index<GraphNodeRef> for Graph<'_> {
    type Output = GraphNode;
    fn index(&self, index: GraphNodeRef) -> &GraphNode {
        &self.graph_nodes[index.0 as usize]
    }
}

impl<'tree> IndexMut<GraphNodeRef> for Graph<'_> {
    fn index_mut(&mut self, index: GraphNodeRef) -> &mut GraphNode {
        &mut self.graph_nodes[index.0 as usize]
    }
}

/// A node in a graph
pub struct GraphNode {
    outgoing_edges: SmallVec<[(GraphNodeID, Edge); 8]>,
    /// The set of attributes associated with this graph node
    pub attributes: Attributes,
}

impl GraphNode {
    fn new() -> GraphNode {
        GraphNode {
            outgoing_edges: SmallVec::new(),
            attributes: Attributes::new(),
        }
    }

    /// Adds an edge to this node.  There can be at most one edge connecting any two graph nodes;
    /// the result indicates whether the edge is new (`Ok`) or already existed (`Err`).  In either
    /// case, you also get a mutable reference to the [`Edge`][] instance for the edge.
    pub fn add_edge(&mut self, sink: GraphNodeRef) -> Result<&mut Edge, &mut Edge> {
        let sink = sink.0;
        match self
            .outgoing_edges
            .binary_search_by_key(&sink, |(sink, _)| *sink)
        {
            Ok(index) => Err(&mut self.outgoing_edges[index].1),
            Err(index) => {
                self.outgoing_edges.insert(index, (sink, Edge::new()));
                Ok(&mut self.outgoing_edges[index].1)
            }
        }
    }

    /// Returns a reference to an outgoing edge from this node, if it exists.
    pub fn get_edge(&self, sink: GraphNodeRef) -> Option<&Edge> {
        let sink = sink.0;
        self.outgoing_edges
            .binary_search_by_key(&sink, |(sink, _)| *sink)
            .ok()
            .map(|index| &self.outgoing_edges[index].1)
    }

    /// Returns a mutable reference to an outgoing edge from this node, if it exists.
    pub fn get_edge_mut(&mut self, sink: GraphNodeRef) -> Option<&mut Edge> {
        let sink = sink.0;
        self.outgoing_edges
            .binary_search_by_key(&sink, |(sink, _)| *sink)
            .ok()
            .map(move |index| &mut self.outgoing_edges[index].1)
    }
}

/// An edge between two nodes in a graph
pub struct Edge {
    /// The set of attributes associated with this edge
    pub attributes: Attributes,
}

impl Edge {
    fn new() -> Edge {
        Edge {
            attributes: Attributes::new(),
        }
    }
}

/// A set of attributes associated with a graph node or edge
pub struct Attributes {
    values: SmallVec<[(Identifier, Value); 8]>,
}

impl Attributes {
    /// Creates a new, empty set of attributes.
    pub fn new() -> Attributes {
        Attributes {
            values: SmallVec::new(),
        }
    }

    /// Adds an attribute to this attribute set.  If there was already an attribute with the same
    /// name, replaces its value and returns `Err`.
    pub fn add<V: Into<Value>>(&mut self, name: Identifier, value: V) -> Result<(), ()> {
        match self.values.binary_search_by_key(&name, |(name, _)| *name) {
            Ok(index) => {
                self.values[index].1 = value.into();
                Err(())
            }
            Err(index) => {
                self.values.insert(index, (name, value.into()));
                Ok(())
            }
        }
    }

    /// Returns the value of a particular attribute, if it exists.
    pub fn get(&self, name: Identifier) -> Option<&Value> {
        self.values
            .binary_search_by_key(&name, |(name, _)| *name)
            .ok()
            .map(|index| &self.values[index].1)
    }

    /// fmt::Displays the contents of this attribute set.
    pub fn display_with<'a>(
        &'a self,
        ctx: &'a Context,
        graph: &'a Graph,
    ) -> impl fmt::Display + 'a {
        struct DisplayAttributes<'a, 'tree>(&'a Attributes, &'a Context, &'a Graph<'tree>);

        impl<'a, 'tree> fmt::Display for DisplayAttributes<'a, 'tree> {
            fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                let attributes = self.0;
                let ctx = self.1;
                let graph = self.2;
                for (name, value) in &attributes.values {
                    write!(
                        f,
                        "  {}: {}\n",
                        ctx.resolve(*name),
                        value.display_with(graph),
                    )?;
                }
                Ok(())
            }
        }

        DisplayAttributes(self, ctx, graph)
    }
}

/// The value of an attribute
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Value {
    // Scalar
    Null,
    Boolean(bool),
    Integer(u32),
    String(String),
    // Compound
    List(Vec<Value>),
    Set(BTreeSet<Value>),
    // References
    SyntaxNode(SyntaxNodeRef),
    GraphNode(GraphNodeRef),
}

impl Value {
    /// Coerces this value into a boolean, returning an error if it's some other type of value.
    pub fn into_bool(self, graph: &Graph) -> Result<bool, ExecutionError> {
        match self {
            Value::Boolean(value) => Ok(value),
            _ => Err(ExecutionError::ExpectedBoolean(format!(
                "got {}",
                self.display_with(graph)
            ))),
        }
    }

    /// Coerces this value into an integer, returning an error if it's some other type of value.
    pub fn into_integer(self, graph: &Graph) -> Result<u32, ExecutionError> {
        match self {
            Value::Integer(value) => Ok(value),
            _ => Err(ExecutionError::ExpectedInteger(format!(
                "got {}",
                self.display_with(graph)
            ))),
        }
    }

    /// Coerces this value into a string, returning an error if it's some other type of value.
    pub fn into_string(self, graph: &Graph) -> Result<String, ExecutionError> {
        match self {
            Value::String(value) => Ok(value),
            _ => Err(ExecutionError::ExpectedString(format!(
                "got {}",
                self.display_with(graph)
            ))),
        }
    }

    /// Coerces this value into a list, returning an error if it's some other type of value.
    pub fn into_list(self, graph: &Graph) -> Result<Vec<Value>, ExecutionError> {
        match self {
            Value::List(values) => Ok(values),
            _ => Err(ExecutionError::ExpectedList(format!(
                "got {}",
                self.display_with(graph)
            ))),
        }
    }

    /// Coerces this value into a syntax node reference, returning an error if it's some other type
    /// of value.
    pub fn into_syntax_node<'a, 'tree>(
        self,
        graph: &'a Graph<'tree>,
    ) -> Result<&'a Node<'tree>, ExecutionError> {
        match self {
            Value::SyntaxNode(node) => Ok(&graph[node]),
            _ => Err(ExecutionError::ExpectedSyntaxNode(format!(
                "got {}",
                self.display_with(graph)
            ))),
        }
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Value {
        Value::Boolean(value)
    }
}

impl From<u32> for Value {
    fn from(value: u32) -> Value {
        Value::Integer(value)
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Value {
        Value::String(value.to_string())
    }
}

impl From<String> for Value {
    fn from(value: String) -> Value {
        Value::String(value)
    }
}

impl From<Vec<Value>> for Value {
    fn from(value: Vec<Value>) -> Value {
        Value::List(value)
    }
}

impl DisplayWithGraph for Value {
    fn fmt(&self, f: &mut fmt::Formatter, graph: &Graph) -> fmt::Result {
        match self {
            Value::Null => write!(f, "#null"),
            Value::Boolean(value) => {
                if *value {
                    write!(f, "#true")
                } else {
                    write!(f, "#false")
                }
            }
            Value::Integer(value) => write!(f, "{}", value),
            Value::String(value) => write!(f, "{:?}", value),
            Value::List(value) => {
                write!(f, "[")?;
                let mut first = true;
                for element in value {
                    if first {
                        write!(f, "{}", element.display_with(graph))?;
                        first = false;
                    } else {
                        write!(f, ", {}", element.display_with(graph))?;
                    }
                }
                write!(f, "]")
            }
            Value::Set(value) => {
                write!(f, "{{")?;
                let mut first = true;
                for element in value {
                    if first {
                        write!(f, "{}", element.display_with(graph))?;
                        first = false;
                    } else {
                        write!(f, ", {}", element.display_with(graph))?;
                    }
                }
                write!(f, "}}")
            }
            Value::SyntaxNode(node) => node.fmt(f, graph),
            Value::GraphNode(node) => node.fmt(f, graph),
        }
    }
}

/// A reference to a syntax node in a graph
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SyntaxNodeRef(SyntaxNodeID);

impl From<SyntaxNodeRef> for Value {
    fn from(value: SyntaxNodeRef) -> Value {
        Value::SyntaxNode(value)
    }
}

impl DisplayWithGraph for SyntaxNodeRef {
    fn fmt(&self, f: &mut fmt::Formatter, graph: &Graph) -> fmt::Result {
        let node = graph[*self];
        write!(f, "{}", format_node(&node))
    }
}

pub fn format_node(node: &Node) -> String {
    format!(
        "[syntax node {} ({}, {})]",
        node.kind(),
        node.start_position().row + 1,
        node.start_position().column + 1
    )
}

/// A reference to a graph node
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GraphNodeRef(GraphNodeID);

impl From<GraphNodeRef> for Value {
    fn from(value: GraphNodeRef) -> Value {
        Value::GraphNode(value)
    }
}

impl DisplayWithGraph for GraphNodeRef {
    fn fmt(&self, f: &mut fmt::Formatter, _graph: &Graph) -> fmt::Result {
        write!(f, "[graph node {}]", self.0)
    }
}

/// Trait to Display with a given Context
pub trait DisplayWithGraph
where
    Self: Sized,
{
    fn fmt<'tree>(&self, f: &mut fmt::Formatter, graph: &Graph<'tree>) -> fmt::Result;

    fn display_with<'a, 'tree>(&'a self, graph: &'a Graph<'tree>) -> Box<dyn fmt::Display + 'a> {
        struct Impl<'a, 'tree, T: DisplayWithGraph>(&'a T, &'a Graph<'tree>);

        impl<'a, 'tree, T: DisplayWithGraph> fmt::Display for Impl<'a, 'tree, T> {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                self.0.fmt(f, self.1)
            }
        }

        Box::new(Impl(self, graph))
    }
}
