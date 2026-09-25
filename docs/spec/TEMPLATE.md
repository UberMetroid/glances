# Spec: <module>

Behavioral contract for rewriting `<old path>`, derived from ground truth
only (T0): observed v0.10.72 behavior, live OS interfaces, installed manuals.
No upstream source was consulted. Code is written to this spec — never to
the old implementation.

## Role
One paragraph: what this module does and why it exists.

## Inputs (ground truth)
- OS interfaces read (exact paths/calls) and what each field means.
- Data received from other modules (OWNED dependencies only).

## Computation (defined fresh)
- The algorithm in plain words, with edge cases and error behavior.
- Frozen constants (wire compat) vs chosen constants (our design).

## Outputs (wire contract)
- Exact shapes emitted (JSON keys, CLI text, exit codes) with a capture reference.

## Captures
- `corpus/<module>/...`: input/output vectors from the v0.10.72 oracle.
- Volatile fields (timestamps, PIDs, live readings) listed explicitly.

## Independent oracle
- How the new test derives expected values from ground truth in-test.
