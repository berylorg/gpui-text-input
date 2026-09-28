use super::*;

impl ExactGeometryOwner {
    pub fn admit_page_with_capacity(
        &mut self,
        key: GeometryJobKey,
        page: &RangePage,
        text_system: &WindowTextSystem,
        max_bytes: usize,
        max_items: usize,
    ) -> Result<ExactGeometryAdmission, ExactGeometryFailure> {
        self.admit_page_inner(key, page, text_system, max_bytes, max_items, false)
    }

    pub fn admit_resident_page_with_capacity(
        &mut self,
        key: GeometryJobKey,
        page: &RangePage,
        text_system: &WindowTextSystem,
        max_bytes: usize,
        max_items: usize,
    ) -> Result<ExactGeometryAdmission, ExactGeometryFailure> {
        self.admit_page_inner(key, page, text_system, max_bytes, max_items, true)
    }

    pub fn admit_object_page_with_capacity(
        &mut self,
        key: GeometryJobKey,
        text_page: &RangePage,
        object_page: &ObjectPage,
        text_system: &WindowTextSystem,
        max_bytes: usize,
        max_items: usize,
    ) -> Result<ExactGeometryAdmission, ExactGeometryFailure> {
        self.admit_object_page_inner(
            key,
            text_page,
            object_page,
            text_system,
            max_bytes,
            max_items,
            false,
        )
    }

    pub fn admit_resident_object_page_with_capacity(
        &mut self,
        key: GeometryJobKey,
        text_page: &RangePage,
        object_page: &ObjectPage,
        text_system: &WindowTextSystem,
        max_bytes: usize,
        max_items: usize,
    ) -> Result<ExactGeometryAdmission, ExactGeometryFailure> {
        self.admit_object_page_inner(
            key,
            text_page,
            object_page,
            text_system,
            max_bytes,
            max_items,
            true,
        )
    }
}
