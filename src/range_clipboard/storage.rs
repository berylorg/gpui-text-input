use std::{
    alloc::{Layout, alloc, dealloc},
    fmt,
    mem::ManuallyDrop,
    ptr::{self, NonNull},
    slice,
};

pub(super) struct ExactArray<T> {
    pointer: NonNull<T>,
    len: usize,
    capacity: usize,
}

impl<T> ExactArray<T> {
    pub(super) fn try_with_capacity(capacity: usize) -> Result<Self, ()> {
        let pointer = if capacity == 0 || size_of::<T>() == 0 {
            NonNull::dangling()
        } else {
            let layout = Layout::array::<T>(capacity).map_err(|_| ())?;
            NonNull::new(unsafe { alloc(layout) }.cast()).ok_or(())?
        };
        Ok(Self {
            pointer,
            len: 0,
            capacity,
        })
    }

    pub(super) const fn len(&self) -> usize {
        self.len
    }

    pub(super) const fn capacity(&self) -> usize {
        self.capacity
    }

    pub(super) fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(super) fn as_slice(&self) -> &[T] {
        unsafe { slice::from_raw_parts(self.pointer.as_ptr(), self.len) }
    }

    pub(super) fn push(&mut self, value: T) -> Result<(), T> {
        if self.len == self.capacity {
            return Err(value);
        }
        unsafe { self.pointer.as_ptr().add(self.len).write(value) };
        self.len += 1;
        Ok(())
    }

    pub(super) fn into_vec(self) -> Vec<T> {
        if self.capacity == 0 {
            return Vec::new();
        }
        let this = ManuallyDrop::new(self);
        unsafe { Vec::from_raw_parts(this.pointer.as_ptr(), this.len, this.capacity) }
    }
}

impl<T: Copy> ExactArray<T> {
    pub(super) fn extend_from_slice(&mut self, values: &[T]) -> Result<(), ()> {
        let next_len = self.len.checked_add(values.len()).ok_or(())?;
        if next_len > self.capacity {
            return Err(());
        }
        unsafe {
            ptr::copy_nonoverlapping(
                values.as_ptr(),
                self.pointer.as_ptr().add(self.len),
                values.len(),
            )
        };
        self.len = next_len;
        Ok(())
    }
}

impl<T> Drop for ExactArray<T> {
    fn drop(&mut self) {
        unsafe {
            ptr::drop_in_place(ptr::slice_from_raw_parts_mut(
                self.pointer.as_ptr(),
                self.len,
            ))
        };
        if self.capacity != 0 && size_of::<T>() != 0 {
            let layout = Layout::array::<T>(self.capacity).expect("validated exact allocation");
            unsafe { dealloc(self.pointer.as_ptr().cast(), layout) };
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for ExactArray<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExactArray")
            .field("items", &self.as_slice())
            .field("capacity", &self.capacity)
            .finish()
    }
}

impl<T: PartialEq> PartialEq for ExactArray<T> {
    fn eq(&self, other: &Self) -> bool {
        self.capacity == other.capacity && self.as_slice() == other.as_slice()
    }
}

impl<T: Eq> Eq for ExactArray<T> {}

unsafe impl<T: Send> Send for ExactArray<T> {}
unsafe impl<T: Sync> Sync for ExactArray<T> {}

#[derive(Debug, Default)]
pub(super) struct ExactOutput {
    bytes: Option<ExactArray<u8>>,
}

impl ExactOutput {
    pub(super) fn allocate(&mut self, capacity: usize) -> Result<(), ()> {
        if self.bytes.is_some() || capacity == 0 {
            return Err(());
        }
        self.bytes = Some(ExactArray::try_with_capacity(capacity)?);
        Ok(())
    }

    pub(super) fn len(&self) -> usize {
        self.bytes.as_ref().map_or(0, ExactArray::len)
    }

    pub(super) fn capacity(&self) -> usize {
        self.bytes.as_ref().map_or(0, ExactArray::capacity)
    }

    pub(super) fn push_str(&mut self, text: &str) -> Result<(), ()> {
        if text.is_empty() {
            return Ok(());
        }
        self.bytes
            .as_mut()
            .ok_or(())?
            .extend_from_slice(text.as_bytes())
    }

    pub(super) fn as_str(&self) -> &str {
        let bytes = self.bytes.as_ref().map_or(&[][..], ExactArray::as_slice);
        unsafe { std::str::from_utf8_unchecked(bytes) }
    }

    pub(super) fn get(&self, range: std::ops::Range<usize>) -> Option<&str> {
        self.as_str().get(range)
    }

    pub(super) fn clear(&mut self) {
        if let Some(bytes) = self.bytes.as_mut() {
            bytes.len = 0;
        }
    }

    pub(super) fn into_string(self) -> String {
        let Some(bytes) = self.bytes else {
            return String::new();
        };
        let bytes = bytes.into_vec();
        unsafe { String::from_utf8_unchecked(bytes) }
    }
}
