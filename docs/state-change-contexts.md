# LSL state-change context restrictions

## Syntax

A state-change statement has the form `state target;`, where `target` is a named state or `default`. It is a statement, so the parser accepts it in any statement block. The parser is deliberately syntax-only and does not enforce compiler context restrictions.

## Official main-page statements

- The LSL Wiki `State` page documents state changes in event handlers, including `state two;` and `state default;`. The `StateKeyword` example also shows `state default;`.
- The same `State` caveat states: user-defined (global) functions cannot change a script's state; the compiler reports `ERROR: Global functions can't change state`.
- The caveat describes a historical workaround: put the state change in a simple `if` without an `else` clause, for example `if (condition) state Two;`. It contrasts this with an `else` form and says Rand Linden previously stated that the workaround is no longer valid. It calls the workaround a bugfeature that may be removed.
- No official statement was found that `state default;` has different context restrictions from a named state target. The documented restriction is about changing the script's state generally.
- The references do not document the behavior of a state change inside `while`, `for`, or nested blocks within a user-defined function. Those cases remain unverified.

## Historical user observations

These are Talk:State posts by users, not Linden Lab specification.

### February 2013: workaround on a live server

Innula Zenovka reported that a state change inside an `if` block in a user-defined function worked on Second Life Server `13.02.08.270166`:

```lsl
ChangeStates()
{
    if (1 == 1)
    {
        state other;
    }
}
```

The post says the script compiled and ran without a compiler or runtime error. Omei Qunhua confirmed it worked on the same server. The same thread records that removing the `if` block still produced the documented `ERROR: Global functions can't change state`; ObviousAltIsObvious Resident treated the working form as a compiler bug, not supported behavior. Omei also reported testing a variant for 100,000 loops without observing a memory leak in either LSO or Mono; this is a user observation, not a specification claim.

### August 2014: Mono and LSO behavior

Sei Lisa reported that Mono "handles it gracefully": the observed effect was like a `return` statement, but the state change was not effective until the current event finalized. The same post reported an LSO failure when a non-`void` function was expected to return a value and the caller used that value: the script crashed with a `Bounds Check Error`. If the function returned no value, or the caller did not use the value, LSO reportedly worked.

The test sketch in the thread used a `string` function with `if (1) state another;` followed by `return "something";`. Under Mono the caller observed the default string value (length `0`); under LSO the caller crashed. Sei Lisa clarified that Mono and LSO otherwise behaved the same, and that the LSO crash occurred only when the function's return value was used.

Pedro Oval's follow-up described the behavior as a return from the function, not a `longjmp`: the state change is flagged, the function returns immediately, and the change is applied when the event finishes. The post also showed generated CIL that calls `ChangeState`, pushes a default value of the function's return type, and executes `ret`. These are historical user observations and generated-code interpretation, not an official statement of modern server behavior.

### Named state versus `default`

The 2013 example used both `state other;` and `state default;` in user functions. No historical post or official text was found that gives `default` a different context restriction from a named state. This is absence of evidence, not proof of equivalence.

## Compiler-source evidence

[Sei-Lisa/LSL-compiler](https://github.com/Sei-Lisa/LSL-compiler) is a standalone distillation of the viewer's LSL compiler, producing LSO assembler and Mono CIL. It is not the current Second Life server compiler and cannot execute scripts on a simulator.

Source inspection shows:

- `lscript_error.cpp` contains `LSERROR_STATE_CHANGE_IN_GLOBAL` with the text `Global functions can't change state`.
- `LLScriptStateChange::recurse` emits the global-function error during `LSCP_PRUNE` when the prune type is `LSPRUNE_GLOBAL_VOIDS` or `LSPRUNE_GLOBAL_NON_VOIDS`.
- `LLScriptIf::recurse` handles `LSCP_PRUNE` by setting `prunearg = FALSE` and not recursing into its body, which explains why a state change inside an `if` was not rejected by that pass. `LLScriptIfElse::recurse` does recurse into both branches, so the workaround is specifically tied to the non-`else` form.
- For CIL, `LLScriptStateChange::recurse` emits `ChangeState(string)`, pushes an initial/default value of the current function's return type, and emits `ret`.
- For LSO, the same node emits `STATE` after `print_exit_pops`; the compiler does not execute the resulting program.

The compiler was not built locally: its README requires `kwdb`, Python 2, `flex`, and `bison`. Even if built, it could confirm compile-time acceptance/rejection and generated code, but not modern server runtime behavior or the LSO crash.

## Current neolint behavior

- `state running;` and `state default;` are accepted in event handlers.
- Both forms are accepted syntactically in user-defined function bodies, nested blocks, `if`/`else`, `while`, and `for`.
- SEM022 validates state-change targets in known canonical event handlers. A target must be a declared named state or `default`.
- SEM022 does not validate state changes in user-defined functions or unknown extension event handlers.
- A missing state target (`state ;`) remains a syntax error (`PAR007`).

## Decision

SEM022 covers the unambiguous event-handler target check: if a known event handler says `state missing;` and no such state exists, that is a compiler-visible undefined name. The target is checked by token span; declarations may appear before or after the event handler.

The state-change-in-function restriction remains unresolved. The historical `if`-without-`else` workaround has contradictory status, and the extracted compiler is not a current server runtime. neolint therefore does not reject function state changes and does not infer whether the workaround works. `state default;` remains valid.

## Sources

- [LSL Wiki — State](https://wiki.secondlife.com/wiki/State)
- [LSL Wiki — Talk:State](https://wiki.secondlife.com/wiki/Talk:State)
- [LSL Wiki — StateKeyword](http://lslwiki.digiworldz.com/lslwiki/wakka.php?wakka=StateKeyword)
- [Sei-Lisa/LSL-compiler](https://github.com/Sei-Lisa/LSL-compiler)
