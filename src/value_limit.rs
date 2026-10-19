use std::{fmt, ops::Range};

use crate::{TextInputMode, TextInputState};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextInputValueLimitError {
    pub limit: usize,
    pub attempted_bytes: usize,
}

impl fmt::Display for TextInputValueLimitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "text input value exceeds {} UTF-8 bytes (attempted {})",
            self.limit, self.attempted_bytes
        )
    }
}

impl std::error::Error for TextInputValueLimitError {}

impl TextInputState {
    pub fn value_byte_limit(&self) -> Option<usize> {
        self.value_byte_limit
    }

    pub fn set_value_byte_limit(
        &mut self,
        limit: Option<usize>,
    ) -> Result<(), TextInputValueLimitError> {
        if let Some(limit) = limit
            && self.text.len() > limit
        {
            return Err(TextInputValueLimitError {
                limit,
                attempted_bytes: self.text.len(),
            });
        }
        self.value_byte_limit = limit;
        Ok(())
    }

    pub fn take_value_limit_error(&mut self) -> Option<TextInputValueLimitError> {
        self.value_limit_error.take()
    }

    pub fn set_read_only(&mut self, read_only: bool) {
        self.options = self.options.with_read_only(read_only);
    }

    pub(crate) fn admit_value_bytes(&mut self, attempted_bytes: usize) -> bool {
        self.value_limit_error = self
            .value_byte_limit
            .filter(|limit| attempted_bytes > *limit)
            .map(|limit| TextInputValueLimitError {
                limit,
                attempted_bytes,
            });
        self.value_limit_error.is_none()
    }

    pub(crate) fn admit_replacement(&mut self, range: &Range<usize>, replacement: &str) -> bool {
        self.admit_successor_bytes(
            self.text.len() - range.len(),
            normalized_value_bytes(replacement, self.mode()),
        )
    }

    pub(crate) fn admit_successor_bytes(
        &mut self,
        retained_bytes: usize,
        inserted_bytes: usize,
    ) -> bool {
        let Some(attempted_bytes) = retained_bytes.checked_add(inserted_bytes) else {
            self.value_limit_error = Some(TextInputValueLimitError {
                limit: self.value_byte_limit.unwrap_or(usize::MAX),
                attempted_bytes: usize::MAX,
            });
            return false;
        };
        self.admit_value_bytes(attempted_bytes)
    }
}

pub(crate) fn normalized_value_bytes(text: &str, mode: TextInputMode) -> usize {
    let mut bytes = 0usize;
    let mut previous_newline = false;
    let mut previous_cr = false;
    for ch in text.chars() {
        let next = match mode {
            TextInputMode::SingleLine if matches!(ch, '\r' | '\n') => {
                usize::from(!previous_newline)
            }
            TextInputMode::Multiline if ch == '\n' && previous_cr => 0,
            _ => ch.len_utf8(),
        };
        bytes = bytes.checked_add(next).unwrap_or(usize::MAX);
        previous_newline = matches!(ch, '\r' | '\n');
        previous_cr = ch == '\r';
    }
    bytes
}
