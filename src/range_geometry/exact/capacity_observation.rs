use super::{ExactGeometryError, types::CapacityRefusal};

pub struct CapacityObservations {
    configured_limit: (usize, usize),
    enclosing_limit: (usize, usize),
    preparation_baseline: (usize, usize),
    enclosing_baseline: (usize, usize),
    pub(super) configured_peak: (usize, usize),
    pub(super) enclosing_peak: (usize, usize),
    pub(super) refusal: Option<CapacityRefusal>,
}

impl CapacityObservations {
    pub(super) fn new(configured: (usize, usize), enclosing: (usize, usize)) -> Self {
        Self::with_baselines(configured, enclosing, (0, 0), (0, 0))
    }

    pub(super) fn with_baselines(
        configured: (usize, usize),
        enclosing: (usize, usize),
        preparation_baseline: (usize, usize),
        enclosing_baseline: (usize, usize),
    ) -> Self {
        Self {
            configured_limit: configured,
            enclosing_limit: enclosing,
            preparation_baseline,
            enclosing_baseline,
            configured_peak: (0, 0),
            enclosing_peak: (0, 0),
            refusal: None,
        }
    }

    pub fn clear_refusal(&mut self) {
        self.refusal = None;
    }

    pub(super) fn remaining_capacity(
        &self,
        occupied: (usize, usize),
    ) -> Result<(usize, usize), ExactGeometryError> {
        let remaining =
            |raw: usize, baseline: usize, current: usize, configured: usize, enclosing: usize| {
                let mapped = current.checked_add(raw.checked_sub(baseline)?)?;
                Some(
                    configured
                        .checked_sub(raw)?
                        .min(enclosing.checked_sub(mapped)?),
                )
            };
        Ok((
            remaining(
                occupied.0,
                self.preparation_baseline.0,
                self.enclosing_baseline.0,
                self.configured_limit.0,
                self.enclosing_limit.0,
            )
            .ok_or(ExactGeometryError::CapacityExceeded)?,
            remaining(
                occupied.1,
                self.preparation_baseline.1,
                self.enclosing_baseline.1,
                self.configured_limit.1,
                self.enclosing_limit.1,
            )
            .ok_or(ExactGeometryError::CapacityExceeded)?,
        ))
    }

    pub fn observe_preparation(
        &mut self,
        raw: (usize, usize),
        shared_credit: (usize, usize),
    ) -> Result<(), ExactGeometryError> {
        self.clear_refusal();
        let translate = |raw: usize, baseline: usize, enclosing: usize, credit: usize| {
            let growth = raw.checked_sub(baseline)?;
            if credit > growth {
                return None;
            }
            enclosing.checked_add(growth)?.checked_sub(credit)
        };
        let bytes = translate(
            raw.0,
            self.preparation_baseline.0,
            self.enclosing_baseline.0,
            shared_credit.0,
        )
        .ok_or(ExactGeometryError::CapacityExceeded)?;
        let items = translate(
            raw.1,
            self.preparation_baseline.1,
            self.enclosing_baseline.1,
            shared_credit.1,
        )
        .ok_or(ExactGeometryError::CapacityExceeded)?;
        self.observe(raw, (bytes, items))
    }

    pub fn observe(
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
