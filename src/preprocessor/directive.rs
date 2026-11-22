//! Directive scanning, conditional compilation, and the full preprocessing run.

use std::sync::Arc;

use crate::diagnostic::DiagnosticKind;
use crate::preprocessor::diagnostics::{
    DiagSink, RawDiagnostic, RawRelated, PRE_CONDITIONAL_DEPTH, PRE_CONFLICTING_REDEFINITION,
    PRE_DUPLICATE_ELSE, PRE_ELIF_AFTER_ELSE, PRE_ELIF_WITHOUT_IF, PRE_ELSE_WITHOUT_IF,
    PRE_ENDIF_WITHOUT_IF, PRE_INVALID_DEFINE, PRE_INVALID_IFDEF, PRE_INVALID_UNDEF,
    PRE_MALFORMED_DIRECTIVE, PRE_MISSING_OPERAND, PRE_UNKNOWN_DIRECTIVE, PRE_UNSUPPORTED_DIRECTIVE,
    PRE_UNSUPPORTED_MACRO_OPERATOR, PRE_UNTERMINATED_CONDITIONAL,
};
use crate::preprocessor::expand::{
    expand_condition, Expander, ExpansionLimits, ProvArena, ProvNode, MAX_EXPANDED_BYTES,
    MAX_EXPANDED_TOKENS,
};
use crate::preprocessor::expr;
use crate::preprocessor::macros::{DefineOutcome, MacroDefinition, MacroTable};
use crate::preprocessor::token::{
    is_identifier_continue, needs_separator, tokenize_mapped, MappedText, PieceKind, RawSpan,
    WorkItem, WorkOrigin,
};
use crate::preprocessor::{PreparedSource, PreprocessDiagnostic, RelatedLocation};
use crate::source::file::SourceFile;
use crate::source::manager::SourceManager;
use crate::source::map::{PreparedSpan, SourceMapBuilder, SpanError};
use crate::source::{AnalysisUnitId, OriginKind, SourceId};

pub(crate) const MAX_CONDITIONAL_DEPTH: usize = 64;

#[derive(Debug)]
struct CondFrame {
    parent_active: bool,
    active: bool,
    any_taken: bool,
    else_seen: bool,
    opening: RawSpan,
}

#[derive(Debug)]
pub(crate) struct Preprocessor<'a> {
    unit: AnalysisUnitId,
    source: SourceId,
    text: &'a str,
    files: Vec<Arc<SourceFile>>,
    macros: MacroTable,
    conditions: Vec<CondFrame>,
    output: Vec<WorkItem>,
    prov: ProvArena,
    diags: DiagSink,
    limits: ExpansionLimits,
    last_directive_end: Option<usize>,
    pending_start: Option<usize>,
    pending_end: usize,
    stopped: bool,
}

impl<'a> Preprocessor<'a> {
    pub(crate) fn new(
        unit: AnalysisUnitId,
        source: SourceId,
        text: &'a str,
        files: Vec<Arc<SourceFile>>,
    ) -> Self {
        Self {
            unit,
            source,
            text,
            files,
            macros: MacroTable::new(),
            conditions: Vec::new(),
            output: Vec::new(),
            prov: ProvArena::new(),
            diags: DiagSink::new(),
            limits: ExpansionLimits::new(),
            last_directive_end: None,
            pending_start: None,
            pending_end: 0,
            stopped: false,
        }
    }

    pub(crate) fn run(mut self) -> Result<PreparedSource, SpanError> {
        let mut position = 0usize;
        let mut in_block_comment = false;
        while position < self.text.len() {
            let line_start = position;
            let continued = continued_line(self.text, position);
            if !in_block_comment && !continued {
                if let Some(hash) = directive_hash(self.text, position) {
                    self.flush_pending();
                    let (mapped, end) = MappedText::logical_line(self.text, self.source, hash + 1);
                    let directive_span = directive_span(self.source, self.text, hash, end);
                    self.process_directive(line_start, hash, &mapped, directive_span);
                    self.last_directive_end = Some(end);
                    self.emit_last_newline(line_start, end);
                    position = end;
                    continue;
                }
            } else if !continued {
                if let Some((hash, comment_end)) = closing_comment_directive(self.text, position) {
                    if self.is_active() {
                        self.push_pending(position, comment_end);
                    }
                    self.flush_pending();
                    let (mapped, end) = MappedText::logical_line(self.text, self.source, hash + 1);
                    let directive_span = directive_span(self.source, self.text, hash, end);
                    self.process_directive(comment_end, hash, &mapped, directive_span);
                    self.last_directive_end = Some(end);
                    self.emit_last_newline(comment_end, end);
                    position = end;
                    in_block_comment = false;
                    continue;
                }
            }
            let (end, new_block) = scan_line(self.text, position, in_block_comment);
            if self.is_active() {
                self.push_pending(position, end);
            } else {
                self.flush_pending();
                self.emit_newlines(position, end);
            }
            position = end;
            in_block_comment = new_block;
        }
        self.flush_pending();

        let eof = RawSpan::point(self.source, self.text.len());
        while let Some(frame) = self.conditions.pop() {
            self.diags.push_with_related(
                PRE_UNTERMINATED_CONDITIONAL,
                "unterminated conditional directive",
                frame.opening,
                vec![RawRelated {
                    span: eof,
                    label: "end of file",
                }],
            );
        }
        self.finalize()
    }

    fn is_active(&self) -> bool {
        self.conditions.last().map_or(true, |frame| frame.active)
    }

    fn push_pending(&mut self, start: usize, end: usize) {
        if end <= start || self.stopped {
            return;
        }
        if self.pending_start.is_some() && self.pending_end != start {
            self.flush_pending();
        }
        if self.pending_start.is_none() {
            self.pending_start = Some(start);
        }
        self.pending_end = end;
    }

    fn flush_pending(&mut self) {
        let Some(start) = self.pending_start.take() else {
            return;
        };
        let end = self.pending_end;
        if self.stopped || end <= start {
            return;
        }
        let mapped = MappedText::spliced_range(self.text, self.source, start, end);
        let pieces = tokenize_mapped(&mapped);
        self.expand_and_append(pieces);
    }

    fn append(&mut self, items: Vec<WorkItem>) {
        if self.stopped {
            return;
        }
        for item in items {
            if item.text.is_empty() {
                continue;
            }
            if matches!(item.origin, WorkOrigin::Direct) {
                let accepted = {
                    let limits = &mut self.limits;
                    let diags = &mut self.diags;
                    limits.charge(item.text.len(), 1, diags, item.span)
                };
                if !accepted {
                    self.stopped = true;
                    return;
                }
            }
            self.output.push(item);
        }
    }

    fn expand_and_append(&mut self, pieces: Vec<crate::preprocessor::token::Piece>) {
        if pieces.is_empty() {
            return;
        }
        let items: Vec<WorkItem> = pieces.into_iter().map(WorkItem::from_piece).collect();
        let expanded = {
            let mut expander = Expander::new(
                &self.macros,
                &mut self.prov,
                &mut self.diags,
                &mut self.limits,
                self.source,
            );
            expander.expand_top(items)
        };
        self.append(expanded);
    }

    fn emit_newlines(&mut self, start: usize, end: usize) {
        let mut index = start;
        while index < end {
            let bytes = self.text.as_bytes();
            let len = if bytes[index] == b'\r' {
                if index + 1 < end && bytes[index + 1] == b'\n' {
                    2
                } else {
                    1
                }
            } else if bytes[index] == b'\n' {
                1
            } else {
                index += self.text[index..].chars().next().map_or(1, char::len_utf8);
                continue;
            };
            let span = RawSpan::new(self.source, index, index + len);
            if !is_spliced_newline(self.text, index) {
                self.append(vec![WorkItem {
                    text: self.text[index..index + len].to_owned(),
                    kind: PieceKind::Whitespace,
                    span,
                    origin: WorkOrigin::Direct,
                }]);
            }
            index += len;
        }
    }

    fn emit_last_newline(&mut self, start: usize, end: usize) {
        let span = last_newline(self.text, start, end);
        if let Some((from, to)) = span {
            self.append(vec![WorkItem {
                text: self.text[from..to].to_owned(),
                kind: PieceKind::Whitespace,
                span: RawSpan::new(self.source, from, to),
                origin: WorkOrigin::Direct,
            }]);
        }
    }

    fn process_directive(
        &mut self,
        line_start: usize,
        _hash: usize,
        mapped: &MappedText,
        directive_span: RawSpan,
    ) {
        let text = mapped.text();
        let mut position = 0usize;
        skip_directive_space(text, &mut position);
        if position >= text.len() {
            if self.is_active() {
                self.diags.push(
                    PRE_MALFORMED_DIRECTIVE,
                    "empty preprocessor directive",
                    directive_span,
                );
            }
            return;
        }
        if !text[position..]
            .chars()
            .next()
            .is_some_and(crate::preprocessor::token::is_identifier_start)
        {
            if self.is_active() {
                self.diags.push(
                    PRE_MALFORMED_DIRECTIVE,
                    "malformed preprocessor directive",
                    directive_span,
                );
            }
            return;
        }
        let name_start = position;
        position += text[position..].chars().next().map_or(1, char::len_utf8);
        while position < text.len() {
            let ch = text[position..].chars().next().unwrap_or('\0');
            if !is_identifier_continue(ch) {
                break;
            }
            position += ch.len_utf8();
        }
        let name = &text[name_start..position];
        let name_span = mapped.raw_span(name_start, position);
        let rest = mapped.slice(position, text.len());
        match name {
            "define" => self.define(directive_span, &rest),
            "undef" => self.undef(&rest, directive_span),
            "if" => self.open_if(&rest, directive_span, name_span, ConditionalKind::If),
            "ifdef" => self.open_if(&rest, directive_span, name_span, ConditionalKind::Ifdef),
            "ifndef" => self.open_if(&rest, directive_span, name_span, ConditionalKind::Ifndef),
            "elif" => self.elif(&rest, name_span, directive_span),
            "else" => self.else_directive(&rest, name_span, directive_span),
            "endif" => self.endif(&rest, name_span, directive_span),
            "include" | "pragma" => {
                if self.is_active() {
                    self.diags.push(
                        PRE_UNSUPPORTED_DIRECTIVE,
                        format!("#{name} is not supported in Phase 2"),
                        directive_span,
                    );
                }
            }
            _ => {
                if self.is_active() {
                    self.diags.push(
                        PRE_UNKNOWN_DIRECTIVE,
                        format!("unknown preprocessor directive '#{name}'"),
                        directive_span,
                    );
                }
            }
        }
        let _ = line_start;
    }

    fn define(&mut self, directive_span: RawSpan, rest: &MappedText) {
        if !self.is_active() {
            return;
        }
        let text = rest.text();
        let mut position = 0usize;
        skip_directive_space(text, &mut position);
        if position >= text.len()
            || !text[position..]
                .chars()
                .next()
                .is_some_and(crate::preprocessor::token::is_identifier_start)
        {
            self.diags.push(
                PRE_INVALID_DEFINE,
                "#define requires a macro name",
                directive_span,
            );
            return;
        }
        let name_start = position;
        position += text[position..].chars().next().map_or(1, char::len_utf8);
        while position < text.len() {
            let ch = text[position..].chars().next().unwrap_or('\0');
            if !is_identifier_continue(ch) {
                break;
            }
            position += ch.len_utf8();
        }
        let name = text[name_start..position].to_owned();
        let name_span = rest.raw_span(name_start, position);
        let function_like = text[position..].starts_with('(');
        let params = if function_like {
            let parameter_text = rest.slice(position, text.len());
            match parse_parameters(&parameter_text) {
                Some((params, consumed)) => {
                    position += consumed;
                    Some(params)
                }
                None => {
                    self.diags.push(
                        PRE_INVALID_DEFINE,
                        "malformed macro parameter list",
                        name_span,
                    );
                    return;
                }
            }
        } else {
            None
        };
        let replacement_text = rest.slice(position, text.len());
        let leading = replacement_text.text().len()
            - replacement_text
                .text()
                .trim_start_matches(|ch: char| ch.is_whitespace())
                .len();
        let replacement_text = replacement_text.slice(leading, replacement_text.text().len());
        let mut pieces = tokenize_mapped(&replacement_text);
        if let Some(first) = pieces
            .iter()
            .position(|piece| !matches!(piece.kind, PieceKind::Whitespace | PieceKind::Comment))
        {
            let last = pieces
                .iter()
                .rposition(|piece| {
                    !matches!(piece.kind, PieceKind::Whitespace | PieceKind::Comment)
                })
                .unwrap_or(first);
            pieces = pieces[first..=last].to_vec();
        } else {
            pieces.clear();
        }
        for piece in &pieces {
            if piece.text == "#" || piece.text == "##" {
                self.diags.push(
                    PRE_UNSUPPORTED_MACRO_OPERATOR,
                    "stringification and token pasting are not supported in Phase 2",
                    piece.span,
                );
                return;
            }
        }
        let definition = MacroDefinition {
            name,
            params,
            replacement: pieces,
            definition_span: directive_span,
        };
        let existing_span = self
            .macros
            .get(&definition.name)
            .map(|item| item.definition_span);
        match self.macros.define(definition) {
            DefineOutcome::Defined | DefineOutcome::Identical => {}
            DefineOutcome::Conflict => {
                let mut related = Vec::new();
                if let Some(span) = existing_span {
                    related.push(RawRelated {
                        span,
                        label: "previous macro definition",
                    });
                }
                self.diags.push_with_related(
                    PRE_CONFLICTING_REDEFINITION,
                    "conflicting macro redefinition",
                    name_span,
                    related,
                );
            }
        }
    }

    fn undef(&mut self, rest: &MappedText, directive_span: RawSpan) {
        if !self.is_active() {
            return;
        }
        match single_identifier(rest) {
            Some((name, _)) => {
                self.macros.undef(&name);
            }
            None => {
                self.diags.push(
                    PRE_INVALID_UNDEF,
                    "#undef requires exactly one identifier",
                    directive_span,
                );
            }
        }
    }

    fn open_if(
        &mut self,
        rest: &MappedText,
        directive_span: RawSpan,
        name_span: RawSpan,
        kind: ConditionalKind,
    ) {
        let parent_active = self.is_active();
        if self.conditions.len() >= MAX_CONDITIONAL_DEPTH {
            self.diags.push(
                PRE_CONDITIONAL_DEPTH,
                format!("conditional nesting exceeds {MAX_CONDITIONAL_DEPTH} levels"),
                name_span,
            );
            self.conditions.push(CondFrame {
                parent_active: false,
                active: false,
                any_taken: false,
                else_seen: false,
                opening: directive_span,
            });
            return;
        }
        let operand = if matches!(kind, ConditionalKind::If) {
            None
        } else {
            single_identifier(rest)
        };
        let (operand_text, operand_span) = operand.as_ref().map_or(
            ("", RawSpan::point(self.source, name_span.start)),
            |(name, span)| (name.as_str(), *span),
        );
        let mut active = false;
        match kind {
            ConditionalKind::If => {
                if parent_active {
                    if !has_expression_token(rest) {
                        self.diags.push_with_related(
                            PRE_MISSING_OPERAND,
                            "#if requires an expression",
                            RawSpan::point(self.source, directive_span.end),
                            vec![RawRelated {
                                span: directive_span,
                                label: "conditional directive",
                            }],
                        );
                    } else {
                        active = self.evaluate_condition(rest, directive_span);
                    }
                }
            }
            ConditionalKind::Ifdef | ConditionalKind::Ifndef => {
                if operand.is_none() {
                    if parent_active {
                        self.diags.push_with_related(
                            PRE_INVALID_IFDEF,
                            format!("#{} requires one identifier", kind.name()),
                            if operand_text.is_empty() {
                                name_span
                            } else {
                                operand_span
                            },
                            vec![RawRelated {
                                span: directive_span,
                                label: "conditional directive",
                            }],
                        );
                    }
                } else if parent_active {
                    let defined = self.macros.contains(operand_text);
                    active = if matches!(kind, ConditionalKind::Ifdef) {
                        defined
                    } else {
                        !defined
                    };
                }
            }
        }
        self.conditions.push(CondFrame {
            parent_active,
            active: parent_active && active,
            any_taken: parent_active && active,
            else_seen: false,
            opening: directive_span,
        });
    }

    fn elif(&mut self, rest: &MappedText, name_span: RawSpan, directive_span: RawSpan) {
        let Some(frame) = self.conditions.last_mut() else {
            self.diags.push(
                PRE_ELIF_WITHOUT_IF,
                "#elif without a matching #if",
                name_span,
            );
            return;
        };
        if frame.else_seen {
            self.diags
                .push(PRE_ELIF_AFTER_ELSE, "#elif after #else", name_span);
            frame.active = false;
            return;
        }
        let parent_active = frame.parent_active;
        let any_taken = frame.any_taken;
        if !parent_active || any_taken {
            frame.active = false;
            return;
        }
        let opening = frame.opening;
        let mut active = false;
        if !has_expression_token(rest) {
            self.diags.push_with_related(
                PRE_MISSING_OPERAND,
                "#elif requires an expression",
                RawSpan::point(self.source, directive_span.end),
                vec![RawRelated {
                    span: opening,
                    label: "conditional directive",
                }],
            );
        } else {
            active = self.evaluate_condition(rest, opening);
        }
        let Some(frame) = self.conditions.last_mut() else {
            return;
        };
        frame.active = active;
        frame.any_taken |= active;
    }

    fn else_directive(&mut self, rest: &MappedText, name_span: RawSpan, directive_span: RawSpan) {
        if has_expression_token(rest) {
            self.diags.push(
                PRE_MALFORMED_DIRECTIVE,
                "unexpected tokens after #else",
                directive_span,
            );
        }
        let Some(frame) = self.conditions.last_mut() else {
            self.diags.push(
                PRE_ELSE_WITHOUT_IF,
                "#else without a matching #if",
                name_span,
            );
            return;
        };
        if frame.else_seen {
            self.diags
                .push(PRE_DUPLICATE_ELSE, "duplicate #else", name_span);
            frame.active = false;
            return;
        }
        frame.else_seen = true;
        frame.active = frame.parent_active && !frame.any_taken;
        frame.any_taken |= frame.active;
    }

    fn endif(&mut self, rest: &MappedText, name_span: RawSpan, directive_span: RawSpan) {
        if has_expression_token(rest) {
            self.diags.push(
                PRE_MALFORMED_DIRECTIVE,
                "unexpected tokens after #endif",
                directive_span,
            );
        }
        if self.conditions.pop().is_none() {
            self.diags.push(
                PRE_ENDIF_WITHOUT_IF,
                "#endif without a matching #if",
                name_span,
            );
        }
    }

    fn evaluate_condition(&mut self, rest: &MappedText, opening: RawSpan) -> bool {
        let (_, expression_span) = rest.trimmed();
        let before = self.diags.len();
        let pieces = tokenize_mapped(rest);
        let items = expand_condition(
            pieces,
            &self.macros,
            &mut self.prov,
            &mut self.diags,
            &mut self.limits,
            self.source,
        );
        let tokens = expr::tokenize(&items, &mut self.diags);
        let result = expr::evaluate(&tokens, expression_span, &mut self.diags);
        let after = self.diags.len();
        for index in before..after {
            self.diags
                .extend_related(index, opening, "conditional directive");
        }
        matches!(result, Ok(value) if value != 0)
    }

    fn finalize(mut self) -> Result<PreparedSource, SpanError> {
        let mut items: Vec<WorkItem> = Vec::new();
        for item in self.output.drain(..) {
            if item.text.is_empty() {
                continue;
            }
            let mut separate = false;
            if let Some(last) = items.last() {
                let contiguous_direct = last.origin == WorkOrigin::Direct
                    && item.origin == WorkOrigin::Direct
                    && last.span.source == item.span.source
                    && last.span.end == item.span.start;
                if !contiguous_direct
                    && last.kind != PieceKind::Comment
                    && item.kind != PieceKind::Comment
                    && needs_separator(&last.text, &item.text)
                {
                    separate = true;
                }
            }
            if separate {
                let last = items.last().expect("separator needs a predecessor");
                let anchor = RawSpan::point(self.source, last.span.end.min(self.text.len()));
                let node = self.prov.add(ProvNode {
                    kind: OriginKind::Generated,
                    span: anchor,
                    invocation: None,
                    definition: None,
                    parent: None,
                    label: Some("generated separator"),
                });
                let accepted = {
                    let limits = &mut self.limits;
                    let diags = &mut self.diags;
                    limits.charge(1, 1, diags, anchor)
                };
                if !accepted {
                    self.stopped = true;
                    break;
                }
                items.push(WorkItem {
                    text: " ".to_owned(),
                    kind: PieceKind::Whitespace,
                    span: anchor,
                    origin: WorkOrigin::Node(node),
                });
            }
            if let Some(last) = items.last_mut() {
                let can_merge = last.origin == WorkOrigin::Direct
                    && item.origin == WorkOrigin::Direct
                    && last.span.source == item.span.source
                    && last.span.end == item.span.start;
                if can_merge {
                    last.text.push_str(&item.text);
                    last.span.end = item.span.end;
                    continue;
                }
            }
            items.push(item);
        }

        let mut final_bytes = 0usize;
        let mut final_tokens = 0usize;
        let mut cut = None;
        for (index, item) in items.iter().enumerate() {
            final_bytes = final_bytes.saturating_add(item.text.len());
            final_tokens += 1;
            if final_bytes > MAX_EXPANDED_BYTES {
                if !self.limits.bytes_limit_reported() {
                    self.diags.push(
                        crate::preprocessor::diagnostics::PRE_EXPANDED_BYTES_LIMIT,
                        format!("prepared output exceeds {MAX_EXPANDED_BYTES} bytes"),
                        item.span,
                    );
                }
                cut = Some(index);
                break;
            }
            if final_tokens > MAX_EXPANDED_TOKENS {
                if !self.limits.tokens_limit_reported() {
                    self.diags.push(
                        crate::preprocessor::diagnostics::PRE_EXPANDED_TOKENS_LIMIT,
                        format!("prepared output exceeds {MAX_EXPANDED_TOKENS} tokens"),
                        item.span,
                    );
                }
                cut = Some(index);
                break;
            }
        }
        if let Some(index) = cut {
            items.truncate(index);
        }

        let mut text = String::new();
        for item in &items {
            text.push_str(&item.text);
        }
        let text: Arc<str> = Arc::from(text);
        let files = std::mem::take(&mut self.files);
        let mut builder = SourceMapBuilder::new(self.unit, Arc::clone(&text), files)?;
        let mut origin_ids: Vec<crate::source::OriginId> =
            Vec::with_capacity(self.prov.nodes.len());
        for node in &self.prov.nodes {
            let span = builder.source_span(node.span.source, node.span.start, node.span.end)?;
            let invocation = match node.invocation {
                Some(value) => Some(builder.source_span(value.source, value.start, value.end)?),
                None => None,
            };
            let definition = match node.definition {
                Some(value) => Some(builder.source_span(value.source, value.start, value.end)?),
                None => None,
            };
            let parent = match node.parent {
                Some(parent) => Some(*origin_ids.get(parent.0).ok_or(SpanError::MapInvariant)?),
                None => None,
            };
            let id =
                builder.push_origin(node.kind, span, invocation, definition, parent, node.label)?;
            origin_ids.push(id);
        }

        let mut offset = 0usize;
        for item in &items {
            let end = offset + item.text.len();
            let origin = match item.origin {
                WorkOrigin::Node(id) => *origin_ids.get(id.0).ok_or(SpanError::MapInvariant)?,
                WorkOrigin::Direct => {
                    let span =
                        builder.source_span(item.span.source, item.span.start, item.span.end)?;
                    builder.push_origin(
                        OriginKind::Direct,
                        span,
                        None,
                        None,
                        None,
                        Some("direct code"),
                    )?
                }
            };
            let prepared = PreparedSpan::new(self.unit, offset, end);
            builder.push_segment(prepared, origin)?;
            offset = end;
        }

        let diagnostics = self.materialize_diagnostics(&mut builder)?;
        let anchor_offset = if text.is_empty() {
            self.last_directive_end.unwrap_or(0).min(self.text.len())
        } else {
            self.text.len()
        };
        let anchor = builder.source_span(self.source, anchor_offset, anchor_offset)?;
        builder.set_eof_anchor(anchor)?;
        let map = Arc::new(builder.build()?);
        Ok(PreparedSource::new(self.unit, text, map, diagnostics))
    }

    fn materialize_diagnostics(
        &self,
        builder: &mut SourceMapBuilder,
    ) -> Result<Vec<PreprocessDiagnostic>, SpanError> {
        let mut diagnostics = Vec::with_capacity(self.diags.items().len());
        for RawDiagnostic {
            code,
            severity,
            message,
            primary,
            related,
        } in self.diags.items()
        {
            let primary = builder.source_span(primary.source, primary.start, primary.end)?;
            let mut rendered_related = Vec::with_capacity(related.len());
            for related in related {
                let span = builder.source_span(
                    related.span.source,
                    related.span.start,
                    related.span.end,
                )?;
                rendered_related.push(RelatedLocation::new(span, related.label));
            }
            diagnostics.push(PreprocessDiagnostic {
                code,
                severity: *severity,
                kind: DiagnosticKind::Syntax,
                message: message.clone(),
                primary,
                related: rendered_related,
            });
        }
        Ok(diagnostics)
    }
}

#[derive(Debug, Clone, Copy)]
enum ConditionalKind {
    If,
    Ifdef,
    Ifndef,
}

impl ConditionalKind {
    fn name(self) -> &'static str {
        match self {
            Self::If => "if",
            Self::Ifdef => "ifdef",
            Self::Ifndef => "ifndef",
        }
    }
}

pub(crate) fn preprocess_full(
    manager: &SourceManager,
    root: SourceId,
) -> Result<PreparedSource, SpanError> {
    let unit = manager.unit();
    if root.unit() != unit {
        return Err(SpanError::ForeignAnalysisUnit);
    }
    let file = manager.file(root).ok_or(SpanError::UnknownSourceId)?;
    let text = file.text();
    let processor = Preprocessor::new(unit, root, text, manager.files().to_vec());
    processor.run()
}

/// Recognize `#` after a block comment that began on an earlier physical line
/// and closes on this one. Returns `(hash, comment_end)`.
fn closing_comment_directive(text: &str, start: usize) -> Option<(usize, usize)> {
    let close = text[start..].find("*/")?;
    let comment_end = start + close + 2;
    if text[start..comment_end].contains('\n') || text[start..comment_end].contains('\r') {
        return None;
    }
    let mut index = comment_end;
    while index < text.len() {
        let ch = text[index..].chars().next().unwrap_or('\0');
        if ch == ' ' || ch == '\t' || ch == '\u{0c}' {
            index += ch.len_utf8();
        } else {
            break;
        }
    }
    if text[index..].starts_with('#') {
        Some((index, comment_end))
    } else {
        None
    }
}

fn continued_line(text: &str, start: usize) -> bool {
    if start < 2 {
        return false;
    }
    let bytes = text.as_bytes();
    if bytes[start - 1] == b'\n' {
        let before = if start >= 3 && bytes[start - 2] == b'\r' {
            start - 3
        } else {
            start - 2
        };
        bytes[before] == b'\\'
    } else if bytes[start - 1] == b'\r' {
        start >= 2 && bytes[start - 2] == b'\\'
    } else {
        false
    }
}

fn is_spliced_newline(text: &str, index: usize) -> bool {
    index > 0 && text.as_bytes()[index - 1] == b'\\'
}

fn directive_span(source: SourceId, text: &str, start: usize, end: usize) -> RawSpan {
    let mut content_end = end;
    if text[start..end].ends_with("\r\n") {
        content_end = end.saturating_sub(2);
    } else if text[start..end].ends_with('\n') || text[start..end].ends_with('\r') {
        content_end = end.saturating_sub(1);
    }
    RawSpan::new(source, start, content_end.max(start))
}

fn directive_hash(text: &str, start: usize) -> Option<usize> {
    let mut index = start;
    loop {
        while index < text.len() {
            let ch = text[index..].chars().next().unwrap_or('\0');
            if ch == ' ' || ch == '\t' || ch == '\u{0c}' {
                index += ch.len_utf8();
            } else {
                break;
            }
        }
        if index >= text.len() {
            return None;
        }
        if text[index..].starts_with("//") {
            return None;
        }
        if text[index..].starts_with("/*") {
            let close = text[index + 2..].find("*/")?;
            let segment = &text[index..index + 2 + close + 2];
            if segment.contains('\n') || segment.contains('\r') {
                return None;
            }
            index += 2 + close + 2;
            continue;
        }
        break;
    }
    if text[index..].starts_with('#') {
        Some(index)
    } else {
        None
    }
}

fn scan_line(text: &str, start: usize, in_block_comment: bool) -> (usize, bool) {
    let mut index = start;
    let mut in_block = in_block_comment;
    while index < text.len() {
        if in_block {
            if text[index..].starts_with("*/") {
                in_block = false;
                index += 2;
            } else {
                let ch = text[index..].chars().next().unwrap_or('\0');
                if ch == '\n' || ch == '\r' {
                    return (index + newline_len(text, index), in_block);
                }
                index += ch.len_utf8();
            }
            continue;
        }
        if text[index..].starts_with("//") {
            while index < text.len() {
                let ch = text[index..].chars().next().unwrap_or('\0');
                if ch == '\n' || ch == '\r' {
                    if is_spliced_newline(text, index) {
                        // Phase 2 line splicing precedes comment recognition:
                        // keep scanning this // comment across the physical line.
                        index += newline_len(text, index);
                        continue;
                    }
                    break;
                }
                index += ch.len_utf8();
            }
            continue;
        }
        if text[index..].starts_with("/*") {
            in_block = true;
            index += 2;
            continue;
        }
        let ch = text[index..].chars().next().unwrap_or('\0');
        if ch == '"' {
            index += 1;
            while index < text.len() {
                let next = text[index..].chars().next().unwrap_or('\0');
                if next == '\\' {
                    index += 1;
                    if index < text.len() {
                        let escaped = text[index..].chars().next().unwrap_or('\0');
                        index += escaped.len_utf8();
                    }
                } else if next == '"' {
                    index += 1;
                    break;
                } else if next == '\n' || next == '\r' {
                    break;
                } else {
                    index += next.len_utf8();
                }
            }
            continue;
        }
        if ch == '\n' || ch == '\r' {
            return (index + newline_len(text, index), in_block);
        }
        index += ch.len_utf8();
    }
    (text.len(), in_block)
}

fn newline_len(text: &str, index: usize) -> usize {
    if text[index..].starts_with("\r\n") {
        2
    } else {
        1
    }
}

fn last_newline(text: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    let slice = &text[start..end];
    if let Some(offset) = slice.rfind('\n') {
        let index = start + offset;
        if index > start && text.as_bytes()[index - 1] == b'\r' {
            Some((index - 1, index + 1))
        } else {
            Some((index, index + 1))
        }
    } else {
        slice
            .rfind('\r')
            .map(|offset| (start + offset, start + offset + 1))
    }
}

fn parse_parameters(rest: &MappedText) -> Option<(Vec<String>, usize)> {
    let text = rest.text();
    let mut position = 0usize;
    skip_directive_space(text, &mut position);
    if !text[position..].starts_with('(') {
        return None;
    }
    position += 1;
    let mut params = Vec::new();
    let mut first_parameter = true;
    loop {
        skip_directive_space(text, &mut position);
        if position >= text.len() {
            return None;
        }
        if text[position..].starts_with(')') {
            if !first_parameter {
                return None;
            }
            position += 1;
            break;
        }
        first_parameter = false;
        if text[position..].starts_with("...") {
            return None;
        }
        let start = position;
        let first = text[position..].chars().next()?;
        if !crate::preprocessor::token::is_identifier_start(first) {
            return None;
        }
        position += first.len_utf8();
        while position < text.len() {
            let ch = text[position..].chars().next()?;
            if !is_identifier_continue(ch) {
                break;
            }
            position += ch.len_utf8();
        }
        let param = text[start..position].to_owned();
        if params.contains(&param) {
            return None;
        }
        params.push(param);
        skip_directive_space(text, &mut position);
        if position >= text.len() {
            return None;
        }
        if text[position..].starts_with(',') {
            position += 1;
            skip_directive_space(text, &mut position);
            if position >= text.len() || text[position..].starts_with(')') {
                return None;
            }
            continue;
        }
        if text[position..].starts_with(')') {
            position += 1;
            break;
        }
        return None;
    }
    Some((params, position))
}

fn skip_directive_space(text: &str, position: &mut usize) {
    loop {
        while *position < text.len() {
            let ch = text[*position..].chars().next().unwrap_or('\0');
            if ch == ' ' || ch == '\t' || ch == '\u{0c}' {
                *position += ch.len_utf8();
            } else {
                break;
            }
        }
        if text[*position..].starts_with("/*") {
            if let Some(close) = text[*position + 2..].find("*/") {
                *position += 2 + close + 2;
                continue;
            }
        }
        if text[*position..].starts_with("//") {
            *position = text.len();
        }
        break;
    }
}

fn has_expression_token(rest: &MappedText) -> bool {
    tokenize_mapped(rest)
        .iter()
        .any(|piece| !matches!(piece.kind, PieceKind::Whitespace | PieceKind::Comment))
}

fn single_identifier(rest: &MappedText) -> Option<(String, RawSpan)> {
    let pieces = tokenize_mapped(rest);
    let mut identifier = None;
    for piece in pieces {
        if matches!(piece.kind, PieceKind::Whitespace | PieceKind::Comment) {
            continue;
        }
        if piece.kind != PieceKind::Identifier || identifier.is_some() {
            return None;
        }
        identifier = Some((piece.text, piece.span));
    }
    identifier
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preprocessor::config::PreprocessConfig;

    fn run(text: &str) -> PreparedSource {
        let unit = AnalysisUnitId::new(crate::source::RunId::new(900), 0);
        let mut manager = SourceManager::new(unit);
        let root = manager.load_root_text("root.lsl", text).unwrap();
        crate::preprocessor::preprocess(&manager, root, PreprocessConfig::full()).unwrap()
    }

    #[test]
    fn smoke_object_macro() {
        let prepared = run("#define VALUE 42\ninteger x = VALUE;\n");
        assert_eq!(prepared.text(), "\ninteger x = 42;\n");
        assert!(
            prepared.preprocess_diagnostics().is_empty(),
            "{:?}",
            prepared.preprocess_diagnostics()
        );
    }

    #[test]
    fn smoke_function_macro() {
        let prepared = run("#define ADD(a, b) ((a) + (b))\ninteger x = ADD(2, 3);\n");
        assert_eq!(prepared.text(), "\ninteger x = ((2) + (3));\n");
        assert!(
            prepared.preprocess_diagnostics().is_empty(),
            "{:?}",
            prepared.preprocess_diagnostics()
        );
    }

    #[test]
    fn smoke_if_else() {
        let prepared = run("#if 0\nbad\n#else\ngood\n#endif\n");
        assert_eq!(prepared.text(), "\n\n\ngood\n\n");
        assert!(
            prepared.preprocess_diagnostics().is_empty(),
            "{:?}",
            prepared.preprocess_diagnostics()
        );
    }

    #[test]
    fn every_point_resolves() {
        let prepared = run("#define A 1\nx A\n");
        for offset in 0..=prepared.prepared_len() {
            if prepared.text().is_char_boundary(offset) {
                assert!(prepared
                    .map()
                    .resolve_point(PreparedSpan::new(prepared.unit(), offset, offset))
                    .is_ok());
            }
        }
    }
}

#[cfg(test)]
mod phase2_tests {
    use super::*;
    use crate::preprocessor::config::PreprocessConfig;
    use crate::source::map::PreparedSpan;

    fn run(text: &str) -> PreparedSource {
        let unit = AnalysisUnitId::new(crate::source::RunId::new(901), 0);
        let mut manager = SourceManager::new(unit);
        let root = manager.load_root_text("root.lsl", text).unwrap();
        crate::preprocessor::preprocess(&manager, root, PreprocessConfig::full()).unwrap()
    }

    fn codes(prepared: &PreparedSource) -> Vec<&'static str> {
        prepared
            .preprocess_diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code)
            .collect()
    }

    fn original(prepared: &PreparedSource, span: crate::source::SourceSpan) -> &str {
        prepared.map().file(span.source()).unwrap().text()
    }

    fn text_slice(prepared: &PreparedSource, span: crate::source::SourceSpan) -> &str {
        &original(prepared, span)[span.start()..span.end()]
    }

    fn resolve(prepared: &PreparedSource, needle: &str) -> crate::source::ResolvedSpan {
        let start = prepared.text().find(needle).unwrap();
        prepared
            .map()
            .resolve_span(PreparedSpan::new(
                prepared.unit(),
                start,
                start + needle.len(),
            ))
            .unwrap()
    }

    #[test]
    fn plain_lsl_is_byte_identical() {
        let source = "integer x = 1;\r\n// comment\r\nstring s = \"#define X\";\r\n";
        let prepared = run(source);
        assert_eq!(prepared.text(), source);
        assert!(prepared.preprocess_diagnostics().is_empty());
    }

    #[test]
    fn hash_in_strings_and_comments_is_not_a_directive() {
        let source =
            "string a = \"#define X 1\";\n// #define Y 2\n/* #define Z 3 */\ninteger x = 4;\n";
        let prepared = run(source);
        assert_eq!(prepared.text(), source);
        assert!(prepared.preprocess_diagnostics().is_empty());
    }

    #[test]
    fn directive_after_spaces_and_comment() {
        let prepared = run("   #define X 1\n/* c */ #define Y 2\ninteger z = X + Y;\n");
        assert_eq!(prepared.text(), "\n\ninteger z = 1 + 2;\n");
    }

    #[test]
    fn multiline_comment_before_directive() {
        let prepared = run("/* multi\nline */\n#define X 5\nX\n");
        assert_eq!(prepared.text(), "/* multi\nline */\n\n5\n");
    }

    #[test]
    fn definition_continuation() {
        let prepared = run("#define SUM 1 + \\\n2\ninteger x = SUM;\n");
        assert_eq!(prepared.text(), "\ninteger x = 1 + 2;\n");
    }

    #[test]
    fn conditional_nested_and_inactive_define() {
        let source =
            "#if 0\n#define IGNORED 1\n#if 1\nnested\n#endif\n#else\nactive IGNORE\n#endif\n";
        let prepared = run(source);
        assert!(prepared.text().contains("active IGNORE"));
        assert!(!prepared.text().contains("nested"));
        assert!(!prepared.text().contains("#define"));
        assert!(
            prepared.preprocess_diagnostics().is_empty(),
            "{:?}",
            prepared.preprocess_diagnostics()
        );
    }

    #[test]
    fn elif_after_taken_branch_is_not_evaluated() {
        let source = "#if 1\nfirst\n#elif 1 / 0\nnever\n#else\nnever2\n#endif\n";
        let prepared = run(source);
        assert!(prepared.text().contains("first"));
        assert!(!prepared.text().contains("never"));
        assert!(
            prepared.preprocess_diagnostics().is_empty(),
            "{:?}",
            prepared.preprocess_diagnostics()
        );
    }

    #[test]
    fn defined_and_ifdef() {
        let source = "#define KNOWN 1\n#ifdef UNKNOWN\nbad\n#elif defined(KNOWN)\ngood\n#else\nbad2\n#endif\n#ifndef MISSING\nyes\n#endif\n";
        let prepared = run(source);
        assert!(prepared.text().contains("good"));
        assert!(prepared.text().contains("yes"));
        assert!(!prepared.text().contains("bad"));
    }

    #[test]
    fn function_macro_nested_calls_and_arguments() {
        let source = "#define TWICE(x) ((x) + (x))\n#define ADD(a, b) ((a) + (b))\ninteger x = ADD(TWICE(2), TWICE(3));\n";
        let prepared = run(source);
        assert_eq!(
            prepared.text(),
            "\n\ninteger x = ((((2) + (2))) + (((3) + (3))));\n"
        );
    }

    #[test]
    fn empty_macro_and_separator() {
        let prepared = run("#define EMPTY\ninteger x = 1 EMPTY 2;\n");
        assert_eq!(prepared.text(), "\ninteger x = 1  2;\n");
    }

    #[test]
    fn recursive_macro_is_diagnosed_once() {
        let prepared = run("#define A A\nA\n");
        assert!(prepared.text().contains("A"));
        assert_eq!(codes(&prepared), vec!["PRE017"]);
    }

    #[test]
    fn indirect_recursion_is_diagnosed() {
        let prepared = run("#define A B\n#define B A\nA\n");
        assert_eq!(codes(&prepared), vec!["PRE018"]);
    }

    #[test]
    fn wrong_argument_count_is_diagnosed() {
        let source = "#define F(a, b) a + b\nF(1)\n";
        let prepared = run(source);
        assert_eq!(codes(&prepared), vec!["PRE014"]);
        assert!(prepared.text().contains("F(1)"));
    }

    #[test]
    fn undef_and_identical_redefinition() {
        let source = "#define X 1\n#define X 1\n#undef X\nX\n#undef MISSING\n";
        let prepared = run(source);
        assert_eq!(prepared.text(), "\n\n\nX\n\n");
        assert!(prepared.preprocess_diagnostics().is_empty());
    }

    #[test]
    fn conflicting_redefinition_is_diagnosed() {
        let source = "#define X 1\n#define X 2\nX\n";
        let prepared = run(source);
        assert_eq!(codes(&prepared), vec!["PRE005"]);
        assert!(prepared.text().contains('1'));
    }

    #[test]
    fn unterminated_conditional_has_opening_and_eof() {
        let prepared = run("#if 1\nx\n");
        assert_eq!(codes(&prepared), vec!["PRE013"]);
        let diagnostic = &prepared.preprocess_diagnostics()[0];
        assert_eq!(text_slice(&prepared, diagnostic.primary), "#if 1");
        assert_eq!(diagnostic.related.len(), 1);
        assert_eq!(diagnostic.related[0].span.start(), "#if 1\nx\n".len());
    }

    #[test]
    fn macro_provenance_resolves_definition_and_invocation() {
        let source = "#define VALUE 42\ninteger x = VALUE;\n";
        let prepared = run(source);
        let resolved = resolve(&prepared, "42");
        assert_eq!(text_slice(&prepared, resolved.primary()), "42");
        assert!(resolved
            .related()
            .iter()
            .any(|span| text_slice(&prepared, *span) == "VALUE"));
        assert!(resolved
            .related()
            .iter()
            .any(|span| text_slice(&prepared, *span) == "#define VALUE 42"));
    }

    #[test]
    fn empty_conditional_output_has_eof_anchor() {
        let prepared = run("#if 0");
        assert_eq!(prepared.text(), "");
        assert_eq!(prepared.map().eof_anchor().start(), 5);
    }
}

#[cfg(test)]
mod phase2_extra_tests {
    use super::*;
    use crate::preprocessor::config::PreprocessConfig;
    use crate::source::map::PreparedSpan;

    fn run(text: &str) -> PreparedSource {
        let unit = AnalysisUnitId::new(crate::source::RunId::new(902), 0);
        let mut manager = SourceManager::new(unit);
        let root = manager.load_root_text("root.lsl", text).unwrap();
        crate::preprocessor::preprocess(&manager, root, PreprocessConfig::full()).unwrap()
    }

    fn codes(prepared: &PreparedSource) -> Vec<&'static str> {
        prepared
            .preprocess_diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code)
            .collect()
    }

    #[test]
    fn expression_conditionals_with_macros() {
        let source = "#define FLAG 0\n#define EQ(a, b) ((a) == (b))\n#if defined(FLAG) && EQ(FLAG, 0)\nyes\n#else\nno\n#endif\n#if 0 && (1 / 0)\nno\n#endif\n#if 1 || (1 / 0)\nyes2\n#endif\n";
        let prepared = run(source);
        assert!(prepared.text().contains("yes\n"));
        assert!(prepared.text().contains("yes2"));
        assert!(!prepared.text().contains("no\n"));
        assert!(
            prepared.preprocess_diagnostics().is_empty(),
            "{:?}",
            prepared.preprocess_diagnostics()
        );
    }

    #[test]
    fn structural_conditional_errors() {
        let prepared = run("#else\n#endif\n#elif 1\n");
        assert_eq!(codes(&prepared), vec!["PRE011", "PRE012", "PRE010"]);
    }

    #[test]
    fn duplicate_else_and_elif_after_else() {
        let prepared = run("#if 1\n#else\n#else\n#endif\n#if 0\n#else\n#elif 1\n#endif\n");
        assert_eq!(codes(&prepared), vec!["PRE008", "PRE009"]);
    }

    #[test]
    fn malformed_directive_recovers() {
        let source = "#define\n#define GOOD 1\n#undef\n#undef GOOD\nGOOD\n";
        let prepared = run(source);
        assert_eq!(codes(&prepared), vec!["PRE003", "PRE004"]);
        assert!(prepared.text().contains("GOOD"));
    }

    #[test]
    fn unsupported_directives_and_operators_are_diagnosed() {
        let source = "#include \"x\"\n#pragma once\n#define BAD(a) #a\n#define PASTE(a) a ## a\n";
        let prepared = run(source);
        assert_eq!(
            codes(&prepared),
            vec!["PRE024", "PRE024", "PRE023", "PRE023"]
        );
    }

    #[test]
    fn function_macros_with_zero_and_nested_arguments() {
        let source = "#define ZERO() 0\n#define ID(x) x\n#define SUM(a, b) ((a) + (b))\nZERO() ID(SUM((1 + 2), (3 + 4)))\n";
        let prepared = run(source);
        assert!(
            prepared.text().contains("0 (((1 + 2)) + ((3 + 4)))"),
            "{}",
            prepared.text()
        );
        assert!(
            prepared.preprocess_diagnostics().is_empty(),
            "{:?}",
            prepared.preprocess_diagnostics()
        );
    }

    #[test]
    fn macros_do_not_expand_in_strings_or_comments() {
        let source = "#define X 7\nstring s = \"X\"; // X\n/* X */ X\n";
        let prepared = run(source);
        assert!(prepared.text().contains("\"X\""));
        assert!(prepared.text().contains("// X"));
        assert!(prepared.text().contains("/* X */ 7"));
    }

    #[test]
    fn macro_expansion_separates_adjacent_tokens() {
        let prepared = run("#define A x\n#define B y\nA B\n");
        assert!(prepared.text().contains("x y"));
        let prepared = run("#define PLUS +\n1 PLUS + 2\n");
        assert!(prepared.text().contains("1 + + 2"));
    }

    #[test]
    fn macro_argument_origin_is_the_argument_text() {
        let source = "#define ID(x) x\nID(42)\n";
        let prepared = run(source);
        let start = prepared.text().find("42").unwrap();
        let resolved = prepared
            .map()
            .resolve_span(PreparedSpan::new(prepared.unit(), start, start + 2))
            .unwrap();
        assert_eq!(
            &prepared
                .map()
                .file(resolved.primary().source())
                .unwrap()
                .text()[resolved.primary().start()..resolved.primary().end()],
            "42"
        );
        assert!(!resolved.related().is_empty());
    }

    #[test]
    fn crlf_and_lone_cr_directives() {
        let prepared = run("#define X 1\r\nX\rX\r\n");
        assert!(prepared.text().contains("1"));
        assert!(prepared.text().contains('\r'));
    }
}

#[cfg(test)]
mod phase2_adversarial_tests {
    use super::*;
    use crate::preprocessor::config::PreprocessConfig;

    fn run(text: &str) -> PreparedSource {
        let unit = AnalysisUnitId::new(crate::source::RunId::new(903), 0);
        let mut manager = SourceManager::new(unit);
        let root = manager.load_root_text("root.lsl", text).unwrap();
        crate::preprocessor::preprocess(&manager, root, PreprocessConfig::full()).unwrap()
    }

    fn codes(prepared: &PreparedSource) -> Vec<&'static str> {
        prepared
            .preprocess_diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code)
            .collect()
    }

    #[test]
    fn conditional_depth_limit() {
        let mut source = String::new();
        for _ in 0..MAX_CONDITIONAL_DEPTH {
            source.push_str("#if 1\n");
        }
        source.push_str("x\n");
        for _ in 0..MAX_CONDITIONAL_DEPTH {
            source.push_str("#endif\n");
        }
        let prepared = run(&source);
        assert!(prepared.text().contains('x'));
        assert!(
            prepared.preprocess_diagnostics().is_empty(),
            "{:?}",
            prepared.preprocess_diagnostics()
        );

        let mut source = String::new();
        for _ in 0..=MAX_CONDITIONAL_DEPTH {
            source.push_str("#if 1\n");
        }
        source.push_str("x\n");
        for _ in 0..=MAX_CONDITIONAL_DEPTH {
            source.push_str("#endif\n");
        }
        let prepared = run(&source);
        assert!(!prepared.text().contains('x'));
        assert!(
            codes(&prepared).contains(&"PRE025"),
            "{:?}",
            codes(&prepared)
        );
    }

    #[test]
    fn macro_depth_limit() {
        let mut source = String::new();
        for index in 0..63 {
            source.push_str(&format!("#define M{index} M{}\n", index + 1));
        }
        source.push_str("#define M63 1\nM0\n");
        let prepared = run(&source);
        assert!(prepared.text().contains('1'), "{}", prepared.text());
        assert!(
            prepared.preprocess_diagnostics().is_empty(),
            "{:?}",
            prepared.preprocess_diagnostics()
        );

        let mut source = String::new();
        for index in 0..=63 {
            source.push_str(&format!("#define N{index} N{}\n", index + 1));
        }
        source.push_str("#define N64 1\nN0\n");
        let prepared = run(&source);
        assert!(prepared.text().contains("N64"), "{}", prepared.text());
        assert!(
            codes(&prepared).contains(&"PRE019"),
            "{:?}",
            codes(&prepared)
        );
    }

    #[test]
    fn diagnostic_cap_is_enforced() {
        let mut source = String::new();
        for _ in 0..1_100 {
            source.push_str("#unknown\n");
        }
        let prepared = run(&source);
        assert_eq!(prepared.preprocess_diagnostics().len(), 1000);
        assert_eq!(
            prepared.preprocess_diagnostics().last().unwrap().code,
            "PRE022"
        );
    }

    #[test]
    fn deeply_nested_expression_is_rejected_without_stack_overflow() {
        let mut expression = String::new();
        for _ in 0..200 {
            expression.push('(');
        }
        expression.push('1');
        for _ in 0..200 {
            expression.push(')');
        }
        let prepared = run(&format!("#if {expression}\n#endif\n"));
        assert!(
            codes(&prepared).contains(&"PRE006"),
            "{:?}",
            codes(&prepared)
        );
    }
}

#[cfg(test)]
mod phase2_map_tests {
    use super::*;
    use crate::preprocessor::config::PreprocessConfig;
    use crate::source::map::PreparedSpan;

    fn run(text: &str) -> PreparedSource {
        let unit = AnalysisUnitId::new(crate::source::RunId::new(904), 0);
        let mut manager = SourceManager::new(unit);
        let root = manager.load_root_text("root.lsl", text).unwrap();
        crate::preprocessor::preprocess(&manager, root, PreprocessConfig::full()).unwrap()
    }

    fn original_text(prepared: &PreparedSource, span: crate::source::SourceSpan) -> &str {
        &prepared.map().file(span.source()).unwrap().text()[span.start()..span.end()]
    }

    #[test]
    fn every_byte_of_complex_output_resolves() {
        let source = "#define TWICE(x) ((x) + (x))\n#define ADD(a, b) ((a) + (b))\n#define EMPTY\ninteger x = ADD(TWICE(1), EMPTY 2); string s = \"TWICE\";\n";
        let prepared = run(source);
        assert!(prepared.text().len() < 500);
        for start in 0..prepared.text().len() {
            if !prepared.text().is_char_boundary(start) {
                continue;
            }
            for end in start + 1..=prepared.text().len() {
                if !prepared.text().is_char_boundary(end) {
                    continue;
                }
                assert!(
                    prepared
                        .map()
                        .resolve_span(PreparedSpan::new(prepared.unit(), start, end))
                        .is_ok(),
                    "unresolved prepared range {start}..{end}: {:?}",
                    &prepared.text()[start..end]
                );
            }
        }
        assert!(prepared
            .map()
            .resolve_span(PreparedSpan::new(
                prepared.unit(),
                0,
                prepared.prepared_len()
            ))
            .is_ok());
    }

    #[test]
    fn nested_invocations_keep_definition_and_invocation_chain() {
        let source =
            "#define TWICE(x) ((x) + (x))\n#define ADD(a, b) ((a) + (b))\nADD(TWICE(7), 2)\n";
        let prepared = run(source);
        let start = prepared.text().find('7').unwrap();
        let resolved = prepared
            .map()
            .resolve_span(PreparedSpan::new(prepared.unit(), start, start + 1))
            .unwrap();
        assert_eq!(original_text(&prepared, resolved.primary()), "7");
        let related: Vec<&str> = resolved
            .related()
            .iter()
            .map(|span| original_text(&prepared, *span))
            .collect();
        assert!(related.contains(&"TWICE"), "{related:?}");
        assert!(
            related.iter().any(|text| text.starts_with("ADD(")),
            "{related:?}"
        );
        assert!(
            related.iter().any(|text| text.contains("#define TWICE")),
            "{related:?}"
        );
        assert!(
            related.iter().any(|text| text.contains("#define ADD")),
            "{related:?}"
        );
    }

    #[test]
    fn definition_primary_and_invocation_related() {
        let source = "#define VALUE 42\nVALUE\n";
        let prepared = run(source);
        let start = prepared.text().find("42").unwrap();
        let resolved = prepared
            .map()
            .resolve_span(PreparedSpan::new(prepared.unit(), start, start + 2))
            .unwrap();
        assert_eq!(original_text(&prepared, resolved.primary()), "42");
        let related: Vec<&str> = resolved
            .related()
            .iter()
            .map(|span| original_text(&prepared, *span))
            .collect();
        assert!(related.contains(&"VALUE"), "{related:?}");
    }

    #[test]
    fn generated_separator_has_generated_origin_anchor() {
        let source = "#define A +\n+A\n";
        let prepared = run(source);
        assert!(prepared.text().contains("+ +"), "{}", prepared.text());
        for offset in 0..prepared.prepared_len() {
            if prepared.text().is_char_boundary(offset) {
                assert!(prepared
                    .map()
                    .resolve_span(PreparedSpan::new(prepared.unit(), offset, offset + 1))
                    .is_ok());
            }
        }
    }

    #[test]
    fn empty_source_has_anchor_at_zero() {
        let prepared = run("");
        assert_eq!(prepared.text(), "");
        assert_eq!(prepared.map().eof_anchor().start(), 0);
        assert_eq!(prepared.preprocess_diagnostics().len(), 0);
    }
}

#[cfg(test)]
mod phase2_contract_tests {
    use super::*;
    use crate::preprocessor::config::PreprocessConfig;

    fn run(text: &str) -> PreparedSource {
        let unit = AnalysisUnitId::new(crate::source::RunId::new(905), 0);
        let mut manager = SourceManager::new(unit);
        let root = manager.load_root_text("root.lsl", text).unwrap();
        crate::preprocessor::preprocess(&manager, root, PreprocessConfig::full()).unwrap()
    }

    fn codes(prepared: &PreparedSource) -> Vec<&'static str> {
        prepared
            .preprocess_diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code)
            .collect()
    }

    #[test]
    #[allow(clippy::default_constructed_unit_structs)]
    fn identity_mode_delegates_to_phase_one() {
        let unit = AnalysisUnitId::new(crate::source::RunId::new(906), 0);
        let mut manager = SourceManager::new(unit);
        let root = manager
            .load_root_text("root.lsl", "integer x = 1;\n")
            .unwrap();
        let identity = crate::preprocessor::identity::preprocess_identity(&manager, root).unwrap();
        let configured =
            crate::preprocessor::preprocess(&manager, root, PreprocessConfig::identity()).unwrap();
        assert_eq!(configured.text(), identity.text());
        assert_eq!(
            configured.map().prepared_len(),
            identity.map().prepared_len()
        );
        assert_eq!(configured.map().eof_anchor(), identity.map().eof_anchor());
        assert!(configured.preprocess_diagnostics().is_empty());
        assert_eq!(PreprocessConfig::default(), PreprocessConfig::identity());
        let legacy = PreprocessConfig;
        let legacy_prepared = crate::preprocessor::preprocess(&manager, root, legacy).unwrap();
        assert_eq!(legacy_prepared.text(), identity.text());
    }

    #[test]
    fn object_macro_requires_no_paren_and_function_macro_allows_space() {
        let source = "#define OBJECT (1)\n#define FUNC(x) x\nOBJECT FUNC (2)\n";
        let prepared = run(source);
        assert!(prepared.text().contains("(1) 2"), "{}", prepared.text());
    }

    #[test]
    fn argument_parsing_handles_nesting_and_commas() {
        let source = "#define SECOND(a, b) b\nSECOND((1, 2), (3 + 4))\n";
        let prepared = run(source);
        assert!(prepared.text().contains("(3 + 4)"), "{}", prepared.text());
        assert!(prepared.preprocess_diagnostics().is_empty());
    }

    #[test]
    fn unterminated_macro_call_is_diagnosed_and_preserved() {
        let prepared = run("#define F(a) a\nF(1\n");
        assert_eq!(codes(&prepared), vec!["PRE015"]);
        assert!(prepared.text().contains("F(1"));
    }

    #[test]
    fn macro_name_without_call_and_comments_between_name_and_paren() {
        let prepared = run("#define F(x) x\nF + F/*c*/(1)\n");
        assert!(prepared.text().contains("F + 1"), "{}", prepared.text());
    }

    #[test]
    fn unselected_elif_is_not_evaluated_even_when_malformed() {
        let prepared = run("#if 1\na\n#elif (\nno\n#else\nno\n#endif\n");
        assert!(prepared.text().contains('a'));
        assert!(
            prepared.preprocess_diagnostics().is_empty(),
            "{:?}",
            prepared.preprocess_diagnostics()
        );
    }

    #[test]
    fn defined_operand_is_not_macro_expanded() {
        let source = "#define NAME VALUE\n#define VALUE 1\n#if defined(NAME) && NAME\nok\n#endif\n#if defined(MISSING)\nno\n#endif\n";
        let prepared = run(source);
        assert!(prepared.text().contains("ok"));
        assert!(!prepared.text().contains("no"));
        assert!(
            prepared.preprocess_diagnostics().is_empty(),
            "{:?}",
            prepared.preprocess_diagnostics()
        );
    }

    #[test]
    fn conditional_expression_continuation() {
        let prepared = run("#if 1 + \\\n1 == 2\nyes\n#endif\n");
        assert!(prepared.text().contains("yes"));
    }

    #[test]
    fn invalid_expression_sets_false_and_recovers() {
        let prepared = run("#if 1 +\nno\n#else\nyes\n#endif\n");
        assert!(!prepared.text().contains("no\n"));
        assert!(prepared.text().contains("yes"));
        assert!(codes(&prepared).contains(&"PRE006"));
    }

    #[test]
    fn hash_mid_line_is_not_a_directive() {
        let source = "integer x = 1 # 2;\n#\n";
        let prepared = run(source);
        assert!(prepared.text().contains("integer x = 1 # 2;"));
        assert_eq!(codes(&prepared), vec!["PRE002"]);
    }
}

#[cfg(test)]
mod phase2_limit_tests {
    use super::*;
    use crate::preprocessor::config::PreprocessConfig;

    fn run(text: &str) -> PreparedSource {
        let unit = AnalysisUnitId::new(crate::source::RunId::new(907), 0);
        let mut manager = SourceManager::new(unit);
        let root = manager.load_root_text("root.lsl", text).unwrap();
        crate::preprocessor::preprocess(&manager, root, PreprocessConfig::full()).unwrap()
    }

    #[test]
    fn explosive_but_acyclic_expansion_stops_at_a_limit() {
        let mut source = String::new();
        for index in 0..10 {
            source.push_str(&format!("#define M{index}(x) x x x x\n"));
        }
        let mut invocation = "1".to_owned();
        for index in (0..10).rev() {
            invocation = format!("M{index}({invocation})");
        }
        source.push_str(&invocation);
        source.push('\n');
        let prepared = run(&source);
        let codes: Vec<_> = prepared
            .preprocess_diagnostics()
            .iter()
            .map(|item| item.code)
            .collect();
        assert!(
            codes.contains(&"PRE021") || codes.contains(&"PRE020"),
            "{codes:?}"
        );
        assert!(prepared.text().len() <= 8 * 1024 * 1024);
    }
}

#[cfg(test)]
mod phase2_comment_tests {
    use super::*;
    use crate::preprocessor::config::PreprocessConfig;

    fn run(text: &str) -> PreparedSource {
        let unit = AnalysisUnitId::new(crate::source::RunId::new(908), 0);
        let mut manager = SourceManager::new(unit);
        let root = manager.load_root_text("root.lsl", text).unwrap();
        crate::preprocessor::preprocess(&manager, root, PreprocessConfig::full()).unwrap()
    }

    #[test]
    fn comments_are_whitespace_inside_directives() {
        let source = "# /*a*/ define /*b*/ X 1\n#ifdef /*c*/ X\nyes\n#endif\n#undef /*d*/ X\n#if defined /*e*/ (X)\nno\n#endif\n";
        let prepared = run(source);
        assert!(prepared.text().contains("yes"));
        assert!(!prepared.text().contains("no"));
        assert!(
            prepared.preprocess_diagnostics().is_empty(),
            "{:?}",
            prepared.preprocess_diagnostics()
        );
    }

    #[test]
    fn zero_parameter_call_accepts_whitespace() {
        let prepared = run("#define Z( ) 0\nZ( )\n");
        assert!(prepared.text().contains('0'));
        assert!(prepared.preprocess_diagnostics().is_empty());
    }
}

#[cfg(test)]
mod phase2_state_tests {
    use super::*;
    use crate::preprocessor::config::PreprocessConfig;

    fn run(text: &str) -> PreparedSource {
        let unit = AnalysisUnitId::new(crate::source::RunId::new(909), 0);
        let mut manager = SourceManager::new(unit);
        let root = manager.load_root_text("root.lsl", text).unwrap();
        crate::preprocessor::preprocess(&manager, root, PreprocessConfig::full()).unwrap()
    }

    fn codes(prepared: &PreparedSource) -> Vec<&'static str> {
        prepared
            .preprocess_diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code)
            .collect()
    }

    #[test]
    fn macro_table_changes_take_effect_at_the_definition_point() {
        let source = "BEFORE\n#define BEFORE 1\nBEFORE\n#undef BEFORE\nBEFORE\n";
        let prepared = run(source);
        assert!(
            prepared.text().starts_with("BEFORE\n"),
            "{}",
            prepared.text()
        );
        assert!(prepared.text().contains("\n1\n"), "{}", prepared.text());
        assert_eq!(prepared.text().matches("BEFORE").count(), 2);
    }

    #[test]
    fn malformed_directives_in_inactive_blocks_are_not_reported() {
        let prepared = run("#if 0\n#\n#bogus\n#definitely broken\n#endif\n");
        assert!(
            prepared.preprocess_diagnostics().is_empty(),
            "{:?}",
            prepared.preprocess_diagnostics()
        );
    }

    #[test]
    fn utf8_and_multibyte_offsets_survive_expansion() {
        let source = "string s = \"привет\";\n#define X 42\ninteger x = X; // é\n";
        let prepared = run(source);
        assert_eq!(
            prepared.text(),
            "string s = \"привет\";\n\ninteger x = 42; // é\n"
        );
        assert!(prepared.preprocess_diagnostics().is_empty());
    }

    #[test]
    fn conditional_define_is_visible_only_in_selected_branch() {
        let source = "#if 1\n#define YES 1\n#else\n#define NO 1\n#endif\n#ifdef YES\nyes\n#endif\n#ifdef NO\nno\n#endif\n";
        let prepared = run(source);
        assert!(prepared.text().contains("yes"));
        assert!(!prepared.text().contains("no\n"));
        assert!(codes(&prepared).is_empty());
    }
}

#[cfg(test)]
mod phase2_rescan_tests {
    use super::*;
    use crate::preprocessor::config::PreprocessConfig;

    fn run(text: &str) -> PreparedSource {
        let unit = AnalysisUnitId::new(crate::source::RunId::new(910), 0);
        let mut manager = SourceManager::new(unit);
        let root = manager.load_root_text("root.lsl", text).unwrap();
        crate::preprocessor::preprocess(&manager, root, PreprocessConfig::full()).unwrap()
    }

    #[test]
    fn object_macro_expands_to_a_function_macro_call_in_its_body() {
        let prepared = run("#define F(x) x\n#define CALL F(1)\nCALL\n");
        assert!(prepared.text().contains('1'), "{}", prepared.text());
    }

    #[test]
    fn generated_function_macro_name_can_join_following_parenthesis() {
        let prepared = run("#define F(x) x\n#define NAME F\nNAME(1)\n");
        assert_eq!(prepared.text(), "\n\n1\n");
    }
}

#[cfg(test)]
mod phase2_isolation_tests {
    use super::*;
    use crate::preprocessor::config::PreprocessConfig;

    fn run(text: &str) -> PreparedSource {
        let unit = AnalysisUnitId::new(crate::source::RunId::new(911), 0);
        let mut manager = SourceManager::new(unit);
        let root = manager.load_root_text("root.lsl", text).unwrap();
        crate::preprocessor::preprocess(&manager, root, PreprocessConfig::full()).unwrap()
    }

    #[test]
    fn repeated_runs_are_byte_and_diagnostic_identical() {
        let source =
            "#define F(x) ((x) + (x))\n#if defined(F) && F(1) == 2\nF(2)\n#else\nbad\n#endif\n";
        let first = run(source);
        let second = run(source);
        assert_eq!(first.text(), second.text());
        assert_eq!(
            first.preprocess_diagnostics(),
            second.preprocess_diagnostics()
        );
    }

    #[test]
    fn macro_tables_do_not_cross_units() {
        let first = run("#define LEAK 1\nLEAK\n");
        assert!(first.text().contains('1'));
        let second = run("LEAK\n");
        assert_eq!(second.text(), "LEAK\n");
    }

    #[test]
    fn huge_argument_list_is_diagnosed_without_panic() {
        let mut call = String::from("F(");
        for index in 0..2_000 {
            if index > 0 {
                call.push(',');
            }
            call.push('1');
        }
        call.push_str(")\n");
        let prepared = run(&format!("#define F() 0\n{call}"));
        assert!(prepared
            .preprocess_diagnostics()
            .iter()
            .any(|item| item.code == "PRE014"));
    }

    #[test]
    fn deeply_parenthesized_macro_argument_is_safe() {
        let mut argument = String::new();
        for _ in 0..2_000 {
            argument.push('(');
        }
        argument.push('1');
        for _ in 0..2_000 {
            argument.push(')');
        }
        let prepared = run(&format!("#define ID(x) x\nID({argument})\n"));
        assert!(prepared.text().contains('1'));
        assert_eq!(prepared.preprocess_diagnostics().len(), 0);
    }
}

#[cfg(test)]
mod phase2_comment_close_tests {
    use super::*;
    use crate::preprocessor::config::PreprocessConfig;

    fn run(text: &str) -> PreparedSource {
        let unit = AnalysisUnitId::new(crate::source::RunId::new(912), 0);
        let mut manager = SourceManager::new(unit);
        let root = manager.load_root_text("root.lsl", text).unwrap();
        crate::preprocessor::preprocess(&manager, root, PreprocessConfig::full()).unwrap()
    }

    #[test]
    fn directive_after_multiline_comment_close_on_the_same_line() {
        let source = "/* multi\nline */ #define X 7\nX\n";
        let prepared = run(source);
        assert!(
            prepared.text().contains("/* multi\nline */"),
            "{}",
            prepared.text()
        );
        assert!(prepared.text().contains('7'), "{}", prepared.text());
        assert!(prepared.preprocess_diagnostics().is_empty());
    }
}

#[cfg(test)]
mod phase2_function_depth_tests {
    use super::*;
    use crate::preprocessor::config::PreprocessConfig;

    #[test]
    fn function_macro_expansion_at_depth_64_succeeds() {
        let mut source = String::new();
        for index in 0..63 {
            source.push_str(&format!("#define M{index}(x) M{}(x)\n", index + 1));
        }
        source.push_str("#define M63(x) x\nM0(1)\n");
        let unit = AnalysisUnitId::new(crate::source::RunId::new(913), 0);
        let mut manager = SourceManager::new(unit);
        let root = manager.load_root_text("root.lsl", &source).unwrap();
        let prepared =
            crate::preprocessor::preprocess(&manager, root, PreprocessConfig::full()).unwrap();
        assert!(prepared.text().contains('1'), "{}", prepared.text());
        assert!(
            prepared.preprocess_diagnostics().is_empty(),
            "{:?}",
            prepared.preprocess_diagnostics()
        );
    }
}

#[cfg(test)]
mod phase2_diagnostic_code_tests {
    use super::*;
    use crate::preprocessor::config::PreprocessConfig;

    fn codes(text: &str) -> Vec<&'static str> {
        let unit = AnalysisUnitId::new(crate::source::RunId::new(914), 0);
        let mut manager = SourceManager::new(unit);
        let root = manager.load_root_text("root.lsl", text).unwrap();
        crate::preprocessor::preprocess(&manager, root, PreprocessConfig::full())
            .unwrap()
            .preprocess_diagnostics()
            .iter()
            .map(|item| item.code)
            .collect()
    }

    #[test]
    fn required_diagnostic_categories_have_codes() {
        assert!(codes("#bogus\n").contains(&"PRE001"));
        assert!(codes("#\n").contains(&"PRE002"));
        assert!(codes("#define\n").contains(&"PRE003"));
        assert!(codes("#define F(...) x\n").contains(&"PRE003"));
        assert!(codes("#define F(a,) x\n").contains(&"PRE003"));
        assert!(codes("#undef\n").contains(&"PRE004"));
        assert!(codes("#undef X Y\n").contains(&"PRE004"));
        assert!(codes("#ifdef\n#endif\n").contains(&"PRE007"));
        assert!(codes("#if\n#endif\n").contains(&"PRE030"));
        assert!(codes("#if 1 +\n#endif\n").contains(&"PRE006"));
    }
}

#[cfg(test)]
mod phase2_conditional_extra_tests {
    use super::*;
    use crate::preprocessor::config::PreprocessConfig;

    fn run(text: &str) -> PreparedSource {
        let unit = AnalysisUnitId::new(crate::source::RunId::new(915), 0);
        let mut manager = SourceManager::new(unit);
        let root = manager.load_root_text("root.lsl", text).unwrap();
        crate::preprocessor::preprocess(&manager, root, PreprocessConfig::full()).unwrap()
    }

    #[test]
    fn multiple_elif_branches_select_exactly_one() {
        let source = "#if 0\na\n#elif 0\nb\n#elif 1\nc\n#elif 1\nd\n#else\ne\n#endif\n";
        let prepared = run(source);
        assert!(prepared.text().contains('c'));
        for absent in ["a", "b", "d", "e"] {
            assert!(!prepared.text().contains(absent), "{}", prepared.text());
        }
        assert!(prepared.preprocess_diagnostics().is_empty());
    }

    #[test]
    fn undef_in_inactive_block_does_not_change_table() {
        let source = "#define X 1\n#if 0\n#undef X\n#endif\nX\n";
        let prepared = run(source);
        assert!(prepared.text().contains('1'));
    }

    #[test]
    fn exact_crlf_and_lone_cr_output() {
        let prepared = run("#define X 1\r\nX\rX\r\n");
        assert_eq!(prepared.text(), "\r\n1\r1\r\n");
        let prepared = run("#define X 1\rX\r");
        assert_eq!(prepared.text(), "\r1\r");
    }
}

#[cfg(test)]
mod phase2_directive_comment_tests {
    use super::*;
    use crate::preprocessor::config::PreprocessConfig;

    fn run(text: &str) -> PreparedSource {
        let unit = AnalysisUnitId::new(crate::source::RunId::new(916), 0);
        let mut manager = SourceManager::new(unit);
        let root = manager.load_root_text("root.lsl", text).unwrap();
        crate::preprocessor::preprocess(&manager, root, PreprocessConfig::full()).unwrap()
    }

    #[test]
    fn multiline_comment_inside_define_is_whitespace() {
        let source = "#define X 1 /* multi\nline */\n#define Y /* multi\nline */ 2\nX Y\n";
        let prepared = run(source);
        assert_eq!(prepared.text(), "\n\n1 2\n");
        assert!(prepared.preprocess_diagnostics().is_empty());
    }

    #[test]
    fn directive_inside_define_comment_is_not_processed() {
        let source = "#define X 1 /*\n#define Y 2\n*/\nY\n";
        let prepared = run(source);
        assert_eq!(prepared.text(), "\nY\n");
        assert!(prepared.preprocess_diagnostics().is_empty());
    }
}

#[cfg(test)]
mod phase2_diagnostic_span_tests {
    use super::*;
    use crate::preprocessor::config::PreprocessConfig;

    fn run(text: &str) -> PreparedSource {
        let unit = AnalysisUnitId::new(crate::source::RunId::new(917), 0);
        let mut manager = SourceManager::new(unit);
        let root = manager.load_root_text("root.lsl", text).unwrap();
        crate::preprocessor::preprocess(&manager, root, PreprocessConfig::full()).unwrap()
    }

    #[test]
    fn empty_if_points_at_directive_end_and_related_opening() {
        let prepared = run("  #if\n#endif\n");
        let diagnostic = prepared
            .preprocess_diagnostics()
            .iter()
            .find(|item| item.code == "PRE030")
            .unwrap();
        assert_eq!(diagnostic.primary.start(), diagnostic.primary.end());
        assert_eq!(diagnostic.primary.start(), 5);
        assert_eq!(diagnostic.related.len(), 1);
        assert_eq!(diagnostic.related[0].span.start(), 2);
    }

    #[test]
    fn unknown_directive_primary_is_the_directive_without_newline() {
        let prepared = run("  #bogus\n");
        let diagnostic = &prepared.preprocess_diagnostics()[0];
        let file = prepared.map().file(diagnostic.primary.source()).unwrap();
        assert_eq!(
            &file.text()[diagnostic.primary.start()..diagnostic.primary.end()],
            "#bogus"
        );
    }
}

#[cfg(test)]
mod phase2_rendered_diagnostic_tests {
    use super::*;
    use crate::diagnostic::resolve::RenderedDiagnostic;
    use crate::preprocessor::config::PreprocessConfig;

    #[test]
    fn preprocess_diagnostics_render_through_the_matching_map() {
        let unit = AnalysisUnitId::new(crate::source::RunId::new(918), 0);
        let mut manager = SourceManager::new(unit);
        let root = manager.load_root_text("root.lsl", "#if 1\nx\n").unwrap();
        let prepared =
            crate::preprocessor::preprocess(&manager, root, PreprocessConfig::full()).unwrap();
        let diagnostic = prepared.preprocess_diagnostics()[0].clone();
        let rendered = RenderedDiagnostic::from_preprocess(diagnostic, &prepared).unwrap();
        assert_eq!(rendered.diagnostic().span.start, 0);
        assert_eq!(rendered.diagnostic().span.end, 5);
        assert_eq!(rendered.related().len(), 1);
    }
}

#[cfg(test)]
mod phase2_review_regression_tests {
    use super::*;
    use crate::preprocessor::config::PreprocessConfig;

    fn run(text: &str) -> PreparedSource {
        let unit = AnalysisUnitId::new(crate::source::RunId::new(919), 0);
        let mut manager = SourceManager::new(unit);
        let root = manager.load_root_text("root.lsl", text).unwrap();
        crate::preprocessor::preprocess(&manager, root, PreprocessConfig::full()).unwrap()
    }

    fn codes(prepared: &PreparedSource) -> Vec<&'static str> {
        prepared
            .preprocess_diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code)
            .collect()
    }

    #[test]
    fn line_splicing_prevents_directive_after_continuation_lf() {
        let source = "integer x = 1 + \\\n#define X 2\ninteger y = X;\n";
        let prepared = run(source);
        assert_eq!(
            prepared.text(),
            "integer x = 1 + #define X 2\ninteger y = X;\n"
        );
        assert!(prepared.preprocess_diagnostics().is_empty());
        assert!(prepared
            .map()
            .resolve_span(PreparedSpan::new(
                prepared.unit(),
                0,
                prepared.prepared_len()
            ))
            .is_ok());
        assert_eq!(prepared.map().eof_anchor().start(), source.len());
    }

    #[test]
    fn line_splicing_prevents_directive_after_continuation_crlf() {
        let source = "integer x = 1 + \\\r\n#define X 2\r\ninteger y = X;\r\n";
        let prepared = run(source);
        assert_eq!(
            prepared.text(),
            "integer x = 1 + #define X 2\r\ninteger y = X;\r\n"
        );
        assert!(prepared.preprocess_diagnostics().is_empty());
    }

    #[test]
    fn line_splicing_prevents_directive_after_continuation_lone_cr() {
        let source = "integer x = 1 + \\\r#define X 2\rinteger y = X;\r";
        let prepared = run(source);
        assert_eq!(
            prepared.text(),
            "integer x = 1 + #define X 2\rinteger y = X;\r"
        );
        assert!(prepared.preprocess_diagnostics().is_empty());
    }

    #[test]
    fn leading_empty_line_does_not_break_continuation_state() {
        let prepared = run("\n#define X 1\nX\n");
        assert!(prepared.text().contains('1'));
        let prepared = run("\r\n#define X 1\r\nX\r\n");
        assert!(prepared.text().contains('1'));
        let prepared = run("\r#define X 1\rX\r");
        assert!(prepared.text().contains('1'));
    }

    #[test]
    fn function_macro_invocation_spans_physical_lines() {
        let source = "#define ADD(a,b) ((a)+(b))\ninteger x = ADD(\n  1,\n  2\n);\n";
        let prepared = run(source);
        assert_eq!(prepared.text(), "\ninteger x = ((1)+(2));\n");
        assert!(prepared.preprocess_diagnostics().is_empty());
        assert!(prepared
            .map()
            .resolve_span(PreparedSpan::new(
                prepared.unit(),
                0,
                prepared.prepared_len()
            ))
            .is_ok());
    }

    #[test]
    fn function_macro_invocation_spans_crlf_lines() {
        let source = "#define ADD(a,b) ((a)+(b))\r\ninteger x = ADD(\r\n  1,\r\n  2\r\n);\r\n";
        let prepared = run(source);
        assert_eq!(prepared.text(), "\r\ninteger x = ((1)+(2));\r\n");
        assert!(prepared.preprocess_diagnostics().is_empty());
    }

    #[test]
    fn else_rejects_extra_tokens_and_recovers() {
        let prepared = run("#if 0\n#else junk\nyes\n#endif\n");
        assert_eq!(codes(&prepared), vec!["PRE002"]);
        assert!(prepared.text().contains("yes"));
        assert!(!prepared.text().contains("junk"));
    }

    #[test]
    fn endif_rejects_extra_tokens_and_recovers() {
        let prepared = run("#if 1\nyes\n#endif junk\n");
        assert_eq!(codes(&prepared), vec!["PRE002"]);
        assert!(prepared.text().contains("yes"));
    }

    #[test]
    fn else_and_endif_allow_whitespace_and_comments() {
        let prepared = run("#if 0\n#else /* c */\nyes\n#endif // c\n");
        assert!(prepared.preprocess_diagnostics().is_empty());
        assert!(prepared.text().contains("yes"));
    }

    #[test]
    fn continued_line_comment_does_not_open_a_block_comment() {
        let prepared = run("// comment \\
/* fake opener\n#define X 7\nX\n");
        assert!(prepared.text().contains("// comment /* fake opener\n"));
        assert!(prepared.text().contains("7\n"));
        assert!(prepared.preprocess_diagnostics().is_empty());
    }

    #[test]
    fn continued_line_comment_splicing_supports_crlf_and_lone_cr() {
        let slash = char::from(92);
        let crlf: String = [char::from(13), char::from(10)].into_iter().collect();
        let cr = char::from(13).to_string();
        let cases = [
            (
                format!("// comment {slash}{crlf}/* fake opener{crlf}#define X 11{crlf}X{crlf}"),
                format!("11{crlf}"),
            ),
            (
                format!("// comment {slash}{cr}/* fake opener{cr}#define X 12{cr}X{cr}"),
                format!("12{cr}"),
            ),
        ];
        for (source, expected) in cases {
            let prepared = run(&source);
            assert!(prepared.text().contains(&expected), "{source:?}");
            assert!(prepared.preprocess_diagnostics().is_empty(), "{source:?}");
        }
    }

    #[test]
    fn continued_line_comment_ignores_quotes_for_scanner_state() {
        let prepared = run("// comment \\
\" fake string opener\n#define X 8\nX\n");
        assert!(prepared
            .text()
            .contains("// comment \" fake string opener\n"));
        assert!(prepared.text().contains("8\n"));
        assert!(prepared.preprocess_diagnostics().is_empty());
    }

    #[test]
    fn continued_line_comment_handles_multiple_splices() {
        let prepared = run("// comment \\
/* still comment \\
\" still comment \\
end\n#define X 9\nX\n");
        assert!(prepared
            .text()
            .contains("// comment /* still comment \" still comment end\n"));
        assert!(prepared.text().contains("9\n"));
        assert!(prepared.preprocess_diagnostics().is_empty());
    }

    #[test]
    fn block_comment_state_still_survives_physical_lines() {
        let prepared = run("/* real block\nstill block\n*/ #define X 10\nX\n");
        assert!(prepared.text().contains("/* real block\nstill block\n*/"));
        assert!(prepared.text().contains("10\n"));
        assert!(prepared.preprocess_diagnostics().is_empty());
    }

    #[test]
    fn negative_shift_operands_select_else_and_diagnose() {
        for operator in ["<<", ">>"] {
            let source = format!("#if -1 {operator} 1\nyes\n#else\nno\n#endif\n");
            let prepared = run(&source);
            assert!(prepared.text().contains("no"), "operator {operator}");
            assert!(!prepared.text().contains("yes"), "operator {operator}");
            assert_eq!(codes(&prepared), vec!["PRE028"], "operator {operator}");
        }
    }

    #[test]
    fn non_negative_shift_controls_remain_valid() {
        for (source, expected) in [
            ("#if 0 << 1\nyes\n#else\nno\n#endif\n", false),
            ("#if 1 << 1\nyes\n#else\nno\n#endif\n", true),
            ("#if 0 >> 1\nyes\n#else\nno\n#endif\n", false),
            ("#if 1 >> 1\nyes\n#else\nno\n#endif\n", false),
        ] {
            let prepared = run(source);
            assert_eq!(prepared.text().contains("yes"), expected, "{source}");
            assert_eq!(prepared.text().contains("no"), !expected, "{source}");
            assert!(prepared.preprocess_diagnostics().is_empty(), "{source}");
        }
    }
}

#[cfg(test)]
mod phase2_multiline_provenance_tests {
    use super::*;
    use crate::preprocessor::config::PreprocessConfig;
    use crate::source::map::PreparedSpan;
    use crate::source::SourceSpan;

    fn run(text: &str) -> PreparedSource {
        let unit = AnalysisUnitId::new(crate::source::RunId::new(920), 0);
        let mut manager = SourceManager::new(unit);
        let root = manager.load_root_text("root.lsl", text).unwrap();
        crate::preprocessor::preprocess(&manager, root, PreprocessConfig::full()).unwrap()
    }

    fn text_of(prepared: &PreparedSource, span: SourceSpan) -> &str {
        &prepared.map().file(span.source()).unwrap().text()[span.start()..span.end()]
    }

    #[test]
    fn multiline_argument_keeps_original_span_and_invocation() {
        let prepared = run("#define ID(x) x\nID(\n  42\n)\n");
        assert_eq!(prepared.text(), "\n42\n");
        let start = prepared.text().find("42").unwrap();
        let resolved = prepared
            .map()
            .resolve_span(PreparedSpan::new(prepared.unit(), start, start + 2))
            .unwrap();
        assert_eq!(text_of(&prepared, resolved.primary()), "42");
        assert!(resolved
            .related()
            .iter()
            .any(|span| text_of(&prepared, *span).contains("ID(")));
        assert!(resolved
            .related()
            .iter()
            .any(|span| text_of(&prepared, *span).contains("#define ID")));
    }
}
