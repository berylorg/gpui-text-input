use crate::TextInputMode;

pub(crate) fn normalize_text(text: &str, mode: TextInputMode) -> String {
    match mode {
        TextInputMode::SingleLine => normalize_single_line_text(text),
        TextInputMode::Multiline => normalize_multiline_text(text),
    }
}

fn normalize_single_line_text(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut pending_newline_spacing = false;

    for ch in text.chars() {
        match ch {
            '\r' | '\n' => {
                if !pending_newline_spacing {
                    normalized.push(' ');
                    pending_newline_spacing = true;
                }
            }
            _ => {
                normalized.push(ch);
                pending_newline_spacing = false;
            }
        }
    }

    normalized
}

fn normalize_multiline_text(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            normalized.push('\n');
        } else {
            normalized.push(ch);
        }
    }

    normalized
}
