use super::ExactGeometryError;

pub(crate) struct PreparationCapacity {
    pub(crate) refused_capacity: Option<(usize, usize)>,
    pub(crate) bytes: usize,
    pub(crate) items: usize,
    pub(crate) max_bytes: usize,
    pub(crate) max_items: usize,
    pub(crate) peak_bytes: usize,
    pub(crate) peak_items: usize,
}

impl PreparationCapacity {
    pub(super) fn admit(
        &mut self,
        bytes: usize,
        items: usize,
    ) -> Result<(usize, usize), ExactGeometryError> {
        self.admit_from(self.bytes, self.items, bytes, items)
    }

    pub(super) fn admit_from(
        &mut self,
        base_bytes: usize,
        base_items: usize,
        bytes: usize,
        items: usize,
    ) -> Result<(usize, usize), ExactGeometryError> {
        self.refused_capacity = None;
        let bytes = base_bytes.checked_add(bytes);
        let items = base_items.checked_add(items);
        self.peak_bytes = self.peak_bytes.max(bytes.unwrap_or(usize::MAX));
        self.peak_items = self.peak_items.max(items.unwrap_or(usize::MAX));
        let bytes = bytes.ok_or(ExactGeometryError::CapacityExceeded)?;
        let items = items.ok_or(ExactGeometryError::CapacityExceeded)?;
        if bytes > self.max_bytes || items > self.max_items {
            self.refused_capacity = Some((bytes, items));
            return Err(ExactGeometryError::CapacityExceeded);
        }
        Ok((bytes, items))
    }
}
