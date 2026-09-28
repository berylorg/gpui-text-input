use super::*;

pub use super::super::capacity_observation::CapacityObservations;

pub fn fragment_presentation_overlap(
    fragments: &[gpui::StreamingLayoutFragment],
    pages: &[&ObjectPage],
) -> Option<usize> {
    super::super::accounting::fragment_presentation_overlap_bytes(fragments, pages.iter().copied())
}

pub fn shared_object_display(object: &crate::InlineObjectFact) -> gpui::SharedString {
    object.presentation().shared_display()
}

pub fn prepare_continuation_copy(
    owner: &ExactGeometryOwner,
    key: crate::GeometryJobKey,
    index: bool,
    current: Option<(usize, usize)>,
    limit: (usize, usize),
) -> Result<((usize, usize), (usize, usize)), ExactGeometryFailure> {
    let active = owner.response_active(key, index)?;
    let capacity = current.map_or(ResponseCapacity::Geometry(limit), |current| {
        ResponseCapacity::Enclosing { current, limit }
    });
    let mut budget = owner.prepared_budget(0, 0, capacity)?;
    let _copy = prepare_response_continuation(&mut budget, active, capacity)?;
    Ok((
        (budget.peak_bytes, budget.peak_items),
        budget.observations.as_ref().unwrap().enclosing_peak,
    ))
}

pub struct PreparationCapacityProbe(super::super::capacity_observation::CapacityObservations);

#[allow(clippy::too_many_arguments)]
pub fn prepare_object_finalization(
    owner: &ExactGeometryOwner,
    key: crate::GeometryJobKey,
    index: bool,
    text: &RangePage,
    objects: &ObjectPage,
    text_system: &WindowTextSystem,
    current: Option<(usize, usize)>,
    limit: (usize, usize),
) -> Result<((usize, usize), (usize, usize), (usize, usize)), ExactGeometryFailure> {
    let (mut candidate, mut scan_budget, baseline) = object_scan_budget(
        owner,
        key,
        index,
        text,
        objects,
        None,
        (usize::MAX, usize::MAX),
    )?;
    let inputs = owner.inputs.as_deref().unwrap();
    super::super::scan::process_object_page(
        &mut candidate,
        text,
        objects,
        text_system,
        inputs,
        owner.limits,
        inputs.binding.extent().byte_len(),
        &mut scan_budget,
        current.is_some(),
    )
    .map_err(|error| prepared_failure(error, ExactGeometryFailureStage::Scan, &scan_budget))?;
    assert!(objects.complete());
    assert!(candidate.scanner.deferred_object.is_none());
    let capacity = current.map_or(ResponseCapacity::Geometry(limit), |current| {
        ResponseCapacity::Enclosing { current, limit }
    });
    let counts = owner.counts();
    let mut budget = owner.prepared_budget(
        baseline.0 - counts.total_bytes(),
        baseline.1 - counts.total_items(),
        capacity,
    )?;
    let result = owner.finalize_object_response(
        &mut candidate,
        text_system,
        scan_budget.output_display_bytes,
        &mut budget,
    );
    assert_eq!(budget.output_display_bytes, 0);
    result?;
    Ok((
        (budget.peak_bytes, budget.peak_items),
        budget.observations.as_ref().unwrap().enclosing_peak,
        baseline,
    ))
}

#[allow(clippy::too_many_arguments)]
pub fn prepare_object_scan(
    owner: &ExactGeometryOwner,
    key: crate::GeometryJobKey,
    index: bool,
    text: &RangePage,
    objects: &ObjectPage,
    text_system: &WindowTextSystem,
    current: Option<(usize, usize)>,
    limit: (usize, usize),
) -> Result<((usize, usize), (usize, usize), (usize, usize)), ExactGeometryFailure> {
    let (mut candidate, mut budget, baseline) =
        object_scan_budget(owner, key, index, text, objects, current, limit)?;
    super::super::scan::process_object_page(
        &mut candidate,
        text,
        objects,
        text_system,
        owner.inputs.as_deref().unwrap(),
        owner.limits,
        owner.inputs.as_deref().unwrap().binding.extent().byte_len(),
        &mut budget,
        current.is_some(),
    )
    .map_err(|error| prepared_failure(error, ExactGeometryFailureStage::Scan, &budget))?;
    if index || current.is_none() {
        assert_eq!(budget.output_display_bytes, 0);
    } else {
        let expected = candidate
            .scanner
            .fragments
            .iter()
            .map(|fragment| match fragment {
                gpui::StreamingLayoutFragment::InlineObject(fragment) => {
                    fragment.presentation.len()
                }
                _ => 0,
            })
            .sum::<usize>();
        assert_eq!(budget.output_display_bytes, expected);
    }
    Ok((
        (budget.peak_bytes, budget.peak_items),
        budget.observations.as_ref().unwrap().enclosing_peak,
        baseline,
    ))
}

#[allow(clippy::too_many_arguments)]
pub fn prepare_detached_inline(
    owner: &ExactGeometryOwner,
    key: crate::GeometryJobKey,
    index: bool,
    text: &RangePage,
    objects: &ObjectPage,
    text_system: &WindowTextSystem,
    current: Option<(usize, usize)>,
    limit: (usize, usize),
) -> Result<((usize, usize), (usize, usize), (usize, usize)), ExactGeometryFailure> {
    let (mut candidate, mut budget, baseline) =
        object_scan_budget(owner, key, index, text, objects, current, limit)?;
    let deferred = candidate
        .scanner
        .deferred_object
        .take()
        .ok_or_else(|| owner.prepared_validation_failure(ExactGeometryError::SourceContract))?;
    let next = objects
        .objects()
        .first()
        .ok_or_else(|| owner.prepared_validation_failure(ExactGeometryError::SourceContract))?;
    let prior = (
        budget.fixed_bytes,
        budget.fixed_items,
        budget.detached_display_bytes,
    );
    let result = super::super::scan::admit_deferred_object(
        &mut candidate,
        text,
        &deferred,
        next,
        text_system,
        owner.inputs.as_deref().unwrap(),
        owner.limits,
        owner.inputs.as_deref().unwrap().binding.extent().byte_len(),
        &mut budget,
        current.is_some(),
    );
    assert_eq!(
        (
            budget.fixed_bytes,
            budget.fixed_items,
            budget.detached_display_bytes
        ),
        prior
    );
    result.map_err(|error| prepared_failure(error, ExactGeometryFailureStage::Scan, &budget))?;
    Ok((
        (budget.peak_bytes, budget.peak_items),
        budget.observations.as_ref().unwrap().enclosing_peak,
        baseline,
    ))
}

#[allow(clippy::too_many_arguments)]
fn object_scan_budget(
    owner: &ExactGeometryOwner,
    key: crate::GeometryJobKey,
    index: bool,
    text: &RangePage,
    objects: &ObjectPage,
    current: Option<(usize, usize)>,
    limit: (usize, usize),
) -> Result<(Box<ActiveJob>, AdmissionBudget, (usize, usize)), ExactGeometryFailure> {
    let active = owner.response_active(key, index)?;
    let capacity = current.map_or(ResponseCapacity::Geometry(limit), |current| {
        ResponseCapacity::Enclosing { current, limit }
    });
    let page_bytes = text
        .retained_charge()
        .bytes()
        .checked_add(objects.retained_charge().bytes())
        .ok_or_else(|| owner.prepared_validation_failure(ExactGeometryError::CapacityExceeded))?;
    let page_items = text
        .retained_charge()
        .items()
        .checked_add(objects.retained_charge().allocated_items())
        .and_then(|items| items.checked_add(1))
        .ok_or_else(|| owner.prepared_validation_failure(ExactGeometryError::CapacityExceeded))?;
    let identity = ResponseCapacity::Geometry((usize::MAX, usize::MAX));
    let mut copy_budget = owner.prepared_budget(page_bytes, page_items, identity)?;
    let (candidate, _) = prepare_response_continuation(&mut copy_budget, active, identity)?;
    let budget = owner.prepared_budget(page_bytes, page_items, capacity)?;
    let baseline = (
        budget.fixed_bytes + page_bytes,
        budget.fixed_items + page_items,
    );
    Ok((candidate, budget, baseline))
}

pub fn prepare_deferred_tail(
    owner: &ExactGeometryOwner,
    key: crate::GeometryJobKey,
    index: bool,
    objects: &ObjectPage,
    current: Option<(usize, usize)>,
    limit: (usize, usize),
) -> Result<((usize, usize), (usize, usize), (usize, usize)), ExactGeometryFailure> {
    let active = owner.response_active(key, index)?;
    let capacity = current.map_or(ResponseCapacity::Geometry(limit), |current| {
        ResponseCapacity::Enclosing { current, limit }
    });
    let page_bytes = objects.retained_charge().bytes();
    let page_items = objects
        .retained_charge()
        .allocated_items()
        .checked_add(1)
        .ok_or_else(|| owner.prepared_validation_failure(ExactGeometryError::CapacityExceeded))?;
    let mut copy_budget = owner.prepared_budget(
        page_bytes,
        page_items,
        ResponseCapacity::Geometry((usize::MAX, usize::MAX)),
    )?;
    let (mut candidate, _) = prepare_response_continuation(
        &mut copy_budget,
        active,
        ResponseCapacity::Geometry((usize::MAX, usize::MAX)),
    )?;
    let mut budget = owner.prepared_budget(page_bytes, page_items, capacity)?;
    let baseline = (
        budget.fixed_bytes + page_bytes,
        budget.fixed_items + page_items,
    );
    let object = objects
        .objects()
        .last()
        .filter(|_| !objects.complete())
        .ok_or_else(|| owner.prepared_validation_failure(ExactGeometryError::SourceContract))?;
    super::super::scan::defer_object(
        &mut candidate,
        object,
        owner.inputs.as_deref().unwrap(),
        &mut budget,
        current.is_some(),
    )
    .map_err(|error| prepared_failure(error, ExactGeometryFailureStage::Scan, &budget))?;
    Ok((
        (budget.peak_bytes, budget.peak_items),
        budget.observations.as_ref().unwrap().enclosing_peak,
        baseline,
    ))
}

impl PreparationCapacityProbe {
    pub fn with_baselines(
        configured: (usize, usize),
        enclosing: (usize, usize),
        preparation: (usize, usize),
        current: (usize, usize),
    ) -> Self {
        Self(CapacityObservations::with_baselines(
            configured,
            enclosing,
            preparation,
            current,
        ))
    }

    pub fn observe_preparation(
        &mut self,
        raw: (usize, usize),
        shared_credit: (usize, usize),
    ) -> Result<(), ExactGeometryError> {
        self.0.observe_preparation(raw, shared_credit)
    }

    pub fn new(configured: (usize, usize), enclosing: (usize, usize)) -> Self {
        Self(super::super::capacity_observation::CapacityObservations::new(configured, enclosing))
    }

    pub fn observe(
        &mut self,
        configured: (usize, usize),
        enclosing: (usize, usize),
    ) -> Result<(), ExactGeometryError> {
        self.0.observe(configured, enclosing)
    }

    pub fn peaks(&self) -> ((usize, usize), (usize, usize)) {
        (self.0.configured_peak, self.0.enclosing_peak)
    }

    pub fn output_capacity(
        &self,
        occupied: (usize, usize),
        existing_credit: (usize, usize),
        prospective_credit: (usize, usize),
    ) -> Result<(usize, usize), ExactGeometryError> {
        self.0
            .output_capacity(occupied, existing_credit, prospective_credit)
    }

    pub fn into_failure(self, error: ExactGeometryError) -> ExactGeometryFailure {
        prepared_failure(
            error,
            ExactGeometryFailureStage::Finalize,
            &self.into_budget(),
        )
    }

    pub fn observe_startup(
        &mut self,
        occupied: (usize, usize),
        position: gpui::StreamingLayoutPosition,
    ) -> Result<(), ExactGeometryError> {
        let prior = std::mem::replace(self, Self::new((0, 0), (0, 0)));
        let mut budget = prior.into_budget();
        let result = budget.admit_layout_startup(occupied, position);
        self.0 = budget.observations.take().unwrap();
        result
    }

    fn into_budget(self) -> AdmissionBudget {
        AdmissionBudget {
            detached_display_bytes: 0,
            output_display_bytes: 0,
            peak_bytes: self.0.configured_peak.0,
            peak_items: self.0.configured_peak.1,
            observations: Some(self.0),
            refused_capacity: None,
            fixed_bytes: 0,
            fixed_items: 0,
            page_payload_bytes: 0,
            page_items: 0,
            max_bytes: usize::MAX,
            max_items: usize::MAX,
            failure_stage: None,
        }
    }

    pub fn observe_nested(
        &mut self,
        base: (usize, usize),
        additional: (usize, usize),
    ) -> Result<(usize, usize), ExactGeometryError> {
        let mut capacity = super::super::transition::PreparationCapacity {
            observations: Some(std::mem::replace(
                &mut self.0,
                CapacityObservations::new((0, 0), (0, 0)),
            )),
            refused_capacity: None,
            bytes: base.0,
            items: base.1,
            max_bytes: usize::MAX,
            max_items: usize::MAX,
            peak_bytes: 0,
            peak_items: 0,
        };
        let result = capacity.admit_from(base.0, base.1, additional.0, additional.1);
        self.0 = capacity.observations.take().unwrap();
        result
    }

    pub fn configured_refusal(&self) -> bool {
        self.0.refusal == Some(super::super::types::CapacityRefusal::Configured)
    }

    pub fn enclosing_refusal(&self) -> bool {
        self.0.refusal == Some(super::super::types::CapacityRefusal::Enclosing)
    }
}

pub fn preparation_remaining_capacity(
    occupied: (usize, usize),
    configured: (usize, usize),
    enclosing: (usize, usize),
) -> Result<(usize, usize), ExactGeometryFailure> {
    preparation_remaining_capacity_with_baselines(occupied, configured, enclosing, (0, 0), (0, 0))
}

pub fn preparation_remaining_capacity_with_baselines(
    occupied: (usize, usize),
    configured: (usize, usize),
    enclosing: (usize, usize),
    preparation: (usize, usize),
    current: (usize, usize),
) -> Result<(usize, usize), ExactGeometryFailure> {
    let mut budget = AdmissionBudget {
        detached_display_bytes: 0,
        output_display_bytes: 0,
        observations: Some(CapacityObservations::with_baselines(
            configured,
            enclosing,
            preparation,
            current,
        )),
        refused_capacity: None,
        fixed_bytes: 0,
        fixed_items: 0,
        page_payload_bytes: 0,
        page_items: 0,
        max_bytes: configured.0.min(enclosing.0),
        max_items: configured.1.min(enclosing.1),
        peak_bytes: 0,
        peak_items: 0,
        failure_stage: None,
    };
    budget
        .output_capacity(occupied.0, occupied.1, 0)
        .map_err(|error| prepared_failure(error, ExactGeometryFailureStage::Finalize, &budget))
}

pub fn is_enclosing_capacity_refusal(failure: &ExactGeometryFailure) -> bool {
    failure.capacity_refusal == Some(super::super::types::CapacityRefusal::Enclosing)
}

pub fn enclosing_failure_peak(failure: &ExactGeometryFailure) -> Option<(usize, usize)> {
    failure.enclosing_peak()
}

pub fn is_configured_capacity_refusal(failure: &ExactGeometryFailure) -> bool {
    failure.capacity_refusal == Some(super::super::types::CapacityRefusal::Configured)
}

pub fn owner_presentation_overlap(
    owner: &ExactGeometryOwner,
    pages: &[&ObjectPage],
) -> Option<usize> {
    owner.presentation_overlap_bytes(pages.iter().copied())
}

pub fn owner_retained_without_target(
    owner: &ExactGeometryOwner,
    pages: &[&ObjectPage],
) -> Option<crate::RangeSurfaceCharge> {
    owner.retained_without_target(pages.iter().copied())
}

pub fn session_response_successor_ids(
    session: &crate::RangePrepublicationSession,
) -> Option<[u64; 3]> {
    session.response_successor_ids_for_test()
}

#[derive(Debug)]
pub struct PreparedResponseProbe(PreparedTargetResponse);

impl PreparedResponseProbe {
    pub fn enclosing_peak(&self) -> Option<(usize, usize)> {
        self.0.enclosing_peak()
    }

    pub fn presentation_overlap(&self, pages: &[&ObjectPage]) -> Option<usize> {
        self.0.presentation_overlap_bytes(pages.iter().copied())
    }

    pub fn required_capacity(&self) -> (usize, usize) {
        (
            self.0.admission_required_bytes,
            self.0.admission_required_items,
        )
    }

    pub fn terminal_index(&self) -> bool {
        self.0.terminal_index().is_some()
    }

    pub fn terminal_target(&self) -> bool {
        self.0.terminal_target().is_some()
    }

    pub fn object_request(&self) -> Option<ObjectRequest> {
        match self.0.successor() {
            Some(PreparedTargetSuccessor::Object { request, .. }) => Some(request),
            _ => None,
        }
    }

    pub fn page_request(&self) -> Option<PageRequest> {
        match self.0.successor() {
            Some(PreparedTargetSuccessor::Page(request)) => Some(request),
            _ => None,
        }
    }

    pub fn commit(self, owner: &mut ExactGeometryOwner) {
        owner.commit_prepared_target_response(self.0);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn prepare_response(
    owner: &ExactGeometryOwner,
    key: crate::GeometryJobKey,
    page: &RangePage,
    objects: Option<&ObjectPage>,
    text_system: &WindowTextSystem,
    resident: bool,
    index: bool,
    successor_ids: (GeometryJobId, PageRequestId, ObjectRequestId),
    target: BlockTarget,
    capacity: (usize, usize),
) -> Result<PreparedResponseProbe, ExactGeometryFailure> {
    prepare_response_with_budget(
        owner,
        key,
        page,
        objects,
        text_system,
        resident,
        index,
        successor_ids,
        target,
        ResponseCapacity::Geometry(capacity),
    )
}

#[allow(clippy::too_many_arguments)]
pub fn prepare_response_with_enclosing(
    owner: &ExactGeometryOwner,
    key: crate::GeometryJobKey,
    page: &RangePage,
    objects: Option<&ObjectPage>,
    text_system: &WindowTextSystem,
    resident: bool,
    index: bool,
    successor_ids: (GeometryJobId, PageRequestId, ObjectRequestId),
    target: BlockTarget,
    current: (usize, usize),
    limit: (usize, usize),
) -> Result<PreparedResponseProbe, ExactGeometryFailure> {
    prepare_response_with_budget(
        owner,
        key,
        page,
        objects,
        text_system,
        resident,
        index,
        successor_ids,
        target,
        ResponseCapacity::Enclosing { current, limit },
    )
}

#[allow(clippy::too_many_arguments)]
fn prepare_response_with_budget(
    owner: &ExactGeometryOwner,
    key: crate::GeometryJobKey,
    page: &RangePage,
    objects: Option<&ObjectPage>,
    text_system: &WindowTextSystem,
    resident: bool,
    index: bool,
    successor_ids: (GeometryJobId, PageRequestId, ObjectRequestId),
    target: BlockTarget,
    capacity: ResponseCapacity,
) -> Result<PreparedResponseProbe, ExactGeometryFailure> {
    let successor = TargetResponseSuccessor {
        target_job_id: successor_ids.0,
        page_id: successor_ids.1,
        object_id: successor_ids.2,
        max_objects: 32,
        max_object_bytes: 128 * 1024,
        target,
        anchor: None,
        select_all: false,
    };
    match objects {
        Some(objects) => owner.prepare_response_object_page_with_capacity(
            key,
            page,
            objects,
            text_system,
            index,
            resident,
            successor,
            capacity,
        ),
        None => owner.prepare_response_page_with_capacity(
            key,
            page,
            text_system,
            resident,
            index,
            successor,
            capacity,
        ),
    }
    .map(PreparedResponseProbe)
}
