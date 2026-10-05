use super::*;

pub struct ClipboardProvenanceReplay(Option<ProvenanceCollection>);

impl ClipboardProvenanceReplay {
    pub fn new(limits: ClipboardProvenanceLimits) -> Self {
        Self(Some(ProvenanceCollection::new(limits)))
    }

    pub fn push(
        &mut self,
        object: &InlineObjectFact,
        output_start: usize,
        output_end: usize,
    ) -> Result<bool, ()> {
        let collection = self.0.as_mut().ok_or(())?;
        if collection.items.is_none() {
            collection.allocate_builder()?;
        }
        collection.push(object, output_start, output_end)
    }

    pub fn has_items(&self) -> bool {
        self.0.as_ref().is_some_and(ProvenanceCollection::has_items)
    }

    pub fn emit(&mut self, clipboard: ClipboardKey) -> Result<ClipboardProvenancePage, ()> {
        self.0.as_mut().ok_or(())?.emit(clipboard)
    }

    pub fn acknowledge(&mut self, page: ClipboardProvenancePage) -> Result<(), bool> {
        let result = self.0.as_mut().ok_or(true)?.acknowledge(page);
        if result == Err(true) {
            self.0 = None;
        }
        result
    }

    pub fn closure(
        &self,
        clipboard: ClipboardKey,
        text: &str,
    ) -> Result<ClipboardProvenanceClosure, ()> {
        self.0.as_ref().ok_or(())?.closure(clipboard, text)
    }

    pub fn retained_bytes(&self) -> usize {
        self.0
            .as_ref()
            .map_or(0, ProvenanceCollection::retained_bytes)
    }
}
