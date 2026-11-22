//! Integer expression parsing and evaluation for `#if`/`#elif`.
//!
//! The model is fixed signed 64-bit (`i64`) with C-like precedence and
//! short-circuit evaluation. All arithmetic is checked; errors are reported and
//! abort evaluation of the current expression.

use crate::preprocessor::diagnostics::{
    DiagSink, PRE_DIVISION_BY_ZERO, PRE_EXPRESSION_OVERFLOW, PRE_INVALID_IF_EXPRESSION,
    PRE_INVALID_SHIFT, PRE_MALFORMED_LITERAL,
};
use crate::preprocessor::token::{PieceKind, RawSpan, WorkItem};

/// Maximum expression nesting depth.
pub(crate) const MAX_EXPRESSION_DEPTH: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExprTokenKind {
    Number(i64),
    Ident(String),
    LParen,
    RParen,
    Question,
    Colon,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Bang,
    Tilde,
    Shl,
    Shr,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    BitAnd,
    BitXor,
    BitOr,
    AndAnd,
    OrOr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExprToken {
    pub(crate) kind: ExprTokenKind,
    pub(crate) span: RawSpan,
}

/// Tokenize macro-expanded expression items, reporting malformed literals and
/// invalid operators. Invalid literals become zero so parsing can continue.
pub(crate) fn tokenize(items: &[WorkItem], diags: &mut DiagSink) -> Vec<ExprToken> {
    let mut tokens = Vec::new();
    for item in items {
        match item.kind {
            PieceKind::Whitespace | PieceKind::Comment => {}
            PieceKind::Identifier => tokens.push(ExprToken {
                kind: ExprTokenKind::Ident(item.text.clone()),
                span: item.span,
            }),
            PieceKind::Number => {
                let value = match parse_integer_literal(&item.text) {
                    Ok(value) => value,
                    Err(LiteralError::Malformed) => {
                        diags.push(
                            PRE_MALFORMED_LITERAL,
                            format!("invalid integer literal '{}'", item.text),
                            item.span,
                        );
                        0
                    }
                    Err(LiteralError::Overflow) => {
                        diags.push(
                            PRE_EXPRESSION_OVERFLOW,
                            format!("integer literal is outside the i64 range: '{}'", item.text),
                            item.span,
                        );
                        0
                    }
                };
                tokens.push(ExprToken {
                    kind: ExprTokenKind::Number(value),
                    span: item.span,
                });
            }
            PieceKind::String => {
                diags.push(
                    PRE_INVALID_IF_EXPRESSION,
                    "string literals are not valid in preprocessor expressions",
                    item.span,
                );
                tokens.push(ExprToken {
                    kind: ExprTokenKind::Number(0),
                    span: item.span,
                });
            }
            PieceKind::Punct | PieceKind::Other => {
                if let Some(kind) = operator_token(&item.text) {
                    tokens.push(ExprToken {
                        kind,
                        span: item.span,
                    });
                } else {
                    diags.push(
                        PRE_INVALID_IF_EXPRESSION,
                        format!(
                            "unsupported token '{}' in preprocessor expression",
                            item.text
                        ),
                        item.span,
                    );
                }
            }
        }
    }
    tokens
}

fn operator_token(text: &str) -> Option<ExprTokenKind> {
    Some(match text {
        "(" => ExprTokenKind::LParen,
        ")" => ExprTokenKind::RParen,
        "?" => ExprTokenKind::Question,
        ":" => ExprTokenKind::Colon,
        "+" => ExprTokenKind::Plus,
        "-" => ExprTokenKind::Minus,
        "*" => ExprTokenKind::Star,
        "/" => ExprTokenKind::Slash,
        "%" => ExprTokenKind::Percent,
        "!" => ExprTokenKind::Bang,
        "~" => ExprTokenKind::Tilde,
        "<<" => ExprTokenKind::Shl,
        ">>" => ExprTokenKind::Shr,
        "<" => ExprTokenKind::Lt,
        "<=" => ExprTokenKind::Le,
        ">" => ExprTokenKind::Gt,
        ">=" => ExprTokenKind::Ge,
        "==" => ExprTokenKind::Eq,
        "!=" => ExprTokenKind::Ne,
        "&" => ExprTokenKind::BitAnd,
        "^" => ExprTokenKind::BitXor,
        "|" => ExprTokenKind::BitOr,
        "&&" => ExprTokenKind::AndAnd,
        "||" => ExprTokenKind::OrOr,
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiteralError {
    Malformed,
    Overflow,
}

fn parse_integer_literal(text: &str) -> Result<i64, LiteralError> {
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        if hex.is_empty() || !hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return Err(LiteralError::Malformed);
        }
        let value = u64::from_str_radix(hex, 16).map_err(|_| LiteralError::Overflow)?;
        return i64::try_from(value).map_err(|_| LiteralError::Overflow);
    }
    if text.len() > 1 && text.starts_with('0') {
        if !text.chars().all(|ch| ('0'..='7').contains(&ch)) {
            return Err(LiteralError::Malformed);
        }
        let value = u64::from_str_radix(&text[1..], 8).map_err(|_| LiteralError::Overflow)?;
        return i64::try_from(value).map_err(|_| LiteralError::Overflow);
    }
    if text.is_empty() || !text.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(LiteralError::Malformed);
    }
    let value = text.parse::<i64>().map_err(|_| LiteralError::Overflow)?;
    Ok(value)
}

/// Parse and evaluate an expression. `expression_span` is the fallback location
/// for errors at end of input.
pub(crate) fn evaluate(
    tokens: &[ExprToken],
    expression_span: RawSpan,
    diags: &mut DiagSink,
) -> Result<i64, ()> {
    let mut parser = Parser {
        tokens,
        position: 0,
        depth: 0,
        expression_span,
        diags,
    };
    let value = parser.parse_conditional(true)?;
    if let Some(token) = parser.peek() {
        parser.diags.push(
            PRE_INVALID_IF_EXPRESSION,
            "unexpected token after preprocessor expression",
            token.span,
        );
        return Err(());
    }
    Ok(value)
}

struct Parser<'a> {
    tokens: &'a [ExprToken],
    position: usize,
    depth: usize,
    expression_span: RawSpan,
    diags: &'a mut DiagSink,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&ExprToken> {
        self.tokens.get(self.position)
    }

    fn next(&mut self) -> Option<&ExprToken> {
        let token = self.tokens.get(self.position);
        if token.is_some() {
            self.position += 1;
        }
        token
    }

    fn current_span(&self) -> RawSpan {
        self.peek().map_or(self.expression_span, |token| token.span)
    }

    fn enter(&mut self) -> Result<(), ()> {
        self.depth += 1;
        if self.depth > MAX_EXPRESSION_DEPTH {
            self.diags.push(
                PRE_INVALID_IF_EXPRESSION,
                "preprocessor expression nesting is too deep",
                self.current_span(),
            );
            return Err(());
        }
        Ok(())
    }

    fn leave(&mut self) {
        self.depth -= 1;
    }

    fn parse_conditional(&mut self, eval: bool) -> Result<i64, ()> {
        self.enter()?;
        let result = self.parse_conditional_inner(eval);
        self.leave();
        result
    }

    fn parse_conditional_inner(&mut self, eval: bool) -> Result<i64, ()> {
        let condition = self.parse_binary(1, eval)?;
        if !matches!(
            self.peek().map(|token| &token.kind),
            Some(ExprTokenKind::Question)
        ) {
            return Ok(condition);
        }
        self.next();
        let when_true = self.parse_conditional(eval && condition != 0)?;
        if !matches!(
            self.peek().map(|token| &token.kind),
            Some(ExprTokenKind::Colon)
        ) {
            self.diags.push(
                PRE_INVALID_IF_EXPRESSION,
                "expected ':' in conditional expression",
                self.current_span(),
            );
            return Err(());
        }
        self.next();
        let when_false = self.parse_conditional(eval && condition == 0)?;
        if !eval {
            return Ok(0);
        }
        Ok(if condition != 0 {
            when_true
        } else {
            when_false
        })
    }

    fn parse_binary(&mut self, min_precedence: u8, eval: bool) -> Result<i64, ()> {
        let mut left = self.parse_unary(eval)?;
        while let Some((precedence, short_circuit)) =
            self.peek().and_then(|token| binary_precedence(&token.kind))
        {
            if precedence < min_precedence {
                break;
            }
            let operator = self.next().expect("peeked token").clone();
            let right_eval = if !eval {
                false
            } else if let Some(logical) = short_circuit {
                match logical {
                    Logical::And => left != 0,
                    Logical::Or => left == 0,
                }
            } else {
                true
            };
            let right = self.parse_binary(precedence + 1, right_eval)?;
            left = self.apply_binary(&operator, left, right, eval)?;
        }
        Ok(left)
    }

    fn parse_unary(&mut self, eval: bool) -> Result<i64, ()> {
        self.enter()?;
        let result = self.parse_unary_inner(eval);
        self.leave();
        result
    }

    fn parse_unary_inner(&mut self, eval: bool) -> Result<i64, ()> {
        let Some(token) = self.peek().cloned() else {
            self.diags.push(
                PRE_INVALID_IF_EXPRESSION,
                "expected an expression",
                self.expression_span,
            );
            return Err(());
        };
        match token.kind {
            ExprTokenKind::Plus => {
                self.next();
                self.parse_unary(eval)
            }
            ExprTokenKind::Minus => {
                self.next();
                let value = self.parse_unary(eval)?;
                if !eval {
                    return Ok(0);
                }
                value.checked_neg().ok_or_else(|| {
                    self.diags.push(
                        PRE_EXPRESSION_OVERFLOW,
                        "unary minus overflows i64",
                        token.span,
                    );
                })
            }
            ExprTokenKind::Bang => {
                self.next();
                let value = self.parse_unary(eval)?;
                Ok(if eval && value == 0 { 1 } else { 0 })
            }
            ExprTokenKind::Tilde => {
                self.next();
                let value = self.parse_unary(eval)?;
                Ok(if eval { !value } else { 0 })
            }
            _ => self.parse_primary(eval),
        }
    }

    fn parse_primary(&mut self, eval: bool) -> Result<i64, ()> {
        let Some(token) = self.next().cloned() else {
            self.diags.push(
                PRE_INVALID_IF_EXPRESSION,
                "expected an expression",
                self.expression_span,
            );
            return Err(());
        };
        match token.kind {
            ExprTokenKind::Number(value) => Ok(value),
            ExprTokenKind::Ident(_) => Ok(0),
            ExprTokenKind::LParen => {
                let value = self.parse_conditional(eval)?;
                if !matches!(
                    self.peek().map(|token| &token.kind),
                    Some(ExprTokenKind::RParen)
                ) {
                    self.diags.push(
                        PRE_INVALID_IF_EXPRESSION,
                        "expected ')' in preprocessor expression",
                        self.current_span(),
                    );
                    return Err(());
                }
                self.next();
                Ok(value)
            }
            _ => {
                self.diags.push(
                    PRE_INVALID_IF_EXPRESSION,
                    "expected a primary expression",
                    token.span,
                );
                Err(())
            }
        }
    }

    fn apply_binary(
        &mut self,
        operator: &ExprToken,
        left: i64,
        right: i64,
        eval: bool,
    ) -> Result<i64, ()> {
        if !eval {
            return Ok(0);
        }
        let span = operator.span;
        match operator.kind {
            ExprTokenKind::OrOr => Ok(u8::from(left != 0 || right != 0).into()),
            ExprTokenKind::AndAnd => Ok(u8::from(left != 0 && right != 0).into()),
            ExprTokenKind::BitOr => Ok(left | right),
            ExprTokenKind::BitXor => Ok(left ^ right),
            ExprTokenKind::BitAnd => Ok(left & right),
            ExprTokenKind::Eq => Ok(u8::from(left == right).into()),
            ExprTokenKind::Ne => Ok(u8::from(left != right).into()),
            ExprTokenKind::Lt => Ok(u8::from(left < right).into()),
            ExprTokenKind::Le => Ok(u8::from(left <= right).into()),
            ExprTokenKind::Gt => Ok(u8::from(left > right).into()),
            ExprTokenKind::Ge => Ok(u8::from(left >= right).into()),
            ExprTokenKind::Shl | ExprTokenKind::Shr => {
                if !(0..64).contains(&right) {
                    self.diags
                        .push(PRE_INVALID_SHIFT, "shift count must be in 0..63", span);
                    return Err(());
                }
                if matches!(operator.kind, ExprTokenKind::Shl) {
                    let shifted = (left as i128) << right;
                    if shifted < i64::MIN as i128 || shifted > i64::MAX as i128 {
                        self.diags
                            .push(PRE_EXPRESSION_OVERFLOW, "left shift overflows i64", span);
                        return Err(());
                    }
                    Ok(shifted as i64)
                } else {
                    Ok(left >> right)
                }
            }
            ExprTokenKind::Plus => left.checked_add(right).ok_or_else(|| {
                self.diags
                    .push(PRE_EXPRESSION_OVERFLOW, "addition overflows i64", span);
            }),
            ExprTokenKind::Minus => left.checked_sub(right).ok_or_else(|| {
                self.diags
                    .push(PRE_EXPRESSION_OVERFLOW, "subtraction overflows i64", span);
            }),
            ExprTokenKind::Star => left.checked_mul(right).ok_or_else(|| {
                self.diags.push(
                    PRE_EXPRESSION_OVERFLOW,
                    "multiplication overflows i64",
                    span,
                );
            }),
            ExprTokenKind::Slash | ExprTokenKind::Percent => {
                if right == 0 {
                    self.diags.push(
                        PRE_DIVISION_BY_ZERO,
                        "division by zero in preprocessor expression",
                        span,
                    );
                    return Err(());
                }
                if left == i64::MIN && right == -1 {
                    self.diags
                        .push(PRE_EXPRESSION_OVERFLOW, "division overflows i64", span);
                    return Err(());
                }
                if matches!(operator.kind, ExprTokenKind::Slash) {
                    Ok(left / right)
                } else {
                    Ok(left % right)
                }
            }
            _ => {
                self.diags.push(
                    PRE_INVALID_IF_EXPRESSION,
                    "unsupported operator in preprocessor expression",
                    span,
                );
                Err(())
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Logical {
    And,
    Or,
}

fn binary_precedence(kind: &ExprTokenKind) -> Option<(u8, Option<Logical>)> {
    Some(match kind {
        ExprTokenKind::OrOr => (1, Some(Logical::Or)),
        ExprTokenKind::AndAnd => (2, Some(Logical::And)),
        ExprTokenKind::BitOr => (3, None),
        ExprTokenKind::BitXor => (4, None),
        ExprTokenKind::BitAnd => (5, None),
        ExprTokenKind::Eq | ExprTokenKind::Ne => (6, None),
        ExprTokenKind::Lt | ExprTokenKind::Le | ExprTokenKind::Gt | ExprTokenKind::Ge => (7, None),
        ExprTokenKind::Shl | ExprTokenKind::Shr => (8, None),
        ExprTokenKind::Plus | ExprTokenKind::Minus => (9, None),
        ExprTokenKind::Star | ExprTokenKind::Slash | ExprTokenKind::Percent => (10, None),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preprocessor::diagnostics::DiagSink;
    use crate::preprocessor::token::{tokenize_source, WorkItem};
    use crate::source::{AnalysisUnitId, RunId, SourceId};

    fn eval(source: &str) -> (Result<i64, ()>, Vec<&'static str>) {
        let source_id = SourceId::new(AnalysisUnitId::new(RunId::new(77), 0), 0);
        let pieces = tokenize_source(source, source_id, 0, source.len());
        let items: Vec<WorkItem> = pieces.into_iter().map(WorkItem::from_piece).collect();
        let mut diags = DiagSink::new();
        let tokens = tokenize(&items, &mut diags);
        let result = evaluate(
            &tokens,
            RawSpan::new(source_id, 0, source.len()),
            &mut diags,
        );
        let codes = diags.items().iter().map(|item| item.code).collect();
        (result, codes)
    }

    #[test]
    fn table_of_precedence_and_arithmetic() {
        let cases = [
            ("1 + 2 * 3", 7),
            ("(1 + 2) * 3", 9),
            ("1 << 2 + 1", 8),
            ("16 >> 2", 4),
            ("7 / 2", 3),
            ("7 % 2", 1),
            ("1 | 2 & 4", 1),
            ("1 ^ 3", 2),
            ("~0", -1),
            ("!0", 1),
            ("!5", 0),
            ("-5 + 2", -3),
            ("+5", 5),
            ("3 < 4", 1),
            ("3 <= 3", 1),
            ("4 > 3", 1),
            ("4 >= 5", 0),
            ("3 == 3", 1),
            ("3 != 3", 0),
            ("1 && 2", 1),
            ("0 && 2", 0),
            ("0 || 2", 1),
            ("0 || 0", 0),
            ("1 ? 2 : 3", 2),
            ("0 ? 2 : 3", 3),
            ("1 ? 2 : 0 ? 3 : 4", 2),
            ("0 ? 2 : 0 ? 3 : 4", 4),
            ("1 | 2 && 0", 0),
            ("0 && 1 || 1", 1),
            ("-9223372036854775807 - 1", i64::MIN),
        ];
        for (source, expected) in cases {
            let (result, codes) = eval(source);
            assert_eq!(
                result,
                Ok(expected),
                "expression {source:?}, codes {codes:?}"
            );
            assert!(codes.is_empty(), "expression {source:?}, codes {codes:?}");
        }
    }

    #[test]
    fn short_circuit_and_ternary_do_not_evaluate_unselected_branch() {
        for source in [
            "0 && (1 / 0)",
            "1 || (1 / 0)",
            "0 ? (1 / 0) : 7",
            "1 ? 7 : (1 / 0)",
        ] {
            let (result, codes) = eval(source);
            assert!(result.is_ok(), "expression {source:?}, codes {codes:?}");
            assert!(codes.is_empty(), "expression {source:?}, codes {codes:?}");
        }
    }

    #[test]
    fn division_and_remainder_by_zero_are_diagnosed() {
        for source in ["1 / 0", "1 % 0", "(-9223372036854775807 - 1) / -1"] {
            let (result, codes) = eval(source);
            assert!(result.is_err(), "expression {source:?}");
            if source == "1 / 0" || source == "1 % 0" {
                assert!(
                    codes.contains(&PRE_DIVISION_BY_ZERO),
                    "{source:?}: {codes:?}"
                );
            } else {
                assert!(
                    codes.contains(&PRE_EXPRESSION_OVERFLOW),
                    "{source:?}: {codes:?}"
                );
            }
        }
    }

    #[test]
    fn arithmetic_overflow_is_diagnosed() {
        for source in [
            "9223372036854775807 + 1",
            "-9223372036854775807 - 2",
            "3037000500 * 3037000500",
            "-(-9223372036854775807 - 1)",
            "1 << 63",
        ] {
            let (result, codes) = eval(source);
            assert!(result.is_err(), "expression {source:?}");
            assert!(
                codes.contains(&PRE_EXPRESSION_OVERFLOW),
                "{source:?}: {codes:?}"
            );
        }
    }

    #[test]
    fn invalid_shifts_are_diagnosed() {
        for source in ["1 << 64", "1 >> 64", "1 << -1", "1 >> -1"] {
            let (result, codes) = eval(source);
            assert!(result.is_err(), "expression {source:?}");
            assert!(codes.contains(&PRE_INVALID_SHIFT), "{source:?}: {codes:?}");
        }
    }

    #[test]
    fn malformed_literals_and_expressions_are_diagnosed() {
        for source in ["09", "0x", "12abc"] {
            let (_, codes) = eval(source);
            assert!(
                codes.contains(&PRE_MALFORMED_LITERAL),
                "{source:?}: {codes:?}"
            );
        }
        for source in ["(1", "1 +", "1 2", "1 ? 2", "1 ? 2 :", ")"] {
            let (result, codes) = eval(source);
            assert!(result.is_err(), "expression {source:?}");
            assert!(
                codes.contains(&PRE_INVALID_IF_EXPRESSION),
                "{source:?}: {codes:?}"
            );
        }
    }

    #[test]
    fn literals_and_undefined_identifiers() {
        assert_eq!(eval("42").0, Ok(42));
        assert_eq!(eval("052").0, Ok(42));
        assert_eq!(eval("0x2a").0, Ok(42));
        assert_eq!(eval("UNKNOWN").0, Ok(0));
        assert_eq!(eval("0x7fffffffffffffff").0, Ok(i64::MAX));
    }
}
