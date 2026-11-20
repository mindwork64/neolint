# Vector and rotation literal scanning contexts

`neolint` scans tokens rather than a full AST. `starts_vector_literal()` decides whether a `<` token begins an angle-bracket vector/rotation literal or is the relational operator. The decision is conservative: a literal is recognized only when the preceding token requires an operand, when the `<` is the first token in a scanned expression, or immediately after `return`; a `<` after an expression-ending token remains relational.

This matrix records the contexts covered by the integration tests in `tests/vector_literal_contexts.rs`.

| Context | Token before `<` | Positive form | Negative or conservative form |
|---|---|---|---|
| Declaration initializer | `=`, declarator `,` | `vector v = <1, 2, 3>;` | `string s = <1, 2, 3>;` reports SEM015 |
| Assignment RHS | `=` | `vector v; v = <1, 2, 3>;` | `string s; s = <1, 2, 3>;` reports SEM016 |
| Compound assignment RHS | `+=`, `-=`, `*=`, `/=`, `%=` | `vector v; v += <1, 2, 3>;` | `integer i; i += <1, 2, 3>;` reports SEM017 |
| Call argument | `(`, argument `,` | `llSetPos(<1, 2, 3>);` | `llOwnerSay(<1, 2, 3>);` reports SEM004; an extra argument reports SEM002 |
| Return expression | `return` | `vector f() { return <1, 2, 3>; }` | `integer f() { return <1, 2, 3>; }` reports SEM008 |
| Explicit cast operand | `)` of `(vector)` or `(rotation)` | `return (vector)<1, 2, 3>;` | an unsupported cast reports SEM014; an incompatible result reports SEM008/SEM015/SEM016 |
| Equality RHS | `==`, `!=` | `v == <1, 2, 3>` has type `integer` | `string s = <1, 2, 3> == <4, 5, 6>;` reports SEM015 |
| Arithmetic operand | unary/binary `+`, `-`, `*`, `/`, `%` | `<1, 2, 3> + <4, 5, 6>` has type `vector`; `<1, 2, 3> * <4, 5, 6>` has type `float` | an unsupported pair stays unresolved; no speculative diagnostic |
| Grouped/list expression | `(`, `[`, `,` | `llOwnerSay((string)(<1, 2, 3> == <4, 5, 6>));` | `(1) < 2` remains a comparison, not a literal |
| Relational ambiguity | token that can end an expression | `(1) < 2` is a comparison | `<1, 2, 3> < <4, 5, 6>` and `1 > <1, 2, 3>` stay unresolved; no SEM008/SEM015/SEM016 speculation |
| Unknown/extension value | unknown identifier or call | `llExtensionValue()` remains unchecked | no guessed type and no speculative diagnostic |

## Deliberate limitations

- A vector/rotation relational pair is not inferred as a boolean result. The analyzer leaves the expression unresolved instead of guessing LSL or C-like behavior.
- A direct `<` after a token that can end an expression is not treated as a literal. This avoids turning `(left) < right` or `value > limit` into a literal split.
- A closing `>` can end a vector/rotation literal or be a relational operator. The scanner tracks literal depth only after a recognized opening `<`; ambiguous relational forms remain conservative.
- Unknown extension functions and values do not receive inferred types.

References: [LSL Operators](https://wiki.secondlife.com/wiki/LSL_Operators), [LSL Vector](https://wiki.secondlife.com/wiki/Vector), [LSL Rotation](https://wiki.secondlife.com/wiki/Category:LSL_Rotation).
