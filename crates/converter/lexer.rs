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

}
