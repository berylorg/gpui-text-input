use super::*;

#[derive(Clone, Copy)]
pub(super) struct MeasuredLayout {
    receipt: RangeMeasuredLayoutRejection,
    rejected: bool,
}

impl RangeTextInput {
    pub fn measured_layout_rejection(&self) -> Option<RangeMeasuredLayoutRejection> {
        self.measured_layout
            .filter(|measurement| measurement.rejected)
            .map(|measurement| measurement.receipt)
    }

    pub(super) fn has_pending_measured_layout(&self) -> bool {
        self.measured_layout
            .is_some_and(|measurement| !measurement.rejected)
    }

    pub(super) fn retire_measured_layout(&mut self) {
        self.measured_layout = None;
    }

    pub(super) fn record_measured_layout(
        &mut self,
        wrap_width: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.mounted
            || self.resident_protection.is_some()
            || wrap_width <= Pixels::ZERO
            || !f32::from(wrap_width).is_finite()
        {
            return;
        }
        let receipt = RangeMeasuredLayoutRejection {
            binding: self.config.binding,
            wrap_width,
        };
        if self
            .measured_layout
            .is_some_and(|measurement| measurement.receipt == receipt)
        {
            return;
        }
        if wrap_width == self.config.layout.wrap_width && self.measured_layout.is_none() {
            return;
        }
        self.measured_layout = Some(MeasuredLayout {
            receipt,
            rejected: false,
        });
        self.defer_measured_layout(window, cx);
    }

    pub(super) fn defer_measured_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.measured_layout_callback_scheduled
            || !self.has_pending_measured_layout()
            || !self.mounted
            || self.resident_protection.is_some()
        {
            return;
        }
        self.measured_layout_callback_scheduled = true;
        cx.defer_in(window, |input, window, cx| {
            input.measured_layout_callback_scheduled = false;
            input.service_measured_layout(window, cx);
        });
    }

    fn reject_measured_layout(&mut self, cx: &mut Context<Self>) {
        let Some(measurement) = self.measured_layout.as_mut() else {
            return;
        };
        if measurement.rejected {
            return;
        }
        measurement.rejected = true;
        cx.emit(RangeTextInputEvent::MeasuredLayoutRejected(
            measurement.receipt,
        ));
        cx.notify();
    }

    pub(super) fn service_measured_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(measurement) = self.measured_layout else {
            return;
        };
        if !self.mounted || measurement.receipt.binding != self.config.binding {
            self.retire_measured_layout();
            return;
        }
        if measurement.rejected
            || self.resident_protection.is_some()
            || self.edits.active_key().is_some()
        {
            return;
        }
        if self.service_pending_configuration_intent(cx).is_err() {
            self.reject_measured_layout(cx);
            return;
        }
        if self.pending_layout_intent.is_some() || self.pending_presentation_intent.is_some() {
            return;
        }
        if measurement.receipt.wrap_width == self.config.layout.wrap_width {
            self.retire_measured_layout();
            cx.notify();
            return;
        }
        if !self.try_spend_realization_credit(cx) {
            return;
        }
        let mut layout = self.config.layout.clone();
        layout.wrap_width = measurement.receipt.wrap_width;
        let candidate = self.prepare_layout_transition(
            layout,
            self.config.style.clone(),
            self.pending_target_intent,
        );
        let candidate = match candidate {
            Ok(candidate) => candidate,
            Err(_) => {
                self.refund_realization_credit();
                self.reject_measured_layout(cx);
                return;
            }
        };
        self.commit_widget_transition(candidate, Some(cx));
        self.pending_target_intent = None;
        match self.service_geometry_until_external_boundary(window, cx) {
            Ok(()) => self.retire_measured_layout(),
            Err(_) => self.reject_measured_layout(cx),
        }
    }
}
