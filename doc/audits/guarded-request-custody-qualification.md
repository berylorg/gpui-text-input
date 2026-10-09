# Guarded Request Custody Qualification

`take_request_if` inspects the original queue front before dispatch admission. Rejection leaves
the request, ordering and tracking untouched; an empty queue does not invoke the predicate.
Acceptance delegates to ordinary `take_request`. Response, resident protection and quiescence
requirements remain unchanged. This qualifies the widget primitive; Beryl owns qualification of
its failed-source capture caller.

## Evidence

The three-source snapshot `31900FB78D193BC1D91154E15BCF6ABECEC926C718235E83BDFD1740CAD6F3C6`
covers `src/range_widget.rs`, `tests/range_widget.rs` and
`tests/range_widget/guarded_request_custody.rs`. Parent Beryl retains raw receipts under
`.tmp/thread-lineage-evidence`.

- Focused custody tests: 4/4 passed, run `470d2b4a-7c32-49c5-928f-a6c087de4817`.
- Full range-widget suite: 128/128 passed, run `5179b632-ec41-4853-aae3-bb3a8ae00b9e`.
- Locked offline all-target check with `test-support` passed. Fork verification uses its configured
  local owned dependencies; Beryl separately validates the published Git dependency revision.
- All three source hashes and scoped Rust formatting matched the reviewed snapshot; scoped Git
  whitespace checks passed.
- Independent semantic review found no blocking issue in the frozen source, contracts or receipts.
  It covered unchanged rejected custody, accepted identity/tracking, mutation cancellation and
  genuine resident-page release, under the effective production and shared-resource rigor.

The guarded dequeue primitive is accepted. Publication supplies the immutable revision for the
parent's remaining pin, model and native-capture acceptance gates.
