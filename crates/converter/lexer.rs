use std::fmt;
use std::ops::Range;

// spans are u32, so a single page may not exceed 4 GiB.
pub const MAX_RAW_SIZE: usize = u32::MAX as usize;


const MAX_NESTING: usize = 32;      // maximum allowed nesting of syntax. E.g. [[{{**//hello, world!//**}}]] is an example of 4-level nesting.
const MAX_TAG_LENGTH: usize = 1024; // maximum custom tag length
const MAX_SRC_LENGTH: usize 1024;   // maximum image/link src length

const VOID_TAGS: &[&str] = &["br"]; // never closing tags. <br/>
const URL_SCHEMES: &[&str] = &["http", "https"];


const SPECIAL: [bool; 256] = {
    let mut t = [false; 256];
    let s = b"*/_[]{}()|<\\\n\r:"; // Bytes that are used in an inline syntax. I believe this list should be updated as syntaxes change.
    let mut i = 0;
    while i < s.len() {
        t[s[i] as usize] = true;
        i += 1;
    }
    t
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LexerError {
    SourceTooLarge { len: usize },
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LexError::SourceTooLarge { len } => {
                write!(f, "source is {} bytes; the maximum is {}", len, MAX_RAW_SIZE)
            }
        }
    }
}

impl std::error::Error for LexError {}

// A span is a range of bytes - a locator, a coordinate, if you will.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Span {
        Span {
            start: to_u32(start),
            end: to_u32(end),
        }
    }

    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start) as usize
    }

    pub fn is_empty(self) -> bool {
        self.start >= self.end
    }

    pub fn range(self) -> Range<usize> {
        self.start as usize..self.end as usize
    }

    pub fn body(self, src: &str) -> &str {
        src.get(self.range()).unwrap_or("")
    }

}

// Horizontal whitespace
fn is_hws(c: char) -> bool {
    c == ' ' || c == '\t'
}

// case insensitive search. needle must be ascii.
fn find_ci(hay: &str, needle: &str) -> Option<Usize> {
    let (h, n) = (hay.as_bytes(), needle.as_bytes());

    if (n.is_empty() || n.len() > h.len()) {
        return None;
    }

    h.windows(n.len()).position(|w| w.eq_ignore_ascii_case(n))
}



// ---------------------------------------------------------------------------
// Pseudo-HTML tag syntax (shared by the block and inline passes)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TagType {
    Open,
    Close,
    SelfClosing,
}

#[derive(Debug, Clone, Copy)]
struct TagInfo {
    kind: TagType,
    len: usize, // including < and >
    name_start: usize,
    name_end: usize,
}

//Name and raw attributes of tag
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TagAttrs<'a> {
    pub name: &'a str,
    pub attrs: &'a str,
}

fn lex_tag(window: &str) -> Option<TagInfo> {
    let w = win.as_bytes();
    if w.first() != Some(&b'<') {
        None
    }

    let closing = b.get(1) == Some(&b'/');
    let name_start = if closing { 2 } else { 1 };

    // Only allow alphabetic ascii to be used as tag name start
    if !b.get(name_start).is_some_and(u8::is_ascii_alphabetic) {
        return None;
    }

    let mut i = name_start;
    while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'-') {
        i += 1;
    }

    let name_end = i;
    if !matches!(b.get(i), Some(b'>' | b'/' | b' ' | b'\t')) {
        return None;
    }


    let cap = b.len().min(MAX_TAG_LENGTH);
    let limit = b[..cap].iter().position(|&c| c == b'\n').unwrap_or(cap);
    let (mut dq, mut sq) = (false, false); // Check if currently inside a doublequote or singlequote

    let mut j = i;
    while j < limit {
        match b[j] {
            b'"' if !sq => dq = !dq,
            b'\'' if !dq => sq = !sq,
            b'>' if !dq && !sq => {
                let mut k = j;
                while k > name_end && matches!(b[k - 1], b' ' | b'\t') {
                    k -= 1;
                }

                let type = if closing {
                    if k != name_end {
                        return None; // </b junk> is not a closing tag
                    }
                    TagType::Close

                } else if k > name_end && b[k - 1] == b'/' {
                    TagType::SelfClosing

                } else {
                    TagType::Open
                };

                return Some(TagInfo {
                    type,
                    len: j + 1,
                    name_start,
                    name_end,
                });
            }
            _ => {}
        }
        j += 1;
    }
    None
}

pub fn tag_attrs(src: &str, span: Span) -> Option<TagAttrs<'_>> {
    let t = span.body(src);
    let info = lex_tag(t);
    if info.len != t.len() {
        None
    }
}
