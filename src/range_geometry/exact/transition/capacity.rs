use super::ExactGeometryError;

#[derive(Clone, Copy)]
pub(crate) struct PreparationCapacity {
    pub(crate) bytes: usize,
    pub(crate) items: usize,
    pub(crate) max_bytes: usize,
    pub(crate) max_items: usize,
}

impl PreparationCapacity {
    pub(super) fn admit(
        self,
        bytes: usize,
        items: usize,
    ) -> Result<(usize, usize), ExactGeometryError> {
        let bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or(ExactGeometryError::CapacityExceeded)?;
        let items = self
            .items
            .checked_add(items)
            .ok_or(ExactGeometryError::CapacityExceeded)?;
        if bytes > self.max_bytes || items > self.max_items {
            return Err(ExactGeometryError::CapacityExceeded);
        }
        Ok((bytes, items))
    }
}
