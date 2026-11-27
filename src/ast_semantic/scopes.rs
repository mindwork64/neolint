//! AST-derived lexical scopes and declaration bindings.

use std::collections::HashMap;

use crate::ast::{
    Ast, Block, EventHandler, Statement, StatementKind, SwitchStatement, TopLevel, TypeKind,
};
use crate::builtins::{self, ConstantDefinition, FunctionCatalog, TargetApiCatalog};
use crate::target::TargetProfile;

/// Stable index of one lexical scope in a [`ScopeTree`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ScopeId(usize);

impl ScopeId {
    /// Numeric index used by [`ScopeTree::scopes`].
    pub const fn index(self) -> usize {
        self.0
    }
}

/// Kind of declaration stored in a scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    /// Global variable declarator.
    GlobalVariable,
    /// User-defined function.
    Function,
    /// Function or canonical event parameter.
    Parameter,
    /// Local variable declarator.
    Local,
}

/// One declaration binding retained for later name/type analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    /// Declared name.
    pub name: String,
    /// LSL value type name, or the function return type when present.
    pub type_name: Option<String>,
    /// Declaration category.
    pub kind: SymbolKind,
    /// Name/declaration span.
    pub span: crate::diagnostic::Span,
    /// Owning lexical scope.
    pub scope: ScopeId,
}

/// One lexical scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    /// Parent scope; `None` only for the root.
    pub parent: Option<ScopeId>,
    /// Bindings in deterministic source registration order.
    pub symbols: Vec<Symbol>,
}

/// Deterministic AST-derived lexical scope tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeTree {
    scopes: Vec<Scope>,
    root: ScopeId,
    block_scopes: HashMap<(usize, usize), ScopeId>,
    switch_scopes: HashMap<(usize, usize), ScopeId>,
    /// Per-scope name index: spelling -> symbol indices ordered by declaration start.
    ///
    /// Derived from `scopes` and therefore never changes the observable result;
    /// it turns the per-use linear scan over one scope into a hash lookup plus a
    /// binary search, which keeps resolution near O(log n) instead of O(n) per
    /// use when a scope holds many declarations.
    name_index: Vec<HashMap<String, Vec<u32>>>,
    /// Built-in function signatures selected for this analysis target.
    builtin_functions: &'static FunctionCatalog,
    builtin_events: &'static HashMap<String, crate::builtins::Signature>,
    builtin_constants: &'static HashMap<String, ConstantDefinition>,
    target: TargetProfile,
}

impl ScopeTree {
    /// Build scopes and bindings from a parsed AST using canonical Second Life LSL built-ins.
    pub fn build(ast: &Ast) -> Self {
        Self::build_with_catalog(
            ast,
            builtins::catalog_for_target(TargetProfile::default()),
            TargetProfile::default(),
        )
    }

    /// Build all lexical scopes and bindings with one immutable target catalogue.
    pub(crate) fn build_with_catalog(
        ast: &Ast,
        catalog: &'static TargetApiCatalog,
        target: TargetProfile,
    ) -> Self {
        Self::build_with_metadata(
            ast,
            &catalog.callable_functions,
            &catalog.events,
            &catalog.constants,
            target,
        )
    }

    fn build_with_metadata(
        ast: &Ast,
        builtin_functions: &'static FunctionCatalog,
        builtin_events: &'static HashMap<String, crate::builtins::Signature>,
        builtin_constants: &'static HashMap<String, ConstantDefinition>,
        target: TargetProfile,
    ) -> Self {
        let mut tree = Self {
            scopes: vec![Scope {
                parent: None,
                symbols: Vec::new(),
            }],
            root: ScopeId(0),
            block_scopes: HashMap::new(),
            switch_scopes: HashMap::new(),
            name_index: vec![HashMap::new()],
            builtin_functions,
            builtin_events,
            builtin_constants,
            target,
        };
        for item in &ast.items {
            match item {
                TopLevel::Global(global) => {
                    for declarator in &global.declarators {
                        if declarator.name_error.is_none() && !declarator.name.is_empty() {
                            tree.register(
                                tree.root,
                                &declarator.name,
                                Some(type_name(global.ty.kind).to_owned()),
                                SymbolKind::GlobalVariable,
                                declarator.name_span,
                            );
                        }
                    }
                }
                TopLevel::Function(function) => {
                    if function.name.is_empty() {
                        continue;
                    }
                    tree.register(
                        tree.root,
                        &function.name,
                        function.return_type.map(|ty| type_name(ty.kind).to_owned()),
                        SymbolKind::Function,
                        function.name_span,
                    );
                    let Some(body) = &function.body else {
                        continue;
                    };
                    let scope = tree.new_scope(tree.root);
                    tree.block_scopes.insert(block_key(body), scope);
                    for parameter in &function.parameters {
                        if !parameter.name.is_empty() {
                            tree.register(
                                scope,
                                &parameter.name,
                                Some(type_name(parameter.ty.kind).to_owned()),
                                SymbolKind::Parameter,
                                parameter.name_span,
                            );
                        }
                    }
                    tree.walk_block(body, scope);
                }
                TopLevel::State(state) => {
                    if state.body_missing {
                        continue;
                    }
                    for event in &state.events {
                        tree.walk_event(event);
                    }
                }
                TopLevel::Error(_) => {}
            }
        }
        // Guarantee the index invariant even if a future registration path adds
        // bindings out of source order: ordering by declaration start is what the
        // lookup binary search relies on.
        for scope_index in 0..tree.scopes.len() {
            let starts: Vec<usize> = tree.scopes[scope_index]
                .symbols
                .iter()
                .map(|symbol| symbol.span.start)
                .collect();
            if let Some(by_name) = tree.name_index.get_mut(scope_index) {
                for indices in by_name.values_mut() {
                    indices.sort_by_key(|&index| starts[index as usize]);
                }
            }
        }
        tree
    }

    /// Built-in function signatures selected for this tree's target.
    pub(crate) fn builtin_functions(&self) -> &'static FunctionCatalog {
        self.builtin_functions
    }

    /// Built-in event signatures selected for this tree's target.
    pub(crate) fn builtin_events(&self) -> &'static HashMap<String, crate::builtins::Signature> {
        self.builtin_events
    }

    /// Built-in constants selected for this tree's target.
    pub(crate) fn builtin_constants(&self) -> &'static HashMap<String, ConstantDefinition> {
        self.builtin_constants
    }

    /// Platform/runtime profile associated with this tree.
    pub(crate) const fn target(&self) -> TargetProfile {
        self.target
    }

    /// Root/global scope.
    pub fn root(&self) -> ScopeId {
        self.root
    }

    /// All scopes in deterministic creation order.
    pub fn scopes(&self) -> &[Scope] {
        &self.scopes
    }

    /// Look up a scope by id.
    pub fn scope(&self, id: ScopeId) -> Option<&Scope> {
        self.scopes.get(id.0)
    }

    /// Scope created for a function/event body or nested block.
    pub fn scope_for_block(&self, block: &Block) -> Option<ScopeId> {
        self.block_scopes.get(&block_key(block)).copied()
    }

    /// Scope created for a Firestorm switch body.
    pub fn scope_for_switch(&self, switch: &SwitchStatement) -> Option<ScopeId> {
        self.switch_scopes
            .get(&(switch.span.start, switch.span.end))
            .copied()
    }

    /// Source ranges for lexical scopes, in the coordinate space of the parsed AST.
    ///
    /// The LSP adapter maps these ranges through the prepared-source map before
    /// using them against an editor buffer.
    #[cfg(feature = "lsp")]
    pub(crate) fn scope_ranges(&self) -> impl Iterator<Item = ((usize, usize), ScopeId)> + '_ {
        self.block_scopes
            .iter()
            .chain(self.switch_scopes.iter())
            .map(|(range, scope)| (*range, *scope))
    }

    /// Whether `ancestor` is the same scope as `descendant` or encloses it.
    pub fn is_ancestor(&self, ancestor: ScopeId, mut descendant: ScopeId) -> bool {
        loop {
            if descendant == ancestor {
                return true;
            }
            let Some(parent) = self.scope(descendant).and_then(|scope| scope.parent) else {
                return false;
            };
            descendant = parent;
        }
    }

    /// Resolve a visible value binding at a source byte position.
    ///
    /// Function declarations are excluded: they are not LSL values unless
    /// invoked as a call expression.
    pub fn lookup_value_at(
        &self,
        mut scope: ScopeId,
        name: &str,
        use_offset: usize,
    ) -> Option<&Symbol> {
        loop {
            let current = self.scope(scope)?;
            if let Some(indices) = self.name_index.get(scope.0).and_then(|map| map.get(name)) {
                // Symbols are ordered by declaration start, so the last binding
                // visible at `use_offset` is the declaration-order-correct one.
                let visible = indices.partition_point(|&index| {
                    current.symbols[index as usize].span.start <= use_offset
                });
                // Functions are not value bindings: skip over any same-named
                // function rather than stopping at it, matching the previous scan.
                for &index in indices[..visible].iter().rev() {
                    let symbol = &current.symbols[index as usize];
                    if symbol.kind != SymbolKind::Function {
                        return Some(symbol);
                    }
                }
            }
            scope = current.parent?;
        }
    }

    /// Resolve a name through the given scope and its ancestors.
    ///
    /// The latest binding in each scope wins, matching AST/source registration
    /// order. This is a name lookup only; it performs no type inference.
    pub fn lookup(&self, mut scope: ScopeId, name: &str) -> Option<&Symbol> {
        loop {
            let current = self.scope(scope)?;
            if let Some(symbol) = self
                .name_index
                .get(scope.0)
                .and_then(|map| map.get(name))
                .and_then(|indices| indices.last())
                .map(|&index| &current.symbols[index as usize])
            {
                return Some(symbol);
            }
            scope = current.parent?;
        }
    }

    fn new_scope(&mut self, parent: ScopeId) -> ScopeId {
        let id = ScopeId(self.scopes.len());
        self.scopes.push(Scope {
            parent: Some(parent),
            symbols: Vec::new(),
        });
        self.name_index.push(HashMap::new());
        id
    }

    fn register(
        &mut self,
        scope: ScopeId,
        name: &str,
        type_name: Option<String>,
        kind: SymbolKind,
        span: crate::diagnostic::Span,
    ) {
        if let Some(target) = self.scopes.get_mut(scope.0) {
            let index = target.symbols.len() as u32;
            target.symbols.push(Symbol {
                name: name.to_owned(),
                type_name,
                kind,
                span,
                scope,
            });
            if let Some(by_name) = self.name_index.get_mut(scope.0) {
                by_name.entry(name.to_owned()).or_default().push(index);
            }
        }
    }

    fn walk_event(&mut self, event: &EventHandler) {
        let Some(body) = &event.body else {
            return;
        };
        let scope = self.new_scope(self.root);
        self.block_scopes.insert(block_key(body), scope);
        // Only handlers known to the selected target contribute event parameters.
        // Unknown extension event bodies are still scanned for local declarations.
        if self.builtin_events.contains_key(event.name.as_str()) {
            for parameter in &event.parameters {
                if !parameter.name.is_empty() {
                    self.register(
                        scope,
                        &parameter.name,
                        Some(type_name(parameter.ty.kind).to_owned()),
                        SymbolKind::Parameter,
                        parameter.name_span,
                    );
                }
            }
        }
        self.walk_block(body, scope);
    }

    fn walk_block(&mut self, body: &Block, scope: ScopeId) {
        self.walk_statements(&body.statements, scope);
    }

    fn walk_statements<'a>(&mut self, statements: &'a [Statement], scope: ScopeId) {
        let mut stack: Vec<(&'a Statement, ScopeId)> = statements
            .iter()
            .rev()
            .map(|statement| (statement, scope))
            .collect();
        while let Some((statement, current_scope)) = stack.pop() {
            match &statement.kind {
                StatementKind::Local(local) => {
                    for declarator in &local.declarators {
                        if declarator.name_error.is_none() && !declarator.name.is_empty() {
                            self.register(
                                current_scope,
                                &declarator.name,
                                Some(type_name(local.ty.kind).to_owned()),
                                SymbolKind::Local,
                                declarator.name_span,
                            );
                        }
                    }
                }
                StatementKind::Block(block) => {
                    let child = self.new_scope(current_scope);
                    self.block_scopes.insert(block_key(block), child);
                    for statement in block.statements.iter().rev() {
                        stack.push((statement, child));
                    }
                }
                StatementKind::If(if_statement) => {
                    if let Some(else_branch) = &if_statement.else_branch {
                        stack.push((else_branch, current_scope));
                    }
                    stack.push((&if_statement.then_branch, current_scope));
                }
                StatementKind::For(for_statement) => {
                    stack.push((&for_statement.body, current_scope));
                }
                StatementKind::While(while_statement) => {
                    stack.push((&while_statement.body, current_scope));
                }
                StatementKind::DoWhile(do_while) => {
                    stack.push((&do_while.body, current_scope));
                }
                StatementKind::Switch(switch) => {
                    self.push_switch(switch, current_scope, &mut stack);
                }
                StatementKind::Empty
                | StatementKind::Return(_)
                | StatementKind::Jump(_)
                | StatementKind::StateChange(_)
                | StatementKind::Label(_)
                | StatementKind::Expression(_)
                | StatementKind::Break(_)
                | StatementKind::Error(_) => {}
            }
        }
    }

    fn push_switch<'a>(
        &mut self,
        switch: &'a SwitchStatement,
        parent: ScopeId,
        stack: &mut Vec<(&'a Statement, ScopeId)>,
    ) {
        let scope = self.new_scope(parent);
        self.switch_scopes
            .insert((switch.span.start, switch.span.end), scope);
        for arm in switch.arms.iter().rev() {
            for statement in arm.statements.iter().rev() {
                stack.push((statement, scope));
            }
        }
    }
}

pub(crate) fn type_name(kind: TypeKind) -> &'static str {
    match kind {
        TypeKind::Integer => "integer",
        TypeKind::Float => "float",
        TypeKind::String => "string",
        TypeKind::Key => "key",
        TypeKind::List => "list",
        TypeKind::Vector => "vector",
        TypeKind::Rotation => "rotation",
    }
}

/// Convenience lookup that returns the current bindings for a name.
pub fn bindings_for<'a>(tree: &'a ScopeTree, scope: ScopeId, name: &str) -> Vec<&'a Symbol> {
    tree.scope(scope)
        .map(|scope| {
            scope
                .symbols
                .iter()
                .filter(|item| item.name == name)
                .collect()
        })
        .unwrap_or_default()
}

/// Count symbols by name in one scope without exposing mutable storage.
pub fn symbol_counts(scope: &Scope) -> HashMap<&str, usize> {
    let mut counts = HashMap::new();
    for symbol in &scope.symbols {
        *counts.entry(symbol.name.as_str()).or_insert(0) += 1;
    }
    counts
}

fn block_key(block: &Block) -> (usize, usize) {
    (block.span.start, block.span.end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::parse_ast;
    use crate::lex;

    fn tree_for(source: &str) -> ScopeTree {
        let (tokens, lexical) = lex(source);
        assert!(lexical.is_empty(), "{lexical:?}");
        let (ast, syntax) = parse_ast(&tokens);
        assert!(syntax.is_empty(), "{syntax:?}");
        ScopeTree::build(&ast)
    }

    /// Reference implementation of the pre-index linear scan.
    fn naive_value_at<'a>(
        tree: &'a ScopeTree,
        mut scope: ScopeId,
        name: &str,
        offset: usize,
    ) -> Option<&'a Symbol> {
        loop {
            let current = tree.scope(scope)?;
            if let Some(symbol) = current.symbols.iter().rev().find(|item| {
                item.name == name && item.kind != SymbolKind::Function && item.span.start <= offset
            }) {
                return Some(symbol);
            }
            scope = current.parent?;
        }
    }

    fn naive_lookup<'a>(tree: &'a ScopeTree, mut scope: ScopeId, name: &str) -> Option<&'a Symbol> {
        loop {
            let current = tree.scope(scope)?;
            if let Some(symbol) = current.symbols.iter().rev().find(|item| item.name == name) {
                return Some(symbol);
            }
            scope = current.parent?;
        }
    }

    #[test]
    fn name_index_matches_the_linear_scan_for_every_binding_and_offset() {
        let sources = [
            "integer value;\nfunction_name() { }\ndefault { state_entry() { integer value; value = 1; } }",
            "integer a;\ndefault { state_entry() { { integer a; a = 1; } a = 2; } }",
            "default { state_entry() { integer v; v = 1; { integer v; v = 2; { integer v; v = 3; } } } }",
            "integer g;\ninteger helper(integer g) { integer local; local = g; return local; }\ndefault { state_entry() { g = helper(g); } }",
            "integer x;\ninteger x;\ndefault { state_entry() { integer x; x = 1; } }",
        ];
        for source in sources {
            let tree = tree_for(source);
            for scope_index in 0..tree.scopes().len() {
                let scope = ScopeId(scope_index);
                let names: Vec<String> = tree
                    .scope(scope)
                    .map(|current| current.symbols.iter().map(|s| s.name.clone()).collect())
                    .unwrap_or_default();
                for name in names {
                    assert_eq!(
                        tree.lookup(scope, &name).map(|s| (s.span.start, s.scope)),
                        naive_lookup(&tree, scope, &name).map(|s| (s.span.start, s.scope)),
                        "lookup disagreed for {name:?} in scope {scope_index}"
                    );
                    for offset in 0..=source.len() {
                        assert_eq!(
                            tree.lookup_value_at(scope, &name, offset).map(|s| (s.span.start, s.scope)),
                            naive_value_at(&tree, scope, &name, offset).map(|s| (s.span.start, s.scope)),
                            "lookup_value_at disagreed for {name:?} at {offset} in scope {scope_index}"
                        );
                    }
                }
            }
        }
    }
}
