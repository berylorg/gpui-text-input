# Scope

Implement coherent index retirement during local target replacement under
[the atomic transition contract](design.md#range-backed-atomic-interaction-publication),
coordinated by the Beryl root plan. Dependency publication and application qualification remain
separate acceptance boundaries.

# Phase 97: Retire Superseded Index Custody During Local Target Replacement (finished)

Local target replacement now retires the superseded index through the existing prepared release
set. Deferred targets preserve ongoing indexing; refusal preserves exact custody. Seven new tests
cover empty/nonterminal targets, queued/dispatched text, delayed object responses and quiescence.
All 550 tests and default-feature compilation pass; independent lifecycle review accepted.
The coordinating Beryl plan owns canonical publication and empty acquired/restored qualification.
