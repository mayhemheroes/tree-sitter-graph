// -*- coding: utf-8 -*-
// ------------------------------------------------------------------------------------------------
// Copyright © 2021, tree-sitter authors.
// Licensed under either of Apache License, Version 2.0, or MIT license, at your option.
// Please see the LICENSE-APACHE or LICENSE-MIT files in this distribution for license details.
// ------------------------------------------------------------------------------------------------

mod statements;
mod store;
mod values;
mod variables;

use anyhow::Context as _;
use log::{debug, trace};

use std::collections::HashMap;
use std::fmt;

use tree_sitter::CaptureQuantifier::One;
use tree_sitter::Query;
use tree_sitter::QueryCursor;
use tree_sitter::QueryMatch;
use tree_sitter::Tree;

use crate::ast;
use crate::execution::query_capture_value;
use crate::execution::ExecutionError;
use crate::execution::Globals;
use crate::functions::Functions;
use crate::graph;
use crate::graph::DisplayWithGraph as _;
use crate::graph::Graph;
use crate::parser::FULL_MATCH;
use crate::Context;
use crate::DisplayWithContext as _;
use crate::Identifier;

use statements::*;
use store::*;
use values::*;
use variables::*;

impl ast::File {
    /// Executes this graph DSL file against a source file.  You must provide the parsed syntax
    /// tree (`tree`) as well as the source text that it was parsed from (`source`).  You also
    /// provide the set of functions and global variables that are available during execution.
    pub fn execute_lazy<'tree>(
        &self,
        ctx: &Context,
        tree: &'tree Tree,
        source: &'tree str,
        functions: &mut Functions,
        globals: &Globals,
    ) -> Result<Graph<'tree>, ExecutionError> {
        let mut graph = Graph::new();
        if tree.root_node().has_error() {
            return Err(ExecutionError::ParseTreeHasErrors);
        }
        let mut locals = VariableMap::new();
        let mut cursor = QueryCursor::new();
        let mut store = Store::new();
        let mut scoped_store = ScopedVariables::new();
        let mut lazy_graph = Vec::new();
        for stanza in &self.stanzas {
            stanza.execute_lazy(
                ctx,
                tree,
                source,
                &mut graph,
                globals,
                &mut locals,
                &mut cursor,
                &mut store,
                &mut scoped_store,
                &mut lazy_graph,
            )?;
        }
        let mut function_parameters = Vec::new();
        let mut prev_element_debug_info = HashMap::new();
        for graph_stmt in &lazy_graph {
            graph_stmt
                .evaluate(&mut EvaluationContext {
                    ctx,
                    source,
                    graph: &mut graph,
                    functions,
                    store: &mut store,
                    scoped_store: &mut scoped_store,
                    function_parameters: &mut function_parameters,
                    prev_element_debug_info: &mut prev_element_debug_info,
                })
                .with_context(|| format!("Executing {}", graph_stmt.display_with(ctx, &graph)))?;
        }
        Ok(graph)
    }
}

/// Context for execution, which executes stanzas to build the lazy graph
struct ExecutionContext<'a, 'g, 'tree> {
    ctx: &'a Context,
    graph: &'a mut Graph<'tree>,
    globals: &'a Globals<'g>,
    locals: &'a mut dyn Variables,
    current_regex_captures: &'a Vec<String>,
    mat: &'a QueryMatch<'a, 'tree>,
    store: &'a mut Store,
    scoped_store: &'a mut ScopedVariables,
    lazy_graph: &'a mut Vec<Statement>,
}

/// Context for evaluation, which evalautes the lazy graph to build the actual graph
pub(self) struct EvaluationContext<'a, 'tree> {
    pub ctx: &'a Context,
    pub source: &'tree str,
    pub graph: &'a mut Graph<'tree>,
    pub functions: &'a mut Functions,
    pub store: &'a Store,
    pub scoped_store: &'a ScopedVariables,
    pub function_parameters: &'a mut Vec<graph::Value>, // re-usable buffer to reduce memory allocations
    pub prev_element_debug_info: &'a mut HashMap<GraphElementKey, DebugInfo>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub(super) enum GraphElementKey {
    NodeAttribute(graph::GraphNodeRef, Identifier),
    Edge(graph::GraphNodeRef, graph::GraphNodeRef),
    EdgeAttribute(graph::GraphNodeRef, graph::GraphNodeRef, Identifier),
}

impl ast::Stanza {
    fn execute_lazy<'l, 'g, 'tree>(
        &self,
        ctx: &Context,
        tree: &'tree Tree,
        source: &'tree str,
        graph: &mut Graph<'tree>,
        globals: &Globals<'g>,
        locals: &mut VariableMap<'l>,
        cursor: &mut QueryCursor,
        store: &mut Store,
        scoped_store: &mut ScopedVariables,
        lazy_graph: &mut Vec<Statement>,
    ) -> Result<(), ExecutionError> {
        let full_match_index = full_match_capture_index(&self.query);
        let current_regex_captures = vec![];
        let matches = cursor.matches(&self.query, tree.root_node(), source.as_bytes());
        for mat in matches {
            locals.clear();
            let mut exec = ExecutionContext {
                ctx,
                graph,
                globals,
                locals,
                current_regex_captures: &current_regex_captures,
                mat: &mat,
                store,
                scoped_store,
                lazy_graph,
            };
            let node = query_capture_value(full_match_index, One, &mat, exec.graph);
            debug!(
                "match {} at {}",
                node.display_with(exec.graph),
                self.location
            );
            trace!("{{");
            for statement in &self.statements {
                statement
                    .execute_lazy(&mut exec)
                    .with_context(|| format!("Executing {}", statement.display_with(exec.ctx)))?;
            }
            trace!("}}");
        }
        Ok(())
    }
}

impl ast::Statement {
    fn execute_lazy(&self, exec: &mut ExecutionContext) -> Result<(), ExecutionError> {
        match self {
            _ => Ok(()),
        }
    }
}

#[allow(unused)]
pub fn full_match_capture_index(query: &Query) -> usize {
    query
        .capture_names()
        .iter()
        .position(|c| c == FULL_MATCH)
        .unwrap()
}

/// Trait to Display with a given Context and Graph
pub trait DisplayWithContextAndGraph
where
    Self: Sized,
{
    fn fmt<'tree>(
        &self,
        f: &mut fmt::Formatter,
        ctx: &Context,
        graph: &Graph<'tree>,
    ) -> fmt::Result;

    fn display_with<'a, 'tree>(
        &'a self,
        ctx: &'a Context,
        graph: &'a Graph<'tree>,
    ) -> Box<dyn fmt::Display + 'a> {
        struct Impl<'a, 'tree, T: DisplayWithContextAndGraph>(&'a T, &'a Context, &'a Graph<'tree>);

        impl<'a, 'tree, T: DisplayWithContextAndGraph> fmt::Display for Impl<'a, 'tree, T> {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                self.0.fmt(f, self.1, self.2)
            }
        }

        Box::new(Impl(self, ctx, graph))
    }
}

impl<T: DisplayWithContextAndGraph> DisplayWithContextAndGraph for Box<T> {
    fn fmt<'tree>(
        &self,
        f: &mut fmt::Formatter,
        ctx: &Context,
        graph: &Graph<'tree>,
    ) -> fmt::Result {
        self.as_ref().fmt(f, ctx, graph)
    }
}
