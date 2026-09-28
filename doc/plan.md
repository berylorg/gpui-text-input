# Scope

Implement the capacity prerequisite for preserved resident adoption from
[the package contract](design.md#range-backed-preserved-resident-adoption), coordinated by
the Beryl root plan. Resident protection, accounting and publication remain separate work.

# Phase 35: Admit Direct Geometry Request Storage Before Allocation (finished)

Text/object requests now check independently clamped caller ceilings before allocation and mutation.
Refusal preserves the job and request ID for retry. Independent resource review accepted the change;
all 202 regression tests passed, including tight, zero and oversized caller ceilings.

# Phase 2: Enforce Capacity Throughout Preparation (pending)

Apply the admitted budget before geometry scan growth and candidate preparation allocations,
including resident-response paths and candidate/ready/transition ownership peaks. Verify tight
byte/item budgets during progress and exact cleanup with independent resource review. Combined
preserved-resident reservation must not rely on the admission API until this boundary is accepted.
Cover prepared target transitions and prepared-publication collections under the host ceiling.
Returned GPUI custody now uses the geometry budget before invocation.
Direct response admission accepts enclosing ceilings; host derivation and peak propagation remain
required before combined resident reservation can use them.
Direct request creation also accepts enclosing ceilings before pending-record allocation.
Deferred-object custody remains charged through inline admission when deriving remaining capacity.
Carry prepared transition peak evidence through enclosing host admission before host use.
Dependency-private shaping scratch is outside this reservation; retain finite layout input limits.
