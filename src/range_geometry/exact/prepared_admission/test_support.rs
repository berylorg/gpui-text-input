use super::*;

pub fn is_enclosing_capacity_refusal(failure: &ExactGeometryFailure) -> bool {
    failure.capacity_refusal == Some(super::super::types::CapacityRefusal::Enclosing)
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
