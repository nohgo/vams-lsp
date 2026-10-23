use tower_lsp::lsp_types::{Position, Range};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf16,
}

pub fn position(text: &str, byte: usize, encoding: Encoding) -> Position {
    let mut byte = byte.min(text.len());
    while !text.is_char_boundary(byte) {
        byte -= 1;
    }
    let before = &text[..byte];
    let line = before.bytes().filter(|b| *b == b'\n').count();
    let start = before.rfind('\n').map_or(0, |p| p + 1);
    let chars = match encoding {
        Encoding::Utf8 => byte - start,
        Encoding::Utf16 => text[start..byte].encode_utf16().count(),
    };
    Position::new(line as u32, chars as u32)
}

pub fn offset(text: &str, pos: Position, encoding: Encoding) -> Option<usize> {
    let start = if pos.line == 0 {
        0
    } else {
        text.match_indices('\n').nth(pos.line as usize - 1)?.0 + 1
    };
    let end = text[start..].find('\n').map_or(text.len(), |n| start + n);
    let line = text[start..end]
        .strip_suffix('\r')
        .unwrap_or(&text[start..end]);
    if encoding == Encoding::Utf8 {
        let n = pos.character as usize;
        return (n <= line.len() && line.is_char_boundary(n)).then_some(start + n);
    }
    let mut units = 0;
    for (byte, ch) in line.char_indices() {
        if units == pos.character {
            return Some(start + byte);
        }
        units += ch.len_utf16() as u32;
        if units > pos.character {
            return None;
        }
    }
    (units == pos.character).then_some(start + line.len())
}

pub fn range(text: &str, span: std::ops::Range<usize>, encoding: Encoding) -> Range {
    Range::new(
        position(text, span.start, encoding),
        position(text, span.end, encoding),
    )
}
