use std::fmt;

// spans are u32, so a single page may not exceed 4 GiB.
pub const MAX_RAW_SIZE: usize = u32::MAX as usize;

const MAX_NESTING: usize = 32; // maximum allowed nesting of syntax. E.g. [[{{**//hello, world!//**}}]] is an example of 4-level nesting.
const MAX_TAG_LENGTH: usize = 1024; // maximum custom tag length
const MAX_SRC_LENGTH: usize = 1024; // maximum image/link src length

const RAW_TAG: &str = "nowiki"; // tag whose body is raw text
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

impl fmt::Display for LexerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LexerError::SourceTooLarge { len } => {
                write!(
                    f,
                    "source is {} bytes; the maximum is {}",
                    len, MAX_RAW_SIZE
                )
            }
        }
    }
}

impl std::error::Error for LexerError {}

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

    pub fn range(self) -> std::ops::Range<usize> {
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

fn to_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

// ---------------------------------------------------------------------------
// Pseudo-HTML tag syntax (shared by the block and inline passes)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TagKind {
    Open,
    Close,
    SelfClosing,
}

#[derive(Debug, Clone, Copy)]
struct TagInfo {
    kind: TagKind,
    len: usize, // including < and >
    name_start: usize,
    name_end: usize,
}

//Name and raw attributes of tag
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TagParts<'a> {
    pub name: &'a str,
    pub attrs: &'a str,
}

fn lex_tag(window: &str) -> Option<TagInfo> {
    let w = window.as_bytes();
    if w.first() != Some(&b'<') {
        return None;
    }

    let closing = w.get(1) == Some(&b'/');
    let name_start = if closing { 2 } else { 1 };

    // Only allow alphabetic ascii to be used as tag name start
    if !w.get(name_start).is_some_and(u8::is_ascii_alphabetic) {
        return None;
    }

    let mut i = name_start;
    while i < w.len() && (w[i].is_ascii_alphanumeric() || w[i] == b'-') {
        i += 1;
    }

    let name_end = i;
    if !matches!(w.get(i), Some(b'>' | b'/' | b' ' | b'\t')) {
        return None;
    }

    let cap = w.len().min(MAX_TAG_LENGTH);
    let limit = w[..cap].iter().position(|&c| c == b'\n').unwrap_or(cap);
    let (mut dq, mut sq) = (false, false); // Check if currently inside a doublequote or singlequote

    let mut j = i;
    while j < limit {
        match w[j] {
            b'"' if !sq => dq = !dq,
            b'\'' if !dq => sq = !sq,
            b'>' if !dq && !sq => {
                let mut k = j;
                while k > name_end && matches!(w[k - 1], b' ' | b'\t') {
                    k -= 1;
                }

                let kind = if closing {
                    if k != name_end {
                        return None; // </b junk> is not a closing tag
                    }
                    TagKind::Close
                } else if k > name_end && w[k - 1] == b'/' {
                    TagKind::SelfClosing
                } else {
                    TagKind::Open
                };

                return Some(TagInfo {
                    kind,
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

/// Find the closing `</nowiki>` tag at or after `from`.
/// Returns the offset just past the closing tag.
fn find_raw_close(src: &str, from: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    let mut pos = from;
    while let Some(rel) = bytes[pos..].iter().position(|&b| b == b'<') {
        let lt = pos + rel;
        let Some(tag) = lex_tag(&src[lt..]) else {
            pos = lt + 1;
            continue;
        };
        let name = &src[lt + tag.name_start..lt + tag.name_end];
        if matches!(tag.kind, TagKind::Close) && name.eq_ignore_ascii_case(RAW_TAG) {
            return Some(lt + tag.len);
        }
        pos = lt + tag.len;
    }
    None
}

pub fn tag_parts(src: &str, span: Span) -> Option<TagParts<'_>> {
    let t = span.body(src);
    let Some(info) = lex_tag(t) else {
        return None;
    };
    if info.len != t.len() {
        return None;
    }

    let name = &t[info.name_start..info.name_end];
    let mut attrs = t[info.name_end..t.len() - 1].trim();
    if info.kind == TagKind::SelfClosing {
        attrs = attrs.strip_suffix('/').unwrap_or(attrs).trim_end();
    }
    Some(TagParts { name, attrs })
}

// ---------------------------------------------------------------------------
// Block pass
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Blank,
    Paragraph,
    Heading { level: u8 },
    Hr,
    Quote { depth: u8 },
    List { depth: u8, ordered: bool },
    Macro,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockToken {
    pub kind: BlockKind,
    pub span: Span,    // The whole block without surrounding indentation and newline
    pub content: Span, // to feed to lex_inline
}

impl BlockToken {
    pub fn body<'a>(&self, src: &'a str) -> &'a str {
        self.content.body(src)
    }
}

struct BlockLexer<'a> {
    src: &'a str,
    out: Vec<BlockToken>,
    range: Option<Range>,
    raw_until: usize,
    raw_missing_from: usize, // No </nowiki> exists at or after this offset (avoids rescanning).
}

enum Line {
    Blank,
    Block(BlockToken),
    Text { start: usize, end: usize },
}

struct Range {
    start: usize,
    end: usize,
}

pub fn lex_blocks(src: &str) -> Result<Vec<BlockToken>, LexerError> {
    if src.len() > MAX_RAW_SIZE {
        return Err(LexerError::SourceTooLarge { len: src.len() });
    }

    Ok(BlockLexer {
        src,
        out: Vec::new(),
        range: None,
        raw_until: 0,
        raw_missing_from: usize::MAX,
    }
    .run())
}

impl<'a> BlockLexer<'a> {
    fn run(mut self) -> Vec<BlockToken> {
        let src = self.src;
        let bytes = src.as_bytes();
        let mut pos = 0;
        while pos < bytes.len() {
            let (raw_end, next) = match bytes[pos..].iter().position(|&b| b == b'\n') {
                Some(off) => (pos + off, pos + off + 1),
                None => (bytes.len(), bytes.len()),
            };
            let mut end = raw_end;
            if end > pos && bytes[end - 1] == b'\r' {
                end -= 1;
            }
            self.line(pos, end);
            pos = next;
        }
        self.flush_range();
        self.out
    }

    fn line(&mut self, pos: usize, end: usize) {
        let src = self.src;

        if pos < self.raw_until {
            if let Some(p) = self.range.as_mut() {
                let t = src[pos..end].trim_end_matches(is_hws);
                if !t.is_empty() {
                    p.end = pos + t.len();
                }
            }
            if self.raw_until <= end {
                let from = self.raw_until;
                if let Some(e) = self.scan_raw(from, end) {
                    self.raw_until = e;
                }
            }
            return;
        }

        match classify(src, pos, end) {
            Line::Blank => {
                self.flush_range();
                let extend = matches!(self.out.last(), Some(t) if t.kind == BlockKind::Blank);
                if extend {
                    if let Some(t) = self.out.last_mut() {
                        t.span = Span::new(t.span.start as usize, end);
                    }
                } else {
                    self.out.push(BlockToken {
                        kind: BlockKind::Blank,
                        span: Span::new(pos, end),
                        content: Span::new(pos, pos),
                    });
                }
            }
            Line::Block(tok) => {
                self.flush_range();
                self.out.push(tok);
            }
            Line::Text { start, end: tend } => {
                if let Some(p) = self.range.as_mut() {
                    p.end = tend;
                } else {
                    self.range = Some(Range { start, end: tend });
                }
                if let Some(e) = self.scan_raw(start, tend) {
                    self.raw_until = e;
                }
            }
        }
    }

    fn flush_range(&mut self) {
        if let Some(p) = self.range.take() {
            let span = Span::new(p.start, p.end);
            self.out.push(BlockToken {
                kind: BlockKind::Paragraph,
                span,
                content: span,
            });
        }
    }

    /// Look for a `<nowiki>` opened in `src[from..line_end]` whose closing tag
    /// lies beyond `line_end`. Returns the offset just past `</nowiki>`.
    fn scan_raw(&mut self, mut from: usize, line_end: usize) -> Option<usize> {
        let src = self.src;
        let bytes = src.as_bytes();
        while from < line_end {
            let lt = from + bytes[from..line_end].iter().position(|&b| b == b'<')?;
            let Some(tag) = lex_tag(&src[lt..line_end]) else {
                from = lt + 1;
                continue;
            };
            from = lt + tag.len;
            let name = &src[lt + tag.name_start..lt + tag.name_end];
            if tag.kind != TagKind::Open || !name.eq_ignore_ascii_case(RAW_TAG) {
                continue;
            }
            if from >= self.raw_missing_from {
                continue;
            }
            match find_raw_close(src, from) {
                Some(end) => {
                    if end > line_end {
                        return Some(end);
                    }
                    from = end;
                }
                None => self.raw_missing_from = self.raw_missing_from.min(from),
            }
        }
        None
    }
}

/// Classify one line (`src[pos..end]`, no line terminator).
fn classify(src: &str, pos: usize, end: usize) -> Line {
    let line = &src[pos..end];
    let rest = line.trim_start_matches(is_hws);
    let indent = line.len() - rest.len();
    let body = rest.trim_end_matches(is_hws);
    if body.is_empty() {
        return Line::Blank;
    }
    let bs = pos + indent;
    let be = bs + body.len();
    let span = Span::new(bs, be);
    let first = body.as_bytes()[0];
    let block = |kind: BlockKind, cs: usize, ce: usize| {
        Line::Block(BlockToken {
            kind,
            span,
            content: Span::new(cs, ce),
        })
    };

    if first == b'=' {
        if let Some((level, cs, ce)) = heading(body) {
            return block(BlockKind::Heading { level }, bs + cs, bs + ce);
        }
    }

    if body.len() >= 4 && body.bytes().all(|b| b == b'-') {
        return block(BlockKind::Hr, be, be);
    }

    if first == b'~' {
        if let Some((cs, ce)) = macro_inner(body) {
            return block(BlockKind::Macro, bs + cs, bs + ce);
        }
    }

    if first == b'>' {
        let run = body.bytes().take_while(|&b| b == b'>').count();
        let after = &body[run..];
        let lead = after.len() - after.trim_start_matches(is_hws).len();
        let depth = run.min(255) as u8;
        return block(BlockKind::Quote { depth }, bs + run + lead, be);
    }

    if first == b'*' || first == b'-' {
        let cols: usize = line[..indent]
            .bytes()
            .map(|b| if b == b'\t' { 2 } else { 1 })
            .sum();
        let marker_ok = body.len() > 1 && matches!(body.as_bytes()[1], b' ' | b'\t');
        if cols >= 2 && marker_ok {
            let after = &body[1..];
            let lead = after.len() - after.trim_start_matches(is_hws).len();
            let depth = (cols / 2).min(255) as u8;
            return block(
                BlockKind::List {
                    depth,
                    ordered: first == b'-',
                },
                bs + 1 + lead,
                be,
            );
        }
    }

    Line::Text { start: bs, end: be }
}

/// `== text ==` -> `(level, content_start, content_end)` relative to `body`.
/// Needs 2+ `=` on both sides; mismatched runs use the smaller one.
/// A heading with no closing run (or no text) is not a heading.
fn heading(body: &str) -> Option<(u8, usize, usize)> {
    let open = body.bytes().take_while(|&b| b == b'=').count();
    if open < 2 {
        return None;
    }
    let rest = &body[open..];
    let close = rest.bytes().rev().take_while(|&b| b == b'=').count();
    if close < 2 {
        return None;
    }
    let inner_all = &rest[..rest.len() - close];
    let inner = inner_all.trim_matches(is_hws);
    if inner.is_empty() {
        return None;
    }
    let lead = inner_all.len() - inner_all.trim_start_matches(is_hws).len();
    let start = open + lead;
    let level = 7 - open.min(close).clamp(2, 6);
    Some((level as u8, start, start + inner.len()))
}

/// `~~NAME~~` filling the whole (trimmed) line -> range of `NAME` in `body`.
fn macro_inner(body: &str) -> Option<(usize, usize)> {
    let rest = body.strip_prefix("~~")?;
    let q = rest.find("~~")?;
    if q == 0 || q + 2 != rest.len() {
        return None;
    }
    Some((2, 2 + q))
}
