use super::{ExactGeometryError, types::CapacityRefusal};

pub(super) struct CapacityObservations {
    configured_limit: (usize, usize),
    enclosing_limit: (usize, usize),
    pub(super) configured_peak: (usize, usize),
    pub(super) enclosing_peak: (usize, usize),
    pub(super) refusal: Option<CapacityRefusal>,
}

impl CapacityObservations {
    pub(super) fn new(configured: (usize, usize), enclosing: (usize, usize)) -> Self {
        Self {
            configured_limit: configured,
            enclosing_limit: enclosing,
            configured_peak: (0, 0),
            enclosing_peak: (0, 0),
            refusal: None,
        }
    }

    pub(super) fn observe(
        &mut self,
        configured: (usize, usize),
        enclosing: (usize, usize),
    ) -> Result<(), ExactGeometryError> {
        self.configured_peak.0 = self.configured_peak.0.max(configured.0);
        self.configured_peak.1 = self.configured_peak.1.max(configured.1);
        self.enclosing_peak.0 = self.enclosing_peak.0.max(enclosing.0);
        self.enclosing_peak.1 = self.enclosing_peak.1.max(enclosing.1);
        self.refusal =
            if configured.0 > self.configured_limit.0 || configured.1 > self.configured_limit.1 {
                Some(CapacityRefusal::Configured)
            } else if enclosing.0 > self.enclosing_limit.0 || enclosing.1 > self.enclosing_limit.1 {
                Some(CapacityRefusal::Enclosing)
            } else {
                None
            };
        if self.refusal.is_some() {
            Err(ExactGeometryError::CapacityExceeded)
        } else {
            Ok(())
        }
    }
}
