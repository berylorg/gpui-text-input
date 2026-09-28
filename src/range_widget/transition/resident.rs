use super::super::*;

pub(in crate::range_widget) struct PreparedResidentPublication {
    config: RangeTextInputConfig,
    residency: RangeResidency,
    object_residency: ObjectResidency,
    edits: RangeEditCoordinator,
    clipboard: RangeClipboardCoordinator,
    requests: VecDeque<RangeTextInputRequest>,
    response_custody: VecDeque<response_custody::RangeResponseCustody>,
    dispatched_pages: realization::DispatchedKeys<crate::PageRequestKey>,
    dispatched_object_pages: realization::DispatchedKeys<crate::ObjectRequestKey>,
    dispatched_mutations: realization::DispatchedKeys<crate::MutationKey>,
    scrollbar_owner: ScrollbarOwnerKey,
}

impl PreparedResidentPublication {
    pub(in crate::range_widget) fn prepare(
        input: &RangeTextInput,
        config: RangeTextInputConfig,
    ) -> Result<Self, RangePrepublicationAdoptionError> {
        let capacity_error = RangePrepublicationAdoptionError::CapacityMismatch;
        let mount = input
            .scrollbar
            .owner
            .mount_generation
            .get()
            .checked_add(1)
            .ok_or(RangePrepublicationAdoptionError::PredecessorMismatch)?;
        if input.scrollbar.state.current_owner() != Some(input.scrollbar.owner) {
            return Err(RangePrepublicationAdoptionError::PredecessorMismatch);
        }
        let request_capacity = checked_request_capacity(&config).ok_or(capacity_error)?;
        let response_capacity = config
            .residency_limits
            .max_pending_requests()
            .checked_add(config.object_residency_limits.max_pending_requests())
            .ok_or(capacity_error)?;
        let clipboard = RangeClipboardCoordinator::new_composite(
            config.binding,
            config.presentation_generation,
            config.atom_clipboard_policy,
            config.clipboard_limits,
        )
        .map_err(|_| RangePrepublicationAdoptionError::WidgetConstruction)?;
        Ok(Self {
            residency: RangeResidency::new(config.binding, config.residency_limits),
            object_residency: ObjectResidency::new(
                config.binding,
                config.presentation_generation,
                config.object_residency_limits,
            ),
            edits: RangeEditCoordinator::new(config.binding, config.mutation_limits),
            clipboard,
            requests: VecDeque::with_capacity(request_capacity),
            response_custody: VecDeque::with_capacity(response_capacity),
            dispatched_pages: realization::DispatchedKeys::with_capacity(
                config.residency_limits.max_pending_requests(),
            ),
            dispatched_object_pages: realization::DispatchedKeys::with_capacity(
                config.object_residency_limits.max_pending_requests(),
            ),
            dispatched_mutations: realization::DispatchedKeys::with_capacity(
                RangeTextInput::MAX_QUEUED_MUTATION_REQUESTS,
            ),
            scrollbar_owner: ScrollbarOwnerKey::new(
                input.scrollbar.owner.owner_id,
                ScrollbarMountGeneration::new(mount),
            ),
            config,
        })
    }
}

impl RangeTextInput {
    pub(in crate::range_widget) fn commit_prepared_resident_publication(
        &mut self,
        prepared: PreparedResidentPublication,
        owners: AdoptedPrepublicationOwners,
        custody: prepublication::AdoptedPrepublicationCustody,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut desired = DesiredSurface::origin(
            prepared.config.viewport_extent,
            bounded_realization_extent(
                prepared.config.viewport_extent,
                prepared.config.limits.max_realized_block_extent,
            ),
            prepared.config.overscan,
        );
        desired.source_selection = Some(owners.surface.selection());
        desired.scroll = RangeScrollAnchor {
            source: owners.surface.scroll_source(),
            intra_anchor: owners.surface.scroll_intra_anchor(),
        };
        desired.target_block = owners.surface.scroll_block();
        desired.realization_anchor_block = owners.surface.scroll_block();
        desired.preserve_scroll_anchor = false;
        desired.reveal_caret = false;
        let prior_owner = self.scrollbar.owner;
        let prior_geometry = std::mem::replace(&mut self.geometry, owners.geometry);
        let prior_surface = self.surface.replace(owners.surface);
        let prior_custody = self.adopted_prepublication_custody.replace(custody);
        let prior_residency = std::mem::replace(&mut self.residency, prepared.residency);
        let prior_objects =
            std::mem::replace(&mut self.object_residency, prepared.object_residency);
        self.config = prepared.config;
        self.edits = prepared.edits;
        self.clipboard = prepared.clipboard;
        self.requests = prepared.requests;
        self.response_custody = prepared.response_custody;
        self.dispatched_pages = prepared.dispatched_pages;
        self.dispatched_object_pages = prepared.dispatched_object_pages;
        self.dispatched_mutations = prepared.dispatched_mutations;
        self.desired = desired;
        self.published_restoration = Some(owners.seed);
        self.history_frontier = owners
            .history
            .unwrap_or_else(|| RangeHistoryFrontier::unavailable(owners.seed.binding));
        self.next_id = self.next_id.max(owners.next_id);
        self.adopted_positions = None;
        self.mutation_positions = None;
        self.admitted_edit_proofs = Vec::new();
        self.mutation_composition = None;
        self.pending_page_aliases = Vec::new();
        self.clipboard_cut_proofs = None;
        self.pending_clipboard_page = None;
        self.pointer_anchor = None;
        self.active_object = None;
        self.last_surface_admission = Some(owners.surface_charge);
        self.last_realization_step = RangeRealizationStep {
            spent: 0,
            remaining: self.config.limits.max_realization_work_per_frame,
            progressed: false,
            reached_external_boundary: false,
        };
        self.scrollbar.owner = prepared.scrollbar_owner;
        self.scrollbar.model.set(None);
        self.invalidate_resident_protection();
        assert!(self.scrollbar.state.replace_owner(
            prior_owner,
            prepared.scrollbar_owner,
            window,
            cx
        ));
        // Release old page custody only once the successor publication is coherent.
        drop((
            prior_geometry,
            prior_surface,
            prior_residency,
            prior_objects,
        ));
        drop(prior_custody);
        self.observe_surface_admission_peak(owners.surface_charge);
        self.observe_realization_ownership();
        cx.notify();
    }
}
