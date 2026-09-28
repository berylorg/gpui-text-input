use super::*;

fn styled_config(runs: usize, capacity: usize) -> RangeTextInputConfig {
    let mut config = config("hello", 5, 32);
    let run = TextRun {
        len: 0,
        font: font(".SystemUIFont"),
        color: black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let mut backing = Vec::with_capacity(capacity);
    backing.resize(runs, run.clone());
    config.style = StreamingGeometryStyle::new(
        run,
        StreamingOversizePresentation::new(
            SharedString::new_static(""),
            backing,
            px(12.),
            px(16.),
            px(12.),
            None,
        ),
    );
    config
}

#[gpui::test]
fn adoption_charges_independent_configuration_runs(cx: &mut TestAppContext) {
    let window = cx.add_empty_window();
    let restoration = seed("hello", 5, 0);
    let mut empty_support = None;
    let mut empty_retained_support = None;
    for (runs, capacity) in [(0, 0), (0, 32), (1, 1), (1, 32), (3, 32)] {
        for refusal in 0..3 {
            let (environment, cleanup, mut session, candidate) = window.update(|window, _| {
                let (environment, cleanup) =
                    make_environment(91, styled_config(runs, capacity), window.text_system());
                let mut session =
                    RangePrepublicationSession::new(restoration, environment.clone()).unwrap();
                let (candidate, _, _) =
                    drive(&mut session, "hello", window.text_system(), &cleanup);
                (environment, cleanup, session, candidate)
            });
            let retained = candidate.retained_charge();
            let origin = candidate.test_origin_session_charge();
            let peak = candidate.adoption_peak();
            let support = RangeSurfaceCharge {
                bytes: peak.bytes - retained.bytes - origin.bytes,
                items: peak.items - retained.items - origin.items,
            };
            let base = *empty_support.get_or_insert(support);
            let expected = RangeSurfaceCharge {
                bytes: retained.bytes
                    + origin.bytes
                    + base.bytes
                    + runs * std::mem::size_of::<TextRun>(),
                items: retained.items + origin.items + base.items + runs,
            };
            assert_eq!(peak, expected, "runs={runs}, capacity={capacity}");
            let current = RangePrepublicationCurrent {
                binding: restoration.binding,
                history: restoration.history,
                available_capacity: RangeSurfaceCharge {
                    bytes: expected.bytes - usize::from(refusal == 1),
                    items: expected.items - usize::from(refusal == 2),
                },
            };
            let observed = Rc::new(Cell::new(None));
            let result = observed.clone();
            let input = window.update(|window, cx| {
                cx.new(|cx| {
                    match RangeTextInput::new_with_prepublication(
                        &environment,
                        candidate,
                        current,
                        window,
                        cx,
                    ) {
                        Ok(input) => input,
                        Err(error) => {
                            result.set(Some(error));
                            RangeTextInput::new(config("hello", 5, 32), window, cx).unwrap()
                        }
                    }
                })
            });
            assert_eq!(
                observed.get(),
                (refusal != 0).then_some(RangePrepublicationAdoptionError::CapacityMismatch)
            );
            if refusal == 0 {
                let diagnostics = window.update(|_, cx| input.read(cx).realization_diagnostics());
                let retained_support = RangeSurfaceCharge {
                    bytes: diagnostics.current.owned_bytes
                        - diagnostics.current.geometry_bytes
                        - diagnostics.adopted_custody_bytes,
                    items: diagnostics.current.owned_items
                        - diagnostics.current.geometry_items
                        - diagnostics.adopted_custody_items,
                };
                let base = *empty_retained_support.get_or_insert(retained_support);
                assert_eq!(
                    retained_support,
                    RangeSurfaceCharge {
                        bytes: base.bytes + runs * std::mem::size_of::<TextRun>(),
                        items: base.items + runs,
                    },
                    "retained runs={runs}, capacity={capacity}"
                );
            }
            session.cancel();
            drop(session);
            drop(input);
            window.update(|_, _| {});
            window.run_until_parked();
            let effects = drain_cleanup(&cleanup);
            assert_eq!(
                effects.iter().any(|effect| matches!(
                    effect,
                    RangePrepublicationCleanupEffect::ReleaseCandidate { .. }
                )),
                refusal != 0
            );
            assert_eq!(cleanup.ownership().active, 0);
            assert!(drain_cleanup(&cleanup).is_empty());
        }
    }
}
