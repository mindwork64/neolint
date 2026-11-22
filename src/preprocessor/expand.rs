//! Macro expansion with recursion guards and provenance construction.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use crate::preprocessor::builtins;
use crate::preprocessor::diagnostics::{
    DiagSink, RawRelated, PRE_DIRECT_RECURSION, PRE_EXPANDED_BYTES_LIMIT,
    PRE_EXPANDED_TOKENS_LIMIT, PRE_EXPANSION_DEPTH, PRE_INDIRECT_RECURSION,
    PRE_INVALID_MACRO_OPERATOR, PRE_MACRO_ARGUMENT_COUNT, PRE_UNTERMINATED_MACRO_CALL,
};
use crate::preprocessor::macros::{MacroDefinition, MacroTable};
use crate::preprocessor::token::{
    tokenize_generated, Piece, PieceKind, ProvId, RawSpan, WorkItem, WorkOrigin,
};
use crate::source::file::SourceFile;
use crate::source::{OriginKind, SourceId};

pub(crate) const MAX_EXPANSION_DEPTH: usize = 64;
pub(crate) const MAX_EXPANDED_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const MAX_EXPANDED_TOKENS: usize = 1_000_000;

/// One node in the provenance graph. Nodes are appended parent-first.
#[derive(Debug, Clone)]
pub(crate) struct ProvNode {
    pub(crate) kind: OriginKind,
    pub(crate) span: RawSpan,
    pub(crate) invocation: Option<RawSpan>,
    pub(crate) definition: Option<RawSpan>,
    pub(crate) parent: Option<ProvId>,
    pub(crate) label: Option<&'static str>,
}

#[derive(Debug, Default)]
pub(crate) struct ProvArena {
    pub(crate) nodes: Vec<ProvNode>,
}

impl ProvArena {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn add(&mut self, node: ProvNode) -> ProvId {
        let id = ProvId(self.nodes.len());
        self.nodes.push(node);
        id
    }
}

#[derive(Debug, Default)]
pub(crate) struct ExpansionLimits {
    expanded_bytes: usize,
    expanded_tokens: usize,
    bytes_reported: bool,
    tokens_reported: bool,
}

impl ExpansionLimits {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn bytes_limit_reported(&self) -> bool {
        self.bytes_reported
    }

    pub(crate) fn tokens_limit_reported(&self) -> bool {
        self.tokens_reported
    }

    /// Charge output bytes/tokens, reporting one explicit limit diagnostic.
    pub(crate) fn charge(
        &mut self,
        bytes: usize,
        tokens: usize,
        diags: &mut DiagSink,
        trigger: RawSpan,
    ) -> bool {
        self.has_capacity(bytes, tokens, diags, trigger)
    }

    fn has_capacity(
        &mut self,
        bytes: usize,
        tokens: usize,
        diags: &mut DiagSink,
        trigger: RawSpan,
    ) -> bool {
        if self.expanded_bytes.saturating_add(bytes) > MAX_EXPANDED_BYTES {
            if !self.bytes_reported {
                self.bytes_reported = true;
                diags.push(
                    PRE_EXPANDED_BYTES_LIMIT,
                    format!("macro expansion exceeds {MAX_EXPANDED_BYTES} expanded bytes"),
                    trigger,
                );
            }
            return false;
        }
        if self.expanded_tokens.saturating_add(tokens) > MAX_EXPANDED_TOKENS {
            if !self.tokens_reported {
                self.tokens_reported = true;
                diags.push(
                    PRE_EXPANDED_TOKENS_LIMIT,
                    format!("macro expansion exceeds {MAX_EXPANDED_TOKENS} expanded tokens"),
                    trigger,
                );
            }
            return false;
        }
        self.expanded_bytes += bytes;
        self.expanded_tokens += tokens;
        true
    }
}

#[derive(Debug, Clone, Copy)]
struct Context {
    /// Parent origin for argument text and the invocation origin for body text.
    parent: Option<ProvId>,
    invocation: Option<RawSpan>,
    definition: Option<RawSpan>,
}

impl Context {
    fn top() -> Self {
        Self {
            parent: None,
            invocation: None,
            definition: None,
        }
    }
}

#[derive(Debug)]
enum Unit {
    Literal(Piece),
    Whitespace(Piece),
    Param(usize),
    VarArgs,
    Stringify(String),
    Paste(String),
}

/// Returns, for each actual argument, whether it is used in a position that
/// requires the standard pre-expansion. Arguments used only as `#` or `##`
/// operands must stay raw and must not trigger expansion diagnostics.
fn argument_expansion_needs(definition: &MacroDefinition, arg_count: usize) -> Vec<bool> {
    let Some(params) = &definition.params else {
        return vec![false; arg_count];
    };
    let pieces = &definition.replacement;
    let significant: Vec<usize> = pieces
        .iter()
        .enumerate()
        .filter(|(_, piece)| !matches!(piece.kind, PieceKind::Whitespace | PieceKind::Comment))
        .map(|(index, _)| index)
        .collect();
    let mut raw = vec![false; pieces.len()];
    for (position, &index) in significant.iter().enumerate() {
        if pieces[index].text == "#" {
            if let Some(&next) = significant.get(position + 1) {
                raw[next] = true;
            }
        } else if pieces[index].text == "##" {
            if position > 0 {
                raw[significant[position - 1]] = true;
            }
            if let Some(&next) = significant.get(position + 1) {
                raw[next] = true;
            }
        }
    }
    let fixed = params.len();
    let mut needs = vec![false; arg_count];
    let fixed_need: Vec<bool> = (0..fixed)
        .map(|position| {
            significant.iter().any(|&index| {
                !raw[index]
                    && pieces[index].kind == PieceKind::Identifier
                    && pieces[index].text == params[position]
            })
        })
        .collect();
    for (position, need) in fixed_need.into_iter().enumerate() {
        if let Some(slot) = needs.get_mut(position) {
            *slot = need;
        }
    }
    if definition.variadic {
        let varargs_needed = significant.iter().any(|&index| {
            !raw[index]
                && pieces[index].kind == PieceKind::Identifier
                && pieces[index].text == "__VA_ARGS__"
        });
        for need in needs.iter_mut().skip(fixed) {
            *need = varargs_needed;
        }
    }
    needs
}

fn stringify_items(items: &[WorkItem]) -> String {
    let mut output = String::new();
    let mut pending_space = false;
    for item in items {
        if matches!(item.kind, PieceKind::Whitespace | PieceKind::Comment) {
            pending_space = !output.is_empty();
            continue;
        }
        if pending_space && !output.is_empty() {
            output.push(' ');
        }
        pending_space = false;
        output.push_str(&item.text);
    }
    output.trim().to_owned()
}

fn stringify_varargs(args: &[Vec<WorkItem>]) -> String {
    args.iter()
        .map(|arg| stringify_items(arg))
        .collect::<Vec<_>>()
        .join(", ")
}

fn escape_string(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

fn paste_text(
    unit: &Unit,
    _definition: &MacroDefinition,
    raw_args: &[Vec<WorkItem>],
    fixed: usize,
) -> String {
    match unit {
        Unit::Literal(piece) => piece.text.clone(),
        Unit::Whitespace(_) => String::new(),
        Unit::Param(position) => raw_args
            .get(*position)
            .map(|arg| arg.iter().map(|item| item.text.as_str()).collect())
            .unwrap_or_default(),
        Unit::VarArgs => raw_args
            .get(fixed..)
            .unwrap_or_default()
            .iter()
            .map(|arg| {
                arg.iter()
                    .map(|item| item.text.as_str())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join(","),
        Unit::Stringify(text) => format!("\"{}\"", escape_string(text)),
        Unit::Paste(text) => text.clone(),
    }
}

#[derive(Debug, Clone)]
struct ActiveMacro {
    name: String,
    invocation: RawSpan,
}

pub(crate) struct Expander<'a> {
    macros: &'a MacroTable,
    pub(crate) prov: &'a mut ProvArena,
    diags: &'a mut DiagSink,
    limits: &'a mut ExpansionLimits,
    active: Vec<ActiveMacro>,
    recursion_reported: BTreeSet<(String, usize, usize)>,
    source: SourceId,
    base_parent: Option<ProvId>,
    builtin_span: Option<RawSpan>,
    files: &'a HashMap<SourceId, Arc<SourceFile>>,
}

impl<'a> Expander<'a> {
    pub(crate) fn new(
        macros: &'a MacroTable,
        prov: &'a mut ProvArena,
        diags: &'a mut DiagSink,
        limits: &'a mut ExpansionLimits,
        source: SourceId,
        base_parent: Option<ProvId>,
        files: &'a HashMap<SourceId, Arc<SourceFile>>,
    ) -> Self {
        Self {
            macros,
            prov,
            diags,
            limits,
            active: Vec::new(),
            recursion_reported: BTreeSet::new(),
            source,
            base_parent,
            builtin_span: None,
            files,
        }
    }

    pub(crate) fn expand_top(&mut self, items: Vec<WorkItem>) -> Vec<WorkItem> {
        self.expand_items(items, Context::top(), 0)
    }

    fn expand_items(
        &mut self,
        items: Vec<WorkItem>,
        context: Context,
        depth: usize,
    ) -> Vec<WorkItem> {
        let mut output = Vec::new();
        let mut index = 0;
        while index < items.len() {
            let item = &items[index];
            if item.kind == PieceKind::Identifier {
                if self.macros.get(&item.text).is_none() {
                    let builtin_span = match item.origin {
                        WorkOrigin::Direct => item.span,
                        WorkOrigin::Node(_) => self.builtin_span.unwrap_or(item.span),
                    };
                    if let Some(expansion) = builtins::expand(&item.text, builtin_span, self.files)
                    {
                        let node = self.prov.add(ProvNode {
                            kind: OriginKind::Generated,
                            span: RawSpan::point(builtin_span.source, builtin_span.start),
                            invocation: None,
                            definition: None,
                            parent: self.base_parent,
                            label: Some("predefined macro"),
                        });
                        output.push(WorkItem {
                            text: expansion.text,
                            kind: expansion.kind,
                            span: builtin_span,
                            origin: WorkOrigin::Node(node),
                        });
                        index += 1;
                        continue;
                    }
                }
                if let Some(definition) = self.macros.get(&item.text).cloned() {
                    if let Some((args, end)) = self.invocation_arguments(&items, index, &definition)
                    {
                        let call_span = call_span(&items[index..=end], self.source);
                        if self.is_active(&definition.name) {
                            self.report_recursion(&definition, call_span);
                            output.extend(items[index..=end].iter().cloned());
                            index = end + 1;
                            continue;
                        }
                        if depth >= MAX_EXPANSION_DEPTH {
                            self.diags.push(
                                PRE_EXPANSION_DEPTH,
                                format!(
                                    "macro expansion exceeds depth {MAX_EXPANSION_DEPTH} at '{}'",
                                    definition.name
                                ),
                                call_span,
                            );
                            output.extend(items[index..=end].iter().cloned());
                            index = end + 1;
                            continue;
                        }
                        let mut args = args;
                        let expected = definition.params.as_ref().map_or(0, Vec::len);
                        if expected == 1 && args.is_empty() {
                            args.push(Vec::new());
                        }
                        let count_ok = if definition.variadic {
                            args.len() >= expected
                        } else {
                            args.len() == expected
                        };
                        if !count_ok {
                            self.diags.push_with_related(
                                PRE_MACRO_ARGUMENT_COUNT,
                                format!(
                                    "macro '{}' expects {} arguments, got {}",
                                    definition.name,
                                    expected,
                                    args.len()
                                ),
                                call_span,
                                vec![RawRelated {
                                    span: definition.definition_span,
                                    label: "macro definition",
                                }],
                            );
                            output.extend(items[index..=end].iter().cloned());
                            index = end + 1;
                            continue;
                        }
                        let invocation_parent = self.invocation_origin(item, context);
                        let expanded = self.expand_macro(
                            &definition,
                            args,
                            call_span,
                            invocation_parent,
                            depth,
                            &mut output,
                        );
                        if !expanded {
                            output.extend(items[index..=end].iter().cloned());
                        }
                        index = if expanded {
                            self.rescan_generated_call(&mut output, &items, end + 1, context, depth)
                        } else {
                            end + 1
                        };
                        continue;
                    } else if definition.is_function_like() {
                        // A function-like macro name without a call is ordinary text.
                    } else {
                        let call_span = item.span;
                        if self.is_active(&definition.name) {
                            self.report_recursion(&definition, call_span);
                            output.push(item.clone());
                            index += 1;
                            continue;
                        }
                        if depth >= MAX_EXPANSION_DEPTH {
                            self.diags.push(
                                PRE_EXPANSION_DEPTH,
                                format!(
                                    "macro expansion exceeds depth {MAX_EXPANSION_DEPTH} at '{}'",
                                    definition.name
                                ),
                                call_span,
                            );
                            output.push(item.clone());
                            index += 1;
                            continue;
                        }
                        let invocation_parent = self.invocation_origin(item, context);
                        let expanded = self.expand_macro(
                            &definition,
                            Vec::new(),
                            call_span,
                            invocation_parent,
                            depth,
                            &mut output,
                        );
                        if !expanded {
                            output.push(item.clone());
                        }
                        index = if expanded {
                            self.rescan_generated_call(
                                &mut output,
                                &items,
                                index + 1,
                                context,
                                depth,
                            )
                        } else {
                            index + 1
                        };
                        continue;
                    }
                }
            }
            output.push(self.convert_direct_item(item, context));
            index += 1;
        }
        output
    }

    /// Allow a function-like macro name produced by an expansion to consume a
    /// following original `(`. The replacement re-scan handles calls entirely
    /// inside a replacement list; this handles the token-stream boundary.
    fn rescan_generated_call(
        &mut self,
        output: &mut Vec<WorkItem>,
        items: &[WorkItem],
        next: usize,
        context: Context,
        depth: usize,
    ) -> usize {
        let mut next = next;
        loop {
            let Some(last) = output.last() else {
                return next;
            };
            if last.kind != PieceKind::Identifier {
                return next;
            }
            let Some(definition) = self.macros.get(&last.text).cloned() else {
                return next;
            };
            if !definition.is_function_like() || self.is_active(&definition.name) {
                return next;
            }
            let mut open = next;
            while open < items.len()
                && matches!(items[open].kind, PieceKind::Whitespace | PieceKind::Comment)
            {
                open += 1;
            }
            if open >= items.len() || items[open].text != "(" {
                return next;
            }
            let mut candidate = Vec::with_capacity(1 + items.len().saturating_sub(next));
            candidate.push(last.clone());
            candidate.extend(items[next..].iter().cloned());
            let Some((_, close)) = self.invocation_arguments(&candidate, 0, &definition) else {
                return next;
            };
            let prefix = candidate[..=close].to_vec();
            output.pop();
            let expanded = self.expand_items(prefix, context, depth + 1);
            output.extend(expanded);
            next += close;
        }
    }

    fn expand_macro(
        &mut self,
        definition: &MacroDefinition,
        args: Vec<Vec<WorkItem>>,
        call_span: RawSpan,
        invocation_parent: Option<ProvId>,
        depth: usize,
        output: &mut Vec<WorkItem>,
    ) -> bool {
        let mut expanded_args = Vec::with_capacity(args.len());
        let saved_builtin_span = self.builtin_span;
        // Original argument tokens keep their own coordinates. Tokens that
        // came from an enclosing macro replacement inherit that enclosing
        // invocation's coordinate context.
        self.active.push(ActiveMacro {
            name: definition.name.clone(),
            invocation: call_span,
        });
        let raw_args = args;
        let expansion = argument_expansion_needs(definition, raw_args.len());
        for (index, arg) in raw_args.iter().enumerate() {
            let needed = expansion.get(index).copied().unwrap_or(true);
            if !needed {
                expanded_args.push(Vec::new());
                continue;
            }
            let arg_context = Context {
                parent: invocation_parent,
                invocation: Some(call_span),
                definition: Some(definition.definition_span),
            };
            let expanded = self.expand_items(arg.clone(), arg_context, depth + 1);
            expanded_args.push(expanded);
        }

        // Replacement-list builtins are expanded at the effective invocation
        // site. An invocation generated by an enclosing replacement inherits
        // that enclosing invocation's location rather than its definition.
        self.builtin_span = Some(saved_builtin_span.unwrap_or(call_span));
        let replacement = self.build_replacement(
            definition,
            &raw_args,
            &expanded_args,
            call_span,
            invocation_parent,
        );
        let Some(replacement) = replacement else {
            self.active.pop();
            self.builtin_span = saved_builtin_span;
            return false;
        };
        let expanded = self.expand_items(replacement, Context::top(), depth + 1);
        self.active.pop();
        self.builtin_span = saved_builtin_span;
        output.extend(expanded);
        true
    }

    fn build_replacement(
        &mut self,
        definition: &MacroDefinition,
        raw_args: &[Vec<WorkItem>],
        expanded_args: &[Vec<WorkItem>],
        call_span: RawSpan,
        invocation_parent: Option<ProvId>,
    ) -> Option<Vec<WorkItem>> {
        let fixed = definition.params.as_ref().map_or(0, Vec::len);
        let mut estimated_bytes = 0usize;
        let mut estimated_tokens = 0usize;
        for piece in &definition.replacement {
            estimated_bytes = estimated_bytes.saturating_add(piece.text.len());
            estimated_tokens += 1;
        }
        for arg in raw_args {
            for item in arg {
                estimated_bytes = estimated_bytes.saturating_add(item.text.len());
                estimated_tokens += 1;
            }
        }
        if !self
            .limits
            .has_capacity(estimated_bytes, estimated_tokens, self.diags, call_span)
        {
            return None;
        }

        let mut units = Vec::new();
        let pieces = &definition.replacement;
        let mut index = 0usize;
        while index < pieces.len() {
            let piece = &pieces[index];
            if piece.text == "#" && definition.params.is_some() {
                index += 1;
                while index < pieces.len()
                    && matches!(
                        pieces[index].kind,
                        PieceKind::Whitespace | PieceKind::Comment
                    )
                {
                    index += 1;
                }
                if index >= pieces.len() {
                    self.diags.push(
                        PRE_INVALID_MACRO_OPERATOR,
                        "stringification requires a macro parameter",
                        call_span,
                    );
                    return None;
                }
                let operand = &pieces[index];
                let text = if let Some(params) = &definition.params {
                    if operand.kind == PieceKind::Identifier {
                        if let Some(position) =
                            params.iter().position(|param| param == &operand.text)
                        {
                            stringify_items(&raw_args[position])
                        } else if operand.text == "__VA_ARGS__" && definition.variadic {
                            stringify_varargs(&raw_args[fixed..])
                        } else {
                            self.diags.push(
                                PRE_INVALID_MACRO_OPERATOR,
                                "stringification requires a macro parameter",
                                operand.span,
                            );
                            return None;
                        }
                    } else {
                        self.diags.push(
                            PRE_INVALID_MACRO_OPERATOR,
                            "stringification requires a macro parameter",
                            operand.span,
                        );
                        return None;
                    }
                } else {
                    return None;
                };
                units.push(Unit::Stringify(text));
                index += 1;
                continue;
            }
            if piece.text == "##" {
                while matches!(units.last(), Some(Unit::Whitespace(_))) {
                    units.pop();
                }
                let Some(left) = units.pop() else {
                    self.diags.push(
                        PRE_INVALID_MACRO_OPERATOR,
                        "token pasting requires a left operand",
                        call_span,
                    );
                    return None;
                };
                index += 1;
                while index < pieces.len()
                    && matches!(
                        pieces[index].kind,
                        PieceKind::Whitespace | PieceKind::Comment
                    )
                {
                    index += 1;
                }
                if index >= pieces.len() {
                    self.diags.push(
                        PRE_INVALID_MACRO_OPERATOR,
                        "token pasting requires a right operand",
                        call_span,
                    );
                    return None;
                }
                let right =
                    self.paste_operand(&pieces[index], definition, raw_args, fixed, call_span)?;
                let left_text = paste_text(&left, definition, raw_args, fixed);
                let right_text = paste_text(&right, definition, raw_args, fixed);
                let pasted = format!("{left_text}{right_text}");
                if !pasted.is_empty() {
                    let tokens = tokenize_generated(&pasted, call_span);
                    let significant: Vec<&Piece> = tokens
                        .iter()
                        .filter(|item| {
                            !matches!(item.kind, PieceKind::Whitespace | PieceKind::Comment)
                        })
                        .collect();
                    if significant.len() != 1
                        || significant[0].text != pasted
                        || matches!(significant[0].kind, PieceKind::Other)
                    {
                        self.diags.push(
                            PRE_INVALID_MACRO_OPERATOR,
                            format!("token pasting produced an invalid token: '{pasted}'"),
                            call_span,
                        );
                        return None;
                    }
                }
                units.push(Unit::Paste(pasted));
                index += 1;
                continue;
            }
            if definition.params.is_some()
                && piece.kind == PieceKind::Identifier
                && piece.text == "__VA_ARGS__"
                && definition.variadic
            {
                units.push(Unit::VarArgs);
                index += 1;
                continue;
            }
            if let Some(params) = &definition.params {
                if piece.kind == PieceKind::Identifier {
                    if let Some(position) = params.iter().position(|param| param == &piece.text) {
                        units.push(Unit::Param(position));
                        index += 1;
                        continue;
                    }
                }
            }
            if matches!(piece.kind, PieceKind::Whitespace | PieceKind::Comment) {
                units.push(Unit::Whitespace(piece.clone()));
            } else {
                units.push(Unit::Literal(piece.clone()));
            }
            index += 1;
        }

        let mut replacement = Vec::new();
        for unit in units {
            match unit {
                Unit::Whitespace(piece) | Unit::Literal(piece) => {
                    replacement.push(self.definition_item(
                        definition,
                        &piece,
                        call_span,
                        invocation_parent,
                    ));
                }
                Unit::Param(position) => {
                    replacement.extend(expanded_args[position].iter().cloned());
                }
                Unit::VarArgs => {
                    replacement.extend(self.varargs_items(
                        &expanded_args[fixed..],
                        call_span,
                        invocation_parent,
                    ));
                }
                Unit::Stringify(text) => {
                    let node = self.prov.add(ProvNode {
                        kind: OriginKind::Stringify,
                        span: call_span,
                        invocation: Some(call_span),
                        definition: Some(definition.definition_span),
                        parent: invocation_parent.or(self.base_parent),
                        label: Some("stringified argument"),
                    });
                    replacement.push(WorkItem {
                        text: format!("\"{}\"", escape_string(&text)),
                        kind: PieceKind::String,
                        span: call_span,
                        origin: WorkOrigin::Node(node),
                    });
                }
                Unit::Paste(text) => {
                    if text.is_empty() {
                        continue;
                    }
                    let kind = tokenize_generated(&text, call_span)
                        .into_iter()
                        .find(|item| {
                            !matches!(item.kind, PieceKind::Whitespace | PieceKind::Comment)
                        })
                        .map_or(PieceKind::Other, |item| item.kind);
                    let node = self.prov.add(ProvNode {
                        kind: OriginKind::TokenPaste,
                        span: call_span,
                        invocation: Some(call_span),
                        definition: Some(definition.definition_span),
                        parent: invocation_parent.or(self.base_parent),
                        label: Some("pasted token"),
                    });
                    replacement.push(WorkItem {
                        text,
                        kind,
                        span: call_span,
                        origin: WorkOrigin::Node(node),
                    });
                }
            }
        }
        Some(replacement)
    }

    fn definition_item(
        &mut self,
        definition: &MacroDefinition,
        piece: &Piece,
        call_span: RawSpan,
        invocation_parent: Option<ProvId>,
    ) -> WorkItem {
        let node = self.prov.add(ProvNode {
            kind: OriginKind::MacroDefinition,
            span: piece.span,
            invocation: Some(call_span),
            definition: Some(definition.definition_span),
            parent: invocation_parent.or(self.base_parent),
            label: Some("macro definition"),
        });
        WorkItem {
            text: piece.text.clone(),
            kind: piece.kind,
            span: piece.span,
            origin: WorkOrigin::Node(node),
        }
    }

    fn paste_operand(
        &mut self,
        piece: &Piece,
        definition: &MacroDefinition,
        raw_args: &[Vec<WorkItem>],
        fixed: usize,
        call_span: RawSpan,
    ) -> Option<Unit> {
        if piece.kind == PieceKind::Identifier {
            if let Some(params) = &definition.params {
                if let Some(position) = params.iter().position(|param| param == &piece.text) {
                    return Some(Unit::Param(position));
                }
                if piece.text == "__VA_ARGS__" && definition.variadic {
                    return Some(Unit::VarArgs);
                }
            }
        }
        if matches!(piece.kind, PieceKind::Whitespace | PieceKind::Comment) {
            self.diags.push(
                PRE_INVALID_MACRO_OPERATOR,
                "token pasting requires a token operand",
                call_span,
            );
            return None;
        }
        let _ = (fixed, raw_args);
        Some(Unit::Literal(piece.clone()))
    }

    fn varargs_items(
        &mut self,
        args: &[Vec<WorkItem>],
        call_span: RawSpan,
        invocation_parent: Option<ProvId>,
    ) -> Vec<WorkItem> {
        let mut items = Vec::new();
        for (index, arg) in args.iter().enumerate() {
            if index > 0 {
                let node = self.prov.add(ProvNode {
                    kind: OriginKind::Generated,
                    span: call_span,
                    invocation: Some(call_span),
                    definition: None,
                    parent: invocation_parent.or(self.base_parent),
                    label: Some("variadic comma"),
                });
                items.push(WorkItem {
                    text: ", ".to_owned(),
                    kind: PieceKind::Punct,
                    span: call_span,
                    origin: WorkOrigin::Node(node),
                });
            }
            items.extend(arg.iter().cloned());
        }
        items
    }

    fn invocation_origin(&mut self, item: &WorkItem, context: Context) -> Option<ProvId> {
        match item.origin {
            WorkOrigin::Node(id) => Some(id),
            WorkOrigin::Direct => {
                if context.definition.is_some() {
                    Some(self.prov.add(ProvNode {
                        kind: OriginKind::MacroArgument,
                        span: item.span,
                        invocation: context.invocation,
                        definition: context.definition,
                        parent: context.parent.or(self.base_parent),
                        label: Some("macro argument"),
                    }))
                } else {
                    self.base_parent
                }
            }
        }
    }

    fn convert_direct_item(&mut self, item: &WorkItem, context: Context) -> WorkItem {
        if item.origin != WorkOrigin::Direct {
            return item.clone();
        }
        if context.definition.is_some() {
            let node = self.prov.add(ProvNode {
                kind: OriginKind::MacroArgument,
                span: item.span,
                invocation: context.invocation,
                definition: context.definition,
                parent: context.parent.or(self.base_parent),
                label: Some("macro argument"),
            });
            WorkItem {
                text: item.text.clone(),
                kind: item.kind,
                span: item.span,
                origin: WorkOrigin::Node(node),
            }
        } else {
            item.clone()
        }
    }

    fn invocation_arguments(
        &mut self,
        items: &[WorkItem],
        index: usize,
        definition: &MacroDefinition,
    ) -> Option<(Vec<Vec<WorkItem>>, usize)> {
        if !definition.is_function_like() {
            return None;
        }
        let mut open = index + 1;
        while open < items.len()
            && matches!(items[open].kind, PieceKind::Whitespace | PieceKind::Comment)
        {
            open += 1;
        }
        if open >= items.len() || items[open].text != "(" {
            return None;
        }
        let mut args = Vec::new();
        let mut current = Vec::new();
        let mut paren_depth = 0usize;
        let mut position = open + 1;
        while position < items.len() {
            let item = &items[position];
            if item.text == "(" {
                paren_depth += 1;
                current.push(item.clone());
            } else if item.text == ")" {
                if paren_depth == 0 {
                    trim_argument(&mut current);
                    if !current.is_empty() || !args.is_empty() {
                        args.push(current);
                    }
                    return Some((args, position));
                }
                paren_depth -= 1;
                current.push(item.clone());
            } else if item.text == "," && paren_depth == 0 {
                trim_argument(&mut current);
                args.push(std::mem::take(&mut current));
            } else {
                current.push(item.clone());
            }
            position += 1;
        }
        let name_span = items[index].span;
        self.diags.push_with_related(
            PRE_UNTERMINATED_MACRO_CALL,
            format!("unterminated invocation of macro '{}'", definition.name),
            name_span,
            vec![RawRelated {
                span: definition.definition_span,
                label: "macro definition",
            }],
        );
        None
    }

    fn is_active(&self, name: &str) -> bool {
        self.active.iter().any(|entry| entry.name == name)
    }

    fn report_recursion(&mut self, definition: &MacroDefinition, invocation: RawSpan) {
        let direct = self
            .active
            .last()
            .is_some_and(|entry| entry.name == definition.name);
        let key = (definition.name.clone(), invocation.start, invocation.end);
        if !self.recursion_reported.insert(key) {
            return;
        }
        let code = if direct {
            PRE_DIRECT_RECURSION
        } else {
            PRE_INDIRECT_RECURSION
        };
        let message = if direct {
            format!(
                "direct recursion while expanding macro '{}'",
                definition.name
            )
        } else {
            format!(
                "indirect recursion while expanding macro '{}'",
                definition.name
            )
        };
        let mut related = vec![RawRelated {
            span: definition.definition_span,
            label: "macro definition",
        }];
        for entry in &self.active {
            if entry.name == definition.name {
                related.push(RawRelated {
                    span: entry.invocation,
                    label: "outer invocation",
                });
            }
        }
        self.diags
            .push_with_related(code, message, invocation, related);
    }
}

fn trim_argument(argument: &mut Vec<WorkItem>) {
    while argument
        .first()
        .is_some_and(|item| matches!(item.kind, PieceKind::Whitespace | PieceKind::Comment))
    {
        argument.remove(0);
    }
    while argument
        .last()
        .is_some_and(|item| matches!(item.kind, PieceKind::Whitespace | PieceKind::Comment))
    {
        argument.pop();
    }
}

fn call_span(items: &[WorkItem], fallback_source: SourceId) -> RawSpan {
    match (items.first(), items.last()) {
        (Some(first), Some(last)) => {
            RawSpan::new(first.span.source, first.span.start, last.span.end)
        }
        _ => RawSpan::point(fallback_source, 0),
    }
}

/// Resolve `defined` before macro expansion and expand the remaining expression.
#[allow(clippy::too_many_arguments)]
pub(crate) fn expand_condition(
    pieces: Vec<Piece>,
    macros: &MacroTable,
    prov: &mut ProvArena,
    diags: &mut DiagSink,
    limits: &mut ExpansionLimits,
    source: SourceId,
    base_parent: Option<ProvId>,
    files: &HashMap<SourceId, Arc<SourceFile>>,
) -> Vec<WorkItem> {
    let pieces = resolve_defined(pieces, macros, diags);
    let items: Vec<WorkItem> = pieces.into_iter().map(WorkItem::from_piece).collect();
    let mut expander = Expander::new(macros, prov, diags, limits, source, base_parent, files);
    expander.expand_top(items)
}

fn resolve_defined(pieces: Vec<Piece>, macros: &MacroTable, diags: &mut DiagSink) -> Vec<Piece> {
    let mut output = Vec::new();
    let mut index = 0;
    while index < pieces.len() {
        let piece = &pieces[index];
        if piece.kind != PieceKind::Identifier || piece.text != "defined" {
            output.push(piece.clone());
            index += 1;
            continue;
        }
        let start = piece.span;
        let mut position = index + 1;
        while position < pieces.len()
            && matches!(
                pieces[position].kind,
                PieceKind::Whitespace | PieceKind::Comment
            )
        {
            position += 1;
        }
        let mut name = None;
        let mut end = piece.span;
        if position < pieces.len() && pieces[position].text == "(" {
            position += 1;
            while position < pieces.len()
                && matches!(
                    pieces[position].kind,
                    PieceKind::Whitespace | PieceKind::Comment
                )
            {
                position += 1;
            }
            if position < pieces.len() && pieces[position].kind == PieceKind::Identifier {
                name = Some(pieces[position].text.clone());
                end = pieces[position].span;
                position += 1;
                while position < pieces.len()
                    && matches!(
                        pieces[position].kind,
                        PieceKind::Whitespace | PieceKind::Comment
                    )
                {
                    position += 1;
                }
                if position < pieces.len() && pieces[position].text == ")" {
                    end = pieces[position].span;
                    position += 1;
                } else {
                    name = None;
                }
            }
        } else if position < pieces.len() && pieces[position].kind == PieceKind::Identifier {
            name = Some(pieces[position].text.clone());
            end = pieces[position].span;
            position += 1;
        }
        let span = RawSpan::new(start.source, start.start, end.end);
        match name {
            Some(name) => output.push(Piece::new(
                if macros.contains(&name) { "1" } else { "0" }.to_owned(),
                PieceKind::Number,
                span,
            )),
            None => {
                diags.push(
                    crate::preprocessor::diagnostics::PRE_INVALID_IF_EXPRESSION,
                    "malformed 'defined' expression",
                    span,
                );
                output.push(Piece::new("0".to_owned(), PieceKind::Number, span));
            }
        }
        index = position;
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preprocessor::diagnostics::DiagSink;
    use crate::source::{AnalysisUnitId, RunId, SourceId};

    fn span() -> RawSpan {
        RawSpan::new(
            SourceId::new(AnalysisUnitId::new(RunId::new(1), 0), 0),
            0,
            1,
        )
    }

    #[test]
    fn limits_report_once_and_stop_charging() {
        let mut limits = ExpansionLimits::new();
        let mut diags = DiagSink::new();
        assert!(limits.charge(MAX_EXPANDED_BYTES, 0, &mut diags, span()));
        assert!(!limits.charge(1, 0, &mut diags, span()));
        assert!(!limits.charge(1, 0, &mut diags, span()));
        assert_eq!(diags.items().len(), 1);
        assert_eq!(diags.items()[0].code, PRE_EXPANDED_BYTES_LIMIT);
        assert_eq!(diags.items()[0].primary, span());

        let mut limits = ExpansionLimits::new();
        let mut diags = DiagSink::new();
        assert!(limits.charge(0, MAX_EXPANDED_TOKENS, &mut diags, span()));
        assert!(!limits.charge(0, 1, &mut diags, span()));
        assert_eq!(diags.items().len(), 1);
        assert_eq!(diags.items()[0].code, PRE_EXPANDED_TOKENS_LIMIT);
    }

    #[test]
    fn separator_detection_covers_token_merges() {
        assert!(crate::preprocessor::token::needs_separator("a", "b"));
        assert!(crate::preprocessor::token::needs_separator("+", "+"));
        assert!(crate::preprocessor::token::needs_separator("/", "*"));
        assert!(crate::preprocessor::token::needs_separator("/", "/"));
        assert!(crate::preprocessor::token::needs_separator("1", "."));
        assert!(!crate::preprocessor::token::needs_separator("a", " "));
        assert!(!crate::preprocessor::token::needs_separator("+", "1"));
    }
}

#[cfg(test)]
mod provenance_tests {
    use super::*;
    use crate::preprocessor::token::{Piece, PieceKind};
    use crate::source::{AnalysisUnitId, RunId, SourceId};

    fn piece(text: &str, kind: PieceKind, start: usize, end: usize, source: SourceId) -> Piece {
        Piece::new(text.to_owned(), kind, RawSpan::new(source, start, end))
    }

    #[test]
    fn function_expansion_builds_definition_and_argument_origins() {
        let source = SourceId::new(AnalysisUnitId::new(RunId::new(2), 0), 0);
        let definition_span = RawSpan::new(source, 0, 10);
        let replacement = vec![
            piece("x", PieceKind::Identifier, 9, 10, source),
            piece(" ", PieceKind::Whitespace, 10, 11, source),
            piece("+", PieceKind::Punct, 11, 12, source),
            piece(" ", PieceKind::Whitespace, 12, 13, source),
            piece("x", PieceKind::Identifier, 13, 14, source),
        ];
        let definition = MacroDefinition {
            name: "ID".to_owned(),
            params: Some(vec!["x".to_owned()]),
            variadic: false,
            replacement,
            definition_span,
        };
        let mut table = MacroTable::new();
        table.define(definition);
        let items = vec![
            WorkItem::from_piece(piece("ID", PieceKind::Identifier, 20, 22, source)),
            WorkItem::from_piece(piece("(", PieceKind::Punct, 22, 23, source)),
            WorkItem::from_piece(piece("7", PieceKind::Number, 23, 24, source)),
            WorkItem::from_piece(piece(")", PieceKind::Punct, 24, 25, source)),
        ];
        let mut prov = ProvArena::new();
        let mut diags = DiagSink::new();
        let mut limits = ExpansionLimits::new();
        let files = HashMap::new();
        let mut expander = Expander::new(
            &table,
            &mut prov,
            &mut diags,
            &mut limits,
            source,
            None,
            &files,
        );
        let output = expander.expand_top(items);
        assert!(output.iter().any(|item| item.text == "7"));
        assert!(diags.items().is_empty());
        assert!(prov
            .nodes
            .iter()
            .any(|node| node.kind == OriginKind::MacroDefinition));
        assert!(prov
            .nodes
            .iter()
            .any(|node| node.kind == OriginKind::MacroArgument));
        assert!(prov
            .nodes
            .iter()
            .all(|node| { node.invocation.is_some() || node.kind == OriginKind::Direct }));
    }
}
