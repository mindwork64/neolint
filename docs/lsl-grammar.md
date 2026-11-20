# LSL grammar and parser inventory

## A. Scope and authority

This document describes the LSL grammar that neolint currently recognizes, the parser behavior implemented in `src/parser.rs`, and the regression tests that exercise it. It is a checked-in engineering inventory, not a republication of an official compiler grammar.

Evidence categories are marked explicitly:

- **[Official]** — the LSL Wiki, Second Life reference, or official Linden documentation.
- **[Compiler grammar]** — the extracted viewer grammar at pinned revision `62a75b11f084be6aa94cab4a5a57b39909c4dfa9` of [Sei-Lisa/LSL-compiler](https://github.com/Sei-Lisa/LSL-compiler/blob/62a75b11f084be6aa94cab4a5a57b39909c4dfa9/indra.y). This is evidence about that compiler snapshot, not proof of every modern server version.
- **[Tested]** — behavior covered by a named neolint test.
- **[Limitation]** — a known gap or deliberately permissive implementation detail.
- **[Unverified]** — no reliable conclusion from the available evidence.

The parser is syntax-only. It does not build a full AST. Semantic rules SEM001–SEM019 are not grammar rules and are documented separately in `docs/language-support.md`, `docs/state-change-contexts.md`, and `docs/vector-literal-contexts.md`.

## B. Notation

The notation below is EBNF-like, not the exact grammar used by the official compiler:

```text
{ X }       zero or more repetitions
[ X ]       optional
X | Y       alternative
"text"      terminal token
lowercase   grammar production
```

The pinned viewer grammar uses Bison/Yacc syntax. It is an important cross-check, but neolint's parser is hand-written and token-derived.

## C. Program structure

[Official][Compiler grammar][Tested]

```text
program          := { global-declaration } state-definition { state-definition }
state-definition := "default" block
                  | "state" identifier block
```

Rules:

- A script with no states is rejected with `PAR013`; the default state is mandatory.
- `default` must be the first state and may appear only once (`PAR013`, `PAR014`).
- Named states may follow `default`; multiple named states are allowed.
- Global declarations and user-defined functions must precede state definitions (`PAR009`).
- State bodies contain event handlers, not ordinary statements directly.
- A state change is a statement: `state name;` or `state default;`.

The pinned grammar is `lscript_program : globals states | states`; `states` begins with `default`. [Compiler grammar]

Relevant tests:

- `src/parser.rs::tests::accepts_default_state_with_multiple_events`
- `src/parser.rs::tests::accepts_default_followed_by_multiple_named_states`
- `src/parser.rs::tests::rejects_empty_source_without_default_state`
- `src/parser.rs::tests::rejects_global_variables_declared_after_states`
- `src/parser.rs::tests::rejects_duplicate_default_states`
- `tests/grammar_inventory.rs::mixed_program_inventory_positive`

## D. Types and declarations

### Value types

[Official][Tested]

The seven LSL value types are:

| Type | Role |
|---|---|
| `integer` | signed integer value |
| `float` | floating-point value |
| `string` | text |
| `key` | UUID-like key value |
| `list` | heterogeneous list |
| `vector` | three-component value |
| `rotation` | four-component value |

`void` is not a source-language value type. A no-return function omits its return type. Spellings such as `void f()` and `f(void)` are rejected.

[Tested] `src/parser.rs::tests::rejects_explicit_void_return_type`, `src/parser.rs::tests::rejects_void_as_a_parameter_list_marker`.

### Global and local declarations

```text
declaration := type identifier [ "=" initializer ] { "," identifier [ "=" initializer ] } ";"
```

- [Official] Global variables are declared at script scope, above state declarations.
- [Tested] neolint accepts global and local declarations, initializers, and all seven types.
- [Limitation] The pinned compiler grammar describes one declarator per declaration; neolint accepts comma-separated declarators such as `integer a, b;`. The official Variables page does not demonstrate the comma form. This is a permissive implementation extension, not a confirmed modern LSL rule.
- [Unverified] neolint accepts arbitrary expressions in global initializers; the pinned grammar describes `simple_assignable`, which is narrower. Constant-expression restrictions are not enforced by the parser.
- [Unverified] The parser accepts a declaration as the single body of a control statement; the pinned compiler marks declarations as disallowed for such bodies. Compiler/runtime behavior is not established here.

Tests:

- `tests/grammar_inventory.rs::declared_types_and_declarations_inventory_positive`
- `src/parser.rs::tests::accepts_lists_casts_loops_and_conditionals`
- `src/parser.rs::tests::rejects_variable_declarations_in_for_initializers`

### Functions and parameters

```text
function := [ type ] identifier "(" [ parameter { "," parameter } ] ")" block
parameter := type identifier
```

- [Official] A function that returns no value omits the return type; it does not use `void`.
- [Tested] Typed and untyped functions, parameters, and nested calls are accepted.
- [Tested] A later function after a state definition is rejected with `PAR009`.

Tests:

- `tests/grammar_inventory.rs::mixed_program_inventory_positive`
- `src/parser.rs::tests::accepts_user_functions_without_return_type`
- `src/parser.rs::tests::accepts_user_functions_and_named_states`
- `src/parser.rs::tests::rejects_user_functions_declared_after_states`

### Event handlers

```text
event-handler := identifier "(" [ parameter { "," parameter } ] ")" block
```

- [Official] Event handlers live directly inside a state body.
- [Tested] The parser accepts event handlers; SEM003 checks known event signatures against canonical metadata.
- [Compilation boundary] Unknown extension event names remain syntactically valid and are conservatively ignored by SEM012 where applicable.

Tests:

- `src/parser.rs::tests::accepts_default_state_with_multiple_events`
- `tests/lsl_samples.rs::representative_multistate_lsl_sample_passes_all_current_checks`
- `tests/state_change_contexts.rs::event_handlers_accept_named_and_default_state_changes`

## E. Statements

```text
statement := block
           | ";"
           | declaration
           | expression ";"
           | "if" "(" expression ")" statement [ "else" statement ]
           | "for" "(" for-initializer ";" [ expression ] ";" for-increment ")" statement
           | "while" "(" expression ")" statement
           | "do" statement "while" "(" expression ")" ";"
           | "return" [ expression ] ";"
           | "jump" identifier ";"
           | "@" identifier ";"
           | "state" ( identifier | "default" ) ";"
```

| Construct | neolint support | Tests / notes |
|---|---|---|
| Compound block | Yes | `tests/grammar_inventory.rs::statements_inventory_positive` |
| Empty statement `;` | Yes | `src/parser.rs::tests::accepts_empty_statements`; **[Compiler grammar]** `LLScriptNOOP` |
| Local declaration | Yes | `tests/grammar_inventory.rs::declared_types_and_declarations_inventory_positive` |
| Expression statement | Yes | `tests/grammar_inventory.rs::expressions_inventory_positive` |
| Assignment / compound assignment | Yes, restricted operators | `src/parser.rs::tests::accepts_variable_assignment_targets_and_chained_assignments` |
| `if` / `else` | Yes | `tests/grammar_inventory.rs::statements_inventory_positive` |
| `for` | Yes; initializer/increment may be comma-separated expression lists | [Compiler grammar] `forexpressionlist`; `src/parser.rs::tests::accepts_for_expression_lists` |
| `while` | Yes | `tests/grammar_inventory.rs::statements_inventory_positive` |
| `do-while` | Yes | `tests/grammar_inventory.rs::statements_inventory_positive` |
| `return` with/without value | Yes | `tests/grammar_inventory.rs::statements_inventory_positive` |
| `jump` / label | Yes syntactically; label rules are semantic | `tests/grammar_inventory.rs::statements_inventory_positive` |
| State change | Yes | `src/parser.rs::tests::accepts_state_change_to_default`; context caveat in `docs/state-change-contexts.md` |

### `for` restrictions

- [Tested] A typed variable declaration in the initializer is rejected with `PAR008`; declare the loop variable before the loop.
- [Compiler grammar][Tested] Initializer and increment accept comma-separated expression lists. The official For page describes the three clauses and optional empty clauses but does not demonstrate the comma-list form; modern compiler behavior is therefore not independently confirmed.
- [Tested] Empty clauses and empty bodies are accepted.

Tests:

- `src/parser.rs::tests::rejects_variable_declarations_in_for_initializers`
- `src/parser.rs::tests::accepts_for_expression_lists`
- `src/parser.rs::tests::accepts_empty_statements`

### `return`

- [Tested] `return;` and `return expression;` are both syntactically accepted.
- [Semantic boundary] Return-value type checking belongs to SEM008, not the parser.

### `jump` and labels

- [Tested] `jump label;` and `@label;` are accepted.
- [Semantic boundary] Duplicate labels, undefined targets, and jumps into inaccessible nested blocks are checked by SEM009/SEM010, not by the parser.

### State changes

- [Tested] `state running;` and `state default;` are accepted.
- [Unverified] The compiler context restriction for state changes inside user-defined functions remains unresolved; see `docs/state-change-contexts.md`.

## F. Expressions

```text
expression := assignment-expression
assignment-expression := unary-expression
                       | lvalue assignment-operator expression
unary-expression := primary
                  | unary-operator unary-expression
                  | ("++" | "--") lvalue
primary := identifier
         | literal
         | "(" expression ")"
         | call
         | cast
         | list-literal
         | vector-or-rotation-literal
         | primary "." identifier
         | primary ("++" | "--")
call := identifier "(" [ expression { "," expression } ] ")"
cast := "(" type ")" unary-expression
```

### Precedence and associativity

[Tested][Compiler grammar]

| Precedence (low to high) | Operators | Associativity |
|---|---|---|
| 1 | `=`, `+=`, `-=`, `*=`, `/=`, `%=` | right |
| 3 | `\|\|`, `&&` | left grouping; equal precedence |
| 5 | `\|` | left |
| 6 | `^` | left |
| 7 | `&` | left |
| 8 | `==`, `!=` | left |
| 9 | `<`, `<=`, `>`, `>=` | left |
| 10 | `<<`, `>>` | left |
| 11 | `+` | left |
| 12 | `-` | left |
| 13 | `*`, `/`, `%` | left |

The missing numeric levels are intentional implementation gaps, not missing LSL precedence levels.

Important distinctions:

- `<` and `>` are both relational operators and vector/rotation literal delimiters. The parser resolves them by context; see `docs/vector-literal-contexts.md`.
- `&&` and `||` have equal precedence and are left-grouped by neolint. The official operator page separately states that evaluation order is right-to-left and both operands are evaluated; precedence/grouping and runtime evaluation order are different claims.
- Subtraction binds more tightly than addition in LSL. This is locked by `src/parser.rs::tests::logical_and_or_operators_share_lsl_precedence` and `src/parser.rs::tests::subtraction_has_higher_precedence_than_addition`.

### Assignment restrictions

- [Tested] `=`, `+=`, `-=`, `*=`, `/=`, and `%=` are accepted.
- [Tested] Bitwise/shift compound assignments (`&=`, `|=`, `^=`, `<<=`, `>>=`) are rejected.
- [Tested] Assignment `LHS` must be a variable or vector/rotation component (`PAR010`).
- [Tested] Prefix/postfix `++`/`--` require a variable operand (`PAR011`).
- [Tested] Chained assignments such as `a = b = 1` are accepted.

### Literals and special expressions

- [Tested] Integer and float literals, strings, list literals, vector/rotation literals, explicit casts, calls, and component access.
- [Tested] All seven types can be used in declarations.
- [Tested] Component access is only valid on a variable identifier; literal and call-result component access is rejected (`PAR012`).
- [Official] `(vector)`/`(rotation)` casts are followed by expressions; the parser's scanner distinguishes a cast from a parenthesized comparison.
- [Unverified] The exact modern compiler treatment of every invalid type pair is a semantic question, not a grammar question.

Tests:

- `tests/grammar_inventory.rs::expressions_inventory_positive`
- `src/parser.rs::tests::accepts_lists_casts_loops_and_conditionals`
- `src/parser.rs::tests::accepts_component_access_on_vector_and_rotation_variables`
- `src/parser.rs::tests::rejects_component_access_on_literals_and_function_results`
- `tests/vector_literal_contexts.rs::relational_angle_brackets_remain_conservative`
- `src/lexer.rs::tests::recognizes_integer_float_and_exponent_literals`

## G. Literals and lexical structure

[Tested]

- Integers are decimal digit sequences; floats include `.5`, `1.`, and exponent forms such as `1e-3`.
- Strings are double-quoted; escaped quotes are retained as string content by the lexer.
- List literals use `[ ... ]`.
- Vector/rotation literals use `< ... >` with 3 or 4 components.
- Line comments use `//`; block comments use `/* ... */`.
- Keywords are recognized case-sensitively.
- Source spans preserve byte offsets and one-based line/column; CRLF is one logical newline.
- [Limitation] Identifier scanning currently accepts ASCII letters, digits, and underscore, beginning with a letter or underscore. Unicode outside strings reports `LEX000`. The official Variables page says names start with a letter or underscore and are case-sensitive, but does not establish a Unicode requirement for modern LSL.
- [Official][Limitation] The canonical built-in metadata is not a grammar source.

Tests:

- `src/lexer.rs::tests::recognizes_lsl_keywords_and_identifiers`
- `src/lexer.rs::tests::recognizes_integer_float_and_exponent_literals`
- `src/lexer.rs::tests::skips_line_and_block_comments`
- `src/lexer.rs::tests::string_literal_keeps_escaped_quotes`
- `src/lexer.rs::tests::handles_crlf_and_unicode_in_comments`
- `src/lexer.rs::tests::unexpected_unicode_outside_string_is_reported_without_panic`

## H. Parser restrictions and diagnostics

Actual syntax codes in `src/parser.rs` are listed below. There is no `PAR002` in the current implementation.

| Code | Condition | Minimal example | Test |
|---|---|---|---|
| `PAR001` | expected global declaration or state definition | `void f() {}` | `src/parser.rs::tests::rejects_explicit_void_return_type` |
| `PAR003` | expected parameter type | `integer f(void) {}` | `src/parser.rs::tests::rejects_void_as_a_parameter_list_marker` |
| `PAR004` | expected event handler inside state | `default { integer value; }` | `tests/grammar_inventory.rs::focused_malformed_statement_diagnostics` |
| `PAR005` | expected expression after operator | `integer x = 1 + ;` | `tests/grammar_inventory.rs::focused_malformed_statement_diagnostics` |
| `PAR006` | expected expression | `if () {}` | `tests/grammar_inventory.rs::focused_malformed_statement_diagnostics` |
| `PAR007` | required token missing or unexpected | `integer x = 1 }` | `src/parser.rs::tests::reports_missing_semicolon` |
| `PAR008` | typed declaration in `for` initializer | `for (integer i = 0; ...)` | `src/parser.rs::tests::rejects_variable_declarations_in_for_initializers` |
| `PAR009` | global declaration/function after states | `default {} integer x;` | `src/parser.rs::tests::rejects_global_variables_declared_after_states` |
| `PAR010` | invalid assignment target | `1 = 2;` | `src/parser.rs::tests::rejects_assignment_to_non_variable_expressions` |
| `PAR011` | invalid increment/decrement target | `1++;` | `src/parser.rs::tests::rejects_increment_and_decrement_of_non_variables` |
| `PAR012` | component access on non-variable | `<1,2,3>.x` | `src/parser.rs::tests::rejects_component_access_on_literals_and_function_results` |
| `PAR013` | missing/out-of-order default state | empty file; `state x {}` | `src/parser.rs::tests::rejects_empty_source_without_default_state`, `src/parser.rs::tests::rejects_named_state_before_default` |
| `PAR014` | duplicate default state | `default {} default {}` | `src/parser.rs::tests::rejects_duplicate_default_states` |

Malformed delimiter and missing-token cases without a more specific code are expected to produce a syntax diagnostic; `tests/grammar_inventory.rs::focused_malformed_statement_diagnostics` checks representative cases.

## I. Semantic boundary

The parser validates syntax and produces `DiagnosticKind::Syntax`. It does not construct a full AST. It deliberately does not enforce:

- symbol uniqueness or definition ordering inside expressions;
- variable/function/event name resolution;
- argument counts or types for user functions/builtins (SEM002–SEM006);
- return-type compatibility (SEM008);
- jump-label scope (SEM009/SEM010);
- control-flow return reachability (SEM011, token-derived CFG);
- unknown extension symbol policy (SEM012);
- deprecation and type/cast rules (SEM013–SEM019).

See:

- [language-support.md](language-support.md) for the current semantic rule list;
- [state-change-contexts.md](state-change-contexts.md) for the unresolved function/state-change context;
- [vector-literal-contexts.md](vector-literal-contexts.md) for `<`/`>` literal contexts.

## J. Unsupported and uncertain language behavior

- [Official] `?:` is not LSL syntax; the parser rejects it.
- [Official] `void` is not a source type; both `void f()` and `f(void)` are rejected.
- [Tested] Typed declarations in `for` initializers are rejected (`PAR008`).
- [Limitation] Comma-separated declarations are accepted by neolint but not described by the pinned viewer grammar; modern compiler support is unverified.
- [Unverified] Global initializer constant-expression restrictions are not enforced.
- [Unverified] Declarations as single control-statement bodies are accepted by neolint, while the extracted compiler marks declarations disallowed for those bodies.
- [Compiler grammar][Tested] Comma-separated `for` initializer/increment lists are accepted; the official For page does not demonstrate them.
- [Limitation] ASCII-only identifiers are an implementation limit, not established as an LSL requirement.
- [Unverified] State changes inside user-defined functions and their historical `if` workaround; see the dedicated state document.
- [Limitation] Duplicate state names, duplicate event handlers, and undefined state-change targets are not diagnosed.
- [Unverified] Modern-server behavior for every compiler-grammar edge case; the extracted viewer compiler is not a modern-compatibility guarantee.

## K. Grammar-to-test matrix

| Grammar construct | Positive test | Negative test | Support / evidence |
|---|---|---|---|
| Global variable(s) | `tests/grammar_inventory.rs::declared_types_and_declarations_inventory_positive` | `src/parser.rs::tests::rejects_global_variables_declared_after_states` | Tested; comma form is a limitation |
| Typed function | `tests/grammar_inventory.rs::mixed_program_inventory_positive` | `src/parser.rs::tests::rejects_user_functions_declared_after_states` | Tested |
| No-return function | `src/parser.rs::tests::accepts_user_functions_without_return_type` | `src/parser.rs::tests::rejects_explicit_void_return_type` | Official + tested |
| Parameters | `tests/grammar_inventory.rs::mixed_program_inventory_positive` | `src/parser.rs::tests::rejects_void_as_a_parameter_list_marker` | Tested |
| Default state | `src/parser.rs::tests::accepts_single_default_state_without_named_states` | `src/parser.rs::tests::rejects_empty_source_without_default_state` | Official + tested |
| Named/multiple states | `src/parser.rs::tests::accepts_default_followed_by_multiple_named_states` | `src/parser.rs::tests::rejects_named_state_before_default` | Official + tested |
| Event handler | `src/parser.rs::tests::accepts_default_state_with_multiple_events` | — | Official + tested |
| Full mixed script | `tests/lsl_samples.rs::representative_multistate_lsl_sample_passes_all_current_checks` | — | Tested |
| Block / empty statement | `tests/grammar_inventory.rs::statements_inventory_positive` | — | Compiler grammar + tested |
| Local declaration | `tests/grammar_inventory.rs::declared_types_and_declarations_inventory_positive` | `src/parser.rs::tests::rejects_variable_declarations_in_for_initializers` | Tested |
| Expression statement | `tests/grammar_inventory.rs::expressions_inventory_positive` | — | Tested |
| Assignment / compound | `src/parser.rs::tests::accepts_variable_assignment_targets_and_chained_assignments` | `src/parser.rs::tests::rejects_assignment_to_non_variable_expressions` | Tested |
| `if` / `else` | `tests/grammar_inventory.rs::statements_inventory_positive` | — | Tested |
| `for` | `tests/grammar_inventory.rs::statements_inventory_positive`; `src/parser.rs::tests::accepts_for_expression_lists` | `src/parser.rs::tests::rejects_variable_declarations_in_for_initializers` | Tested; comma list from compiler grammar |
| `while` / `do-while` | `tests/grammar_inventory.rs::statements_inventory_positive` | — | Tested |
| `return` | `tests/grammar_inventory.rs::statements_inventory_positive` | `src/parser.rs::tests::rejects_explicit_void_return_type` (type syntax) | Tested; value rules semantic |
| `jump` / label | `tests/grammar_inventory.rs::statements_inventory_positive` | — | Syntax tested; scope semantically checked |
| State change | `src/parser.rs::tests::accepts_state_change_to_default`; `tests/state_change_contexts.rs::event_handlers_accept_named_and_default_state_changes` | `src/parser.rs::tests::rejects_state_change_without_target` | Tested; context caveat |
| Literals | `tests/grammar_inventory.rs::expressions_inventory_positive`; `src/lexer.rs::tests::recognizes_integer_float_and_exponent_literals` | `src/lexer.rs::tests::unexpected_unicode_outside_string_is_reported_without_panic` | Tested |
| Calls / casts | `tests/grammar_inventory.rs::expressions_inventory_positive`; `tests/vector_literal_contexts.rs::explicit_cast_contexts_cover_vector_literals` | `src/parser.rs::tests::rejects_component_access_on_literals_and_function_results` | Tested |
| Unary/binary/logical/comparison | `tests/grammar_inventory.rs::expressions_inventory_positive`; `src/parser.rs::tests::logical_and_or_operators_share_lsl_precedence` | — | Tested |
| Increment/decrement | `src/parser.rs::tests::accepts_increment_and_decrement_of_variables` | `src/parser.rs::tests::rejects_increment_and_decrement_of_non_variables` | Tested |
| List/vector/rotation literals | `tests/grammar_inventory.rs::expressions_inventory_positive`; `tests/vector_literal_contexts.rs::declaration_initializers_cover_literal_expressions_and_commas` | `src/parser.rs::tests::rejects_component_access_on_literals_and_function_results` | Tested |
| Component access | `src/parser.rs::tests::accepts_component_access_on_vector_and_rotation_variables` | `src/parser.rs::tests::rejects_component_access_on_literals_and_function_results` | Tested |
| `<`/`>` ambiguity | `tests/vector_literal_contexts.rs::relational_angle_brackets_remain_conservative` | `src/parser.rs::tests::rejects_component_access_on_literals_and_function_results` | Tested; inference conservative |
| Malformed delimiters | — | `tests/grammar_inventory.rs::focused_malformed_statement_diagnostics` | Tested |

## References

- [LSL Portal](https://wiki.secondlife.com/wiki/LSL_Portal)
- [Second Life LSL Reference](https://create.secondlife.com/script/lsl-reference/)
- [LSL Wiki — State](https://wiki.secondlife.com/wiki/State)
- [LSL Wiki — For](https://wiki.secondlife.com/wiki/For)
- [LSL Wiki — LSL Variables](https://wiki.secondlife.com/wiki/LSL_Variables)
- [LSL Wiki — User-Defined Functions](https://wiki.secondlife.com/wiki/Category:LSL_User-Defined_Functions)
- [LSL Operators](https://wiki.secondlife.com/wiki/LSL_Operators)
- [Linden Lab LSL Guide](https://static-secondlife-com.s3.amazonaws.com/downloads/textures/guides/LSLGuide.pdf)
- [Sei-Lisa/LSL-compiler pinned grammar](https://github.com/Sei-Lisa/LSL-compiler/blob/62a75b11f084be6aa94cab4a5a57b39909c4dfa9/indra.y)
