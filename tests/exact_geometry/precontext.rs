use gpui_text_input::{PageDemandEnvelope, PageDirection, PageRequest};

use super::*;

pub(super) fn bounded_owner(source: &str, max_page_bytes: u64) -> ExactGeometryOwner {
    bounded_owner_with_cap(source, max_page_bytes, 512 * 1024)
}

fn bounded_owner_with_cap(
    source: &str,
    max_page_bytes: u64,
    retained_bytes: usize,
) -> ExactGeometryOwner {
    ExactGeometryOwner::new(
        binding(source, 1),
        layout(8, 24.),
        style(),
        ExactGeometryLimits::new(max_page_bytes, 16, retained_bytes, 16 * 1024).unwrap(),
    )
    .unwrap()
}

fn response(source: &str, id: u64, request: PageRequest) -> RangePage {
    response_with_forward_cap(source, id, request, usize::MAX)
}

fn response_with_forward_cap(
    source: &str,
    id: u64,
    request: PageRequest,
    forward_cap: usize,
) -> RangePage {
    response_with_atoms(source, id, request, forward_cap, &[])
}

pub(super) fn response_with_atoms(
    source: &str,
    id: u64,
    request: PageRequest,
    forward_cap: usize,
    atoms: &[(AtomId, ByteRange)],
) -> RangePage {
    let key = request.key();
    let PageDemandEnvelope::Adjacent {
        anchor,
        direction,
        max_payload_bytes,
    } = key.demand()
    else {
        panic!("geometry only issues adjacent page demands")
    };
    let (start, end) = match direction {
        PageDirection::Forward => {
            let start = anchor.get() as usize;
            let mut end = start
                .saturating_add((max_payload_bytes as usize).min(forward_cap))
                .min(source.len());
            while end > start && !source.is_char_boundary(end) {
                end -= 1;
            }
            (start, end)
        }
        PageDirection::Backward => {
            let end = anchor.get() as usize;
            let mut start = end.saturating_sub(max_payload_bytes as usize);
            while start < end && !source.is_char_boundary(start) {
                start += 1;
            }
            (start, end)
        }
    };
    let page_range = ByteRange::from_u64(start as u64, end as u64).unwrap();
    let atom_facts = atoms
        .iter()
        .filter_map(|(id, global)| {
            global.intersection(page_range).map(|fragment| {
                AtomFact::new(*id, *global, fragment, format!("opaque-{}", id.get()))
            })
        })
        .collect();
    RangePage::new(
        PageId::new(id),
        key,
        page_range,
        source[start..end].to_owned(),
        atom_facts,
        if start == 0 {
            PageEdgeFact::DocumentBoundary
        } else {
            PageEdgeFact::Continues
        },
        if end == source.len() {
            PageEdgeFact::DocumentBoundary
        } else {
            PageEdgeFact::Continues
        },
        end == source.len(),
    )
    .unwrap()
}

pub(super) fn demand(request: PageRequest) -> (ByteOffset, PageDirection) {
    let PageDemandEnvelope::Adjacent {
        anchor, direction, ..
    } = request.key().demand()
    else {
        panic!("geometry only issues adjacent page demands")
    };
    (anchor, direction)
}

fn reach_context(
    owner: &mut ExactGeometryOwner,
    source: &str,
    job: GeometryJobKey,
    text_system: &WindowTextSystem,
) -> (PageRequest, RangePage) {
    reach_context_with_forward_cap(owner, source, job, text_system, usize::MAX)
}

fn reach_context_with_forward_cap(
    owner: &mut ExactGeometryOwner,
    source: &str,
    job: GeometryJobKey,
    text_system: &WindowTextSystem,
    forward_cap: usize,
) -> (PageRequest, RangePage) {
    for id in 1..256 {
        let request = owner.request_page(job, PageRequestId::new(id)).unwrap();
        let page = response_with_forward_cap(source, id, request, forward_cap);
        if demand(request).1 == PageDirection::Backward {
            return (request, page);
        }
        assert_eq!(
            owner
                .admit_page(job, &page, text_system)
                .unwrap()
                .progress(),
            ExactGeometryProgress::Scanning
        );
    }
    panic!("fixture never requested pre-context")
}

#[gpui::test]
fn pending_context_cancel_failure_and_late_pages_release_exact_owned_state(
    cx: &mut TestAppContext,
) {
    with_text_system(cx, |text_system| {
        let source = format!("{}TARGET{}", "😀".repeat(24), "é".repeat(24));

        let mut cancelled = bounded_owner(&source, 32);
        let base = cancelled.counts();
        let cancelled_job = start_index(&mut cancelled, 1);
        let (cancelled_request, late_page) =
            reach_context(&mut cancelled, &source, cancelled_job, text_system);
        let active = cancelled.counts();
        let release = cancelled.cancel(cancelled_job).unwrap();
        assert_eq!(release.jobs, vec![cancelled_job]);
        assert_eq!(release.pages, vec![cancelled_request.key()]);
        assert_eq!(
            release.counts.total_bytes(),
            active.total_bytes() - base.total_bytes()
        );
        assert_eq!(
            release.counts.total_items(),
            active.total_items() - base.total_items()
        );
        assert_eq!(cancelled.counts(), base);
        let late = cancelled
            .admit_page(cancelled_job, &late_page, text_system)
            .unwrap_err();
        assert_eq!(
            late.error(),
            &ExactGeometryError::ObsoleteJob(cancelled_job)
        );
        assert_eq!(late.release(), &ExactGeometryRelease::default());

        let mut failed = bounded_owner(&source, 32);
        let failed_base = failed.counts();
        let failed_job = start_index(&mut failed, 1);
        let (failed_request, _) = reach_context(&mut failed, &source, failed_job, text_system);
        let failed_active = failed.counts();
        let release = failed.fail_page(failed_job, failed_request.key()).unwrap();
        assert_eq!(release.jobs, vec![failed_job]);
        assert_eq!(release.pages, vec![failed_request.key()]);
        assert_eq!(
            release.counts.total_bytes(),
            failed_active.total_bytes() - failed_base.total_bytes()
        );
        assert_eq!(failed.counts(), failed_base);
    });
}

#[gpui::test]
fn borrowed_context_page_peak_accepts_exact_byte_cap_and_rejects_one_under(
    cx: &mut TestAppContext,
) {
    with_text_system(cx, |text_system| {
        let source = format!("{}{}TARGET", "a".repeat(1024), "😀".repeat(24));
        let exercise = |cap| {
            let mut owner = ExactGeometryOwner::new(
                binding(&source, 1),
                layout(8, 24.),
                style(),
                ExactGeometryLimits::new(1024, 2, cap, 16 * 1024).unwrap(),
            )
            .unwrap();
            let job = start_index(&mut owner, 1);
            let (request, page) =
                reach_context_with_forward_cap(&mut owner, &source, job, text_system, 8);
            let resident = owner.counts().total_bytes();
            let result = owner.admit_page(job, &page, text_system);
            (owner, job, request, page, resident, result)
        };

        let (_, _, _, _, resident, accepted) = exercise(512 * 1024);
        let required = accepted.unwrap().admission_required_bytes();
        assert!(required > resident);

        let (_, _, _, _, _, exact) = exercise(required);
        assert_eq!(exact.unwrap().admission_required_bytes(), required);

        let (rejected, job, request, _, _, under) = exercise(required - 1);
        let failure = under.unwrap_err();
        assert_eq!(failure.error(), &ExactGeometryError::CapacityExceeded);
        assert_eq!(
            failure.stage(),
            gpui_text_input::ExactGeometryFailureStage::PageCoexistence
        );
        assert_eq!(failure.admission_required_bytes(), required);
        assert_eq!(failure.release().jobs, vec![job]);
        assert_eq!(failure.release().pages, vec![request.key()]);
        assert!(failure.release().counts.active_job_bytes > 0);
        assert!(rejected.index().is_none());
    });
}

#[gpui::test]
fn context_envelope_rejects_wrong_edge_and_nonprogress_before_owner_admission(
    cx: &mut TestAppContext,
) {
    with_text_system(cx, |text_system| {
        let source = format!("{}TARGET{}", "😀".repeat(24), "é".repeat(24));
        let mut owner = bounded_owner(&source, 32);
        let job = start_index(&mut owner, 1);
        let (request, valid) = reach_context(&mut owner, &source, job, text_system);
        let (anchor, direction) = demand(request);
        assert_eq!(direction, PageDirection::Backward);

        let empty = ByteRange::from_u64(anchor.get(), anchor.get()).unwrap();
        let error = RangePage::new(
            PageId::new(90),
            request.key(),
            empty,
            String::new(),
            vec![],
            PageEdgeFact::Continues,
            PageEdgeFact::Continues,
            false,
        )
        .unwrap_err();
        assert_eq!(
            error,
            gpui_text_input::RangeContractError::NonProgressingPage { anchor, direction }
        );

        let wrong_end = anchor.get() - 4;
        let wrong_start = wrong_end - 4;
        let wrong = ByteRange::from_u64(wrong_start, wrong_end).unwrap();
        let error = RangePage::new(
            PageId::new(91),
            request.key(),
            wrong,
            source[wrong_start as usize..wrong_end as usize].to_owned(),
            vec![],
            PageEdgeFact::Continues,
            PageEdgeFact::Continues,
            false,
        )
        .unwrap_err();
        assert_eq!(
            error,
            gpui_text_input::RangeContractError::ReturnedRangeOutsideEnvelope {
                demand: request.key().demand(),
                returned: wrong,
            }
        );

        let admitted = owner.admit_page(job, &valid, text_system).unwrap();
        assert_eq!(admitted.progress(), ExactGeometryProgress::Scanning);
        assert_eq!(admitted.release().pages, vec![request.key()]);
    });
}

#[gpui::test]
fn split_multibyte_pages_request_bounded_context_then_exact_forward_replay(
    cx: &mut TestAppContext,
) {
    with_text_system(cx, |text_system| {
        let source = format!("{}TARGET{}", "😀".repeat(24), "é".repeat(24));
        let mut owner = bounded_owner(&source, 32);
        let job = start_index(&mut owner, 1);
        let mut request_id = 1;
        let mut prior_forward: Option<RangePage> = None;
        let mut context_count = 0;

        loop {
            let request = owner
                .request_page(job, PageRequestId::new(request_id))
                .unwrap();
            request_id += 1;
            let (anchor, direction) = demand(request);
            let page = response(&source, request_id, request);
            request_id += 1;

            if direction == PageDirection::Backward {
                context_count += 1;
                let prior = prior_forward
                    .as_ref()
                    .expect("context follows a forward page");
                assert_eq!(anchor, prior.range().start());
                let stale = owner.admit_page(job, prior, text_system).unwrap_err();
                assert_eq!(stale.error(), &ExactGeometryError::WrongPage(prior.key()));
                assert_eq!(stale.release(), &ExactGeometryRelease::default());

                let stationary_counts = owner.counts();
                let stationary_estimate = owner.estimate();
                let admission = owner.admit_page(job, &page, text_system).unwrap();
                assert_eq!(admission.progress(), ExactGeometryProgress::Scanning);
                assert_eq!(admission.release().pages, vec![page.key()]);
                assert_eq!(owner.estimate(), stationary_estimate);
                let mut after = owner.counts();
                after.pending_page_bytes = stationary_counts.pending_page_bytes;
                after.pending_page_items = stationary_counts.pending_page_items;
                assert_eq!(after, stationary_counts);

                let replay = owner
                    .request_page(job, PageRequestId::new(request_id))
                    .unwrap();
                request_id += 1;
                assert_eq!(
                    demand(replay),
                    (prior.range().start(), PageDirection::Forward)
                );
                let replay_page = response(&source, request_id, replay);
                request_id += 1;
                let admission = owner.admit_page(job, &replay_page, text_system).unwrap();
                assert_eq!(admission.release().pages, vec![replay_page.key()]);
                prior_forward = Some(replay_page);
                if admission.progress() == ExactGeometryProgress::IndexComplete {
                    break;
                }
                continue;
            }

            let admission = owner.admit_page(job, &page, text_system).unwrap();
            assert_eq!(admission.release().pages, vec![page.key()]);
            prior_forward = Some(page);
            if admission.progress() == ExactGeometryProgress::IndexComplete {
                break;
            }
        }

        assert!(context_count >= 2);
        let canonical = scan_index(text_system, &source, &[source.len()], 8, 16, 512 * 1024, 16);
        assert_eq!(
            owner.index().unwrap().aggregate(),
            canonical.index().unwrap().aggregate()
        );
        let facts = |index: &gpui_text_input::ExactGeometryIndex| {
            index
                .checkpoints()
                .iter()
                .map(|checkpoint| {
                    (
                        checkpoint.source(),
                        checkpoint.cursor_offset(),
                        checkpoint.logical_line(),
                        checkpoint.segment(),
                        checkpoint.is_terminal(),
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            facts(owner.index().unwrap()),
            facts(canonical.index().unwrap())
        );
    });
}
