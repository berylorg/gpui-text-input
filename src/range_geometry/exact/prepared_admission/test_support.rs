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

pub struct PreparationCapacityProbe(super::super::capacity_observation::CapacityObservations);

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
        .remaining_capacity(occupied.0, occupied.1)
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
            capacity.0,
            capacity.1,
        ),
        None => owner.prepare_response_page_with_capacity(
            key,
            page,
            text_system,
            resident,
            index,
            successor,
            capacity.0,
            capacity.1,
        ),
    }
    .map(PreparedResponseProbe)
}
