use super::*;

fn replacement_peak(
    current: RangeRealizationOwnership,
    old: RangeSurfaceCharge,
    incoming: RangeSurfaceCharge,
) -> RangeSurfaceCharge {
    let replacement = RangeSurfaceCharge {
        bytes: current.owned_bytes + incoming.bytes,
        items: current.owned_items + incoming.items,
    };
    let service = RangeSurfaceCharge {
        bytes: current.owned_bytes - old.bytes + 2 * incoming.bytes,
        items: current.owned_items - old.items + 2 * incoming.items,
    };
    RangeSurfaceCharge {
        bytes: replacement.bytes.max(service.bytes),
        items: replacement.items.max(service.items),
    }
}

#[gpui::test]
fn pending_layout_last_wins_accepts_exact_nonoverlapping_peak(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    input.update(cx, |input, cx| {
        input.config.limits.max_realization_work_per_frame = 1;
        input.begin_realization_frame();
        input.spend_realization_credit();

        let mut first_layout = input.config.layout.clone();
        first_layout.input_id = 501;
        input
            .request_layout_intent(first_layout, input.config.style.clone(), cx)
            .unwrap();
        let current = input.current_realization_ownership();
        let old = input.pending_layout_intent.as_ref().unwrap().charge();

        let mut replacement_layout = input.config.layout.clone();
        replacement_layout.input_id = 502;
        let replacement = super::super::realization::PendingLayoutIntent {
            layout: replacement_layout.clone(),
            style: input.config.style.clone(),
        };
        let incoming = replacement.charge();
        let exact = replacement_peak(current, old, incoming);
        input.config.limits.max_surface_bytes = exact.bytes - 1;
        assert!(matches!(
            input.request_layout_intent(replacement_layout.clone(), input.config.style.clone(), cx),
            Err(RangeTextInputError::SurfaceCapacity)
        ));
        assert_eq!(
            input
                .pending_layout_intent
                .as_ref()
                .unwrap()
                .layout
                .input_id,
            501
        );

        input.config.limits.max_surface_bytes = exact.bytes;
        input.config.limits.max_surface_items = exact.items;
        input
            .request_layout_intent(replacement_layout, input.config.style.clone(), cx)
            .unwrap();
        assert_eq!(
            input
                .pending_layout_intent
                .as_ref()
                .unwrap()
                .layout
                .input_id,
            502
        );
        assert!(input.realization_diagnostics().high_water.owned_bytes >= exact.bytes);
    });
}

#[gpui::test]
fn pending_rebind_last_wins_accepts_exact_nonoverlapping_peak(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    input.update(cx, |input, _| {
        let position = input.surface().unwrap().selection().head;
        let (_, text, objects) = admitted_successor_sources(SOURCE, 1, &[position]);
        let proof = crate::range_edit::SourcePositionProof::from_admitted_sources(
            input.config.binding,
            position,
            &text,
            &objects,
        )
        .unwrap();
        let current_binding = input.config.binding;
        let make_intent = |operation| super::super::realization::PendingRebindIntent::Mutation {
            key: crate::MutationKey::new(
                binding().binding(),
                binding().revision(),
                crate::OperationId::new(operation),
            ),
            outcome: crate::MutationOutcome::Rejected,
            binding: current_binding,
            selection: RangeSourceSelection::caret(position),
            positions: crate::MutationPositions::collapsed(position),
            proofs: vec![proof; 3],
            composition: None,
            active_loss_reason: crate::InlineObjectRealizationLossReason::SelectionChanged,
        };
        input
            .retain_pending_rebind_intent(make_intent(501))
            .unwrap();
        let current = input.current_realization_ownership();
        let old = input.pending_rebind_intent.as_ref().unwrap().charge();
        let replacement = make_intent(502);
        let incoming = replacement.charge();
        let exact = replacement_peak(current, old, incoming);

        input.config.limits.max_surface_bytes = exact.bytes - 1;
        assert!(matches!(
            input.retain_pending_rebind_intent(replacement.clone()),
            Err(RangeTextInputError::SurfaceCapacity)
        ));
        assert!(matches!(
            input.pending_rebind_intent.as_ref(),
            Some(super::super::realization::PendingRebindIntent::Mutation { key, .. })
                if key.operation() == crate::OperationId::new(501)
        ));

        input.config.limits.max_surface_bytes = exact.bytes;
        input.config.limits.max_surface_items = exact.items;
        input.retain_pending_rebind_intent(replacement).unwrap();
        assert!(matches!(
            input.pending_rebind_intent.as_ref(),
            Some(super::super::realization::PendingRebindIntent::Mutation { key, .. })
                if key.operation() == crate::OperationId::new(502)
        ));
        assert!(input.realization_diagnostics().high_water.owned_bytes >= exact.bytes);
    });
}
