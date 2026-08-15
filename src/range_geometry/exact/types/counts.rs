/// Exact byte and semantic-record residency of the geometry owner graph.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExactGeometryCounts {
    pub owner_items: usize,
    pub owner_bytes: usize,
    pub input_items: usize,
    pub input_bytes: usize,
    pub desired_target_items: usize,
    pub desired_target_bytes: usize,
    pub active_job_items: usize,
    pub active_job_bytes: usize,
    pub pending_page_items: usize,
    pub pending_page_bytes: usize,
    pub pending_object_page_items: usize,
    pub pending_object_page_bytes: usize,
    pub scan_buffer_items: usize,
    pub scan_buffer_bytes: usize,
    pub active_atom_items: usize,
    pub active_atom_bytes: usize,
    pub deferred_object_items: usize,
    pub deferred_object_bytes: usize,
    pub checkpoints: usize,
    pub checkpoint_bytes: usize,
    pub continuation_items: usize,
    pub continuation_bytes: usize,
    /// Exact GPUI semantic records retained by output fragments.
    pub output_items: usize,
    pub output_record_bytes: usize,
    pub output_payload_bytes: usize,
    pub publication_items: usize,
    pub publication_bytes: usize,
}

impl ExactGeometryCounts {
    pub fn total_bytes(self) -> usize {
        self.owner_bytes
            .saturating_add(self.input_bytes)
            .saturating_add(self.desired_target_bytes)
            .saturating_add(self.active_job_bytes)
            .saturating_add(self.pending_page_bytes)
            .saturating_add(self.pending_object_page_bytes)
            .saturating_add(self.scan_buffer_bytes)
            .saturating_add(self.active_atom_bytes)
            .saturating_add(self.deferred_object_bytes)
            .saturating_add(self.checkpoint_bytes)
            .saturating_add(self.continuation_bytes)
            .saturating_add(self.output_record_bytes)
            .saturating_add(self.output_payload_bytes)
            .saturating_add(self.publication_bytes)
    }

    /// Checked-by-admission semantic records retained by the owner graph.
    pub fn total_items(self) -> usize {
        self.owner_items
            .saturating_add(self.input_items)
            .saturating_add(self.desired_target_items)
            .saturating_add(self.active_job_items)
            .saturating_add(self.pending_page_items)
            .saturating_add(self.pending_object_page_items)
            .saturating_add(self.scan_buffer_items)
            .saturating_add(self.active_atom_items)
            .saturating_add(self.deferred_object_items)
            .saturating_add(self.checkpoints)
            .saturating_add(self.continuation_items)
            .saturating_add(self.output_items)
            .saturating_add(self.publication_items)
    }
}
