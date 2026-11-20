# LSL state-change context restrictions

## Syntax

A state-change statement has the form `state target;`, where `target` is a named state or `default`. It is a statement, so the parser accepts it in any statement block. The parser is deliberately syntax-only and does not enforce compiler context restrictions.

## Confirmed reference facts

- The LSL Wiki `State` page documents state changes in event handlers, including `state two;` and `state default;`, and explicitly documents `state default;` in the `StateKeyword` example set.
- The same `State` caveat states: user-defined (global) functions cannot change a script's state; the compiler reports `ERROR: Global functions can't change state`.
- The caveat describes a historical workaround: put the state change in a simple `if` without an `else` clause, for example `if (condition) state Two;`. It contrasts this with an `else` form and says Rand Linden previously stated the workaround is no longer valid, so it may be removed.
- The same caveat says the workaround is a bugfeature and that care should be taken in using it. Its current compiler status is therefore not reliable.
- No official statement was found that `state default;` has different context restrictions from a named state target. The documented restriction applies to changing the script's state generally.
- The references do not document the behavior of a state change inside `while`, `for`, or nested blocks within a user-defined function. Those cases are unverified.

## Current neolint behavior

- `state running;` and `state default;` are accepted in event handlers.
- Both forms are accepted syntactically in user-defined function bodies, nested blocks, `if`/`else`, `while`, and `for`.
- No semantic diagnostic is currently emitted for a state change in a user-defined function.
- A missing state target (`state ;`) remains a syntax error (`PAR007`).

## Decision

No new semantic diagnostic is introduced yet. The direct function restriction is documented, but the historical `if`-without-`else` workaround has contradictory/obsolete status and no official compiler is available for verification. A rule that flags every function state change could reject a still-working workaround; a rule limited to direct statements would be incomplete. neolint therefore keeps the construct syntax-only and documents the uncertainty until compiler behavior can be verified.

## Sources

- [LSL Wiki — State](https://wiki.secondlife.com/wiki/State)
- [LSL Wiki — StateKeyword](http://lslwiki.digiworldz.com/lslwiki/wakka.php?wakka=StateKeyword)
