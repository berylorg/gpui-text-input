use super::precontext::{bounded_owner, demand, response_with_atoms};
use super::*;

fn drive_atom_job(
    owner: &mut ExactGeometryOwner,
    source: &str,
    job: GeometryJobKey,
    text_system: &WindowTextSystem,
    forward_cap: usize,
    atoms: &[(AtomId, ByteRange)],
    first_id: u64,
) -> (ExactGeometryProgress, usize) {
    let mut id = first_id;
    let mut contexts = 0;
    for _ in 0..512 {
        let request = owner.request_page(job, PageRequestId::new(id)).unwrap();
        id += 1;
        if demand(request).1 == gpui_text_input::PageDirection::Backward {
            contexts += 1;
        }
        let page = response_with_atoms(source, id, request, forward_cap, atoms);
        id += 1;
        let admission = owner.admit_page(job, &page, text_system).unwrap();
        if admission.progress() != ExactGeometryProgress::Scanning {
            return (admission.progress(), contexts);
        }
    }
    panic!("atom geometry fixture did not complete")
}

fn reach_atom_context(
    owner: &mut ExactGeometryOwner,
    source: &str,
    job: GeometryJobKey,
    text_system: &WindowTextSystem,
    atoms: &[(AtomId, ByteRange)],
) -> (gpui_text_input::PageRequest, RangePage) {
    for id in 1..256 {
        let request = owner.request_page(job, PageRequestId::new(id)).unwrap();
        let page = response_with_atoms(source, id, request, 4, atoms);
        if demand(request).1 == gpui_text_input::PageDirection::Backward {
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
    panic!("atom fixture never requested pre-context")
}

#[gpui::test]
fn opaque_ri_atom_is_an_independent_context_origin_across_replay_and_target(
    cx: &mut TestAppContext,
) {
    with_text_system(cx, |text_system| {
        let source = "\u{1f1e6}".repeat(13);
        let atom_range = ByteRange::from_u64(0, 4).unwrap();
        let atoms = [(AtomId::new(77), atom_range)];
        let build = |index_cap, target_cap| {
            let mut owner = ExactGeometryOwner::new(
                binding(&source, 1),
                layout(8, 24.),
                style(),
                ExactGeometryLimits::new(64, 16, 512 * 1024, 16 * 1024).unwrap(),
            )
            .unwrap();
            let index = start_index(&mut owner, 1);
            let (progress, index_contexts) = drive_atom_job(
                &mut owner,
                &source,
                index,
                text_system,
                index_cap,
                &atoms,
                1,
            );
            assert_eq!(progress, ExactGeometryProgress::IndexComplete);
            let target = owner
                .request_block_target(
                    GeometryJobId::new(2),
                    BlockTarget::new(px(14.), px(28.), px(14.)),
                )
                .unwrap();
            let (progress, target_contexts) = drive_atom_job(
                &mut owner,
                &source,
                target.key(),
                text_system,
                target_cap,
                &atoms,
                1_000,
            );
            assert_eq!(progress, ExactGeometryProgress::TargetComplete);
            (owner, index_contexts + target_contexts)
        };

        let (canonical, _) = build(usize::MAX, 4);
        let (partitioned, contexts) = build(4, 4);
        assert!(
            contexts >= 2,
            "fixture must recur through context and replay"
        );
        assert_eq!(
            partitioned.index().unwrap().aggregate(),
            canonical.index().unwrap().aggregate()
        );
        let checkpoints = |owner: &ExactGeometryOwner| {
            owner
                .index()
                .unwrap()
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
        assert_eq!(checkpoints(&partitioned), checkpoints(&canonical));
        assert_eq!(
            partitioned.target().unwrap().target_source(),
            canonical.target().unwrap().target_source()
        );
        assert_eq!(
            fragment_facts(partitioned.target().unwrap().fragments()),
            fragment_facts(canonical.target().unwrap().fragments())
        );
    });
}

#[gpui::test]
fn atom_context_suffix_rejects_intersection_and_obsolete_failure_preserves_newer_job(
    cx: &mut TestAppContext,
) {
    with_text_system(cx, |text_system| {
        let source = "\u{1f1e6}".repeat(13);
        let real_atom = (AtomId::new(77), ByteRange::from_u64(0, 4).unwrap());
        let intersecting = (AtomId::new(88), ByteRange::from_u64(4, 8).unwrap());

        let mut terminal = bounded_owner(&source, 64);
        let terminal_job = start_index(&mut terminal, 1);
        let (request, _) = reach_atom_context(
            &mut terminal,
            &source,
            terminal_job,
            text_system,
            &[real_atom],
        );
        let malformed = response_with_atoms(
            &source,
            500,
            request,
            usize::MAX,
            &[real_atom, intersecting],
        );
        let failure = terminal
            .admit_page(terminal_job, &malformed, text_system)
            .unwrap_err();
        assert_eq!(failure.error(), &ExactGeometryError::SourceContract);
        assert_eq!(
            failure.stage(),
            gpui_text_input::ExactGeometryFailureStage::Scan
        );
        assert_eq!(failure.release().jobs, vec![terminal_job]);
        assert_eq!(failure.release().pages, vec![request.key()]);

        let mut owner = bounded_owner(&source, 64);
        let obsolete_job = start_index(&mut owner, 1);
        let (obsolete_request, _) =
            reach_atom_context(&mut owner, &source, obsolete_job, text_system, &[real_atom]);
        let obsolete_malformed = response_with_atoms(
            &source,
            501,
            obsolete_request,
            usize::MAX,
            &[real_atom, intersecting],
        );
        owner.cancel(obsolete_job).unwrap();
        let current_job = start_index(&mut owner, 2);
        let late = owner
            .admit_page(obsolete_job, &obsolete_malformed, text_system)
            .unwrap_err();
        assert_eq!(late.error(), &ExactGeometryError::ObsoleteJob(obsolete_job));
        assert_eq!(late.release(), &ExactGeometryRelease::default());
        let current_request = owner
            .request_page(current_job, PageRequestId::new(100))
            .unwrap();
        let current_page = response_with_atoms(&source, 502, current_request, 4, &[real_atom]);
        assert_eq!(
            owner
                .admit_page(current_job, &current_page, text_system)
                .unwrap()
                .progress(),
            ExactGeometryProgress::Scanning
        );

        let spanning = [(AtomId::new(99), ByteRange::from_u64(0, 8).unwrap())];
        let mut spanning_owner = bounded_owner(&source, 64);
        let spanning_job = start_index(&mut spanning_owner, 1);
        let (progress, contexts) = drive_atom_job(
            &mut spanning_owner,
            &source,
            spanning_job,
            text_system,
            4,
            &spanning,
            1,
        );
        assert_eq!(progress, ExactGeometryProgress::IndexComplete);
        assert!(contexts >= 1);
    });
}
