# Geometry Object Failure Qualification

Exact geometry-object failure now retires the pending index intent after original key/job
authentication, residency settlement, producer cancellation, release and active-job retirement.
This matches text-page failure. Public `Stale`, foreign rejection, resident publication and
protection rules remain unchanged. A failed producer cannot silently restart from its orphaned
intent; later explicit layout admission can obtain fresh geometry normally.

## Evidence

The three-path snapshot `FDB9C97EC10A3A7EEDE501B64E1D6ADAF86AFF632DCF0EDEBB9FD3E4E03B5A07`
covers `src/range_widget/geometry.rs`, `tests/range_widget.rs` and
`tests/range_widget/geometry_object_failure.rs`. Parent Beryl retains raw receipts under
`.tmp/thread-lineage-evidence`. Qualification uses canonical Git dependencies.

- Focused index/target object-failure cases: 2/2 passed, run
  `d3bffff2-49f8-43d9-b79e-7cb396318043`.
- Full range-widget suite: 130/130 passed in 4.863 seconds, run
  `b335bbfa-8a18-4fb5-958a-08e31187010f`.
- Locked offline all-target checking with `test-support` passed in 12.91 seconds.
- Independent review verified all frozen hashes, exact authentication order, unchanged public
  and foreign outcomes, original custody/full quiescence, successor isolation and later explicit
  layout work. No blocking finding remained.

The private producer cleanup is accepted. Beryl owns unified input/settings dependency pins,
model validation and native capture qualification.
