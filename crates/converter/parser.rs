enum Block {
    Heading { level: u8, content: Vec<Inline> },
    Paragraph(Vec<Inline>),
    Quote { depth: u8 },
    Hr,
    Macro(Macro),
}

enum Inline {
    Text(Span),
    Bold(Vec<Inline>),
    Link { target: Span, label: Vec<Inline> },
}
