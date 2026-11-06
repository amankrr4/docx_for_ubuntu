use std::sync::Arc;

#[derive(Clone, Debug, Default)]
pub struct Document {
    pub blocks: Vec<Block>,
    pub headers: Vec<HeaderFooter>,
    pub footers: Vec<HeaderFooter>,
    pub warnings: Vec<Warning>,
    pub truncated: bool,
    pub images: Vec<Arc<ImageData>>,
    pub stats: Stats,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub nodes: u64,
    pub xml_bytes: u64,
    pub images: u32,
    pub table_cells: u64,
}

#[derive(Clone, Debug)]
pub struct HeaderFooter {
    pub kind: HeaderFooterKind,
    pub part_name: String,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderFooterKind {
    Default,
    First,
    Even,
}

impl HeaderFooterKind {
    pub fn header_label(self) -> &'static str {
        match self {
            Self::Default => "Header",
            Self::First => "First-page header",
            Self::Even => "Even-page header",
        }
    }

    pub fn footer_label(self) -> &'static str {
        match self {
            Self::Default => "Footer",
            Self::First => "First-page footer",
            Self::Even => "Even-page footer",
        }
    }
}

#[derive(Clone, Debug)]
pub enum Block {
    Paragraph(Paragraph),
    Table(Table),
    PageBreak,
}

#[derive(Clone, Debug, Default)]
pub struct Paragraph {
    pub props: ParaProps,
    pub list: Option<ListLabel>,
    pub inlines: Vec<Inline>,
}

#[derive(Clone, Debug, Default)]
pub struct ParaProps {
    pub heading_level: Option<u8>,
    pub align: Align,
    pub indent_left_tw: i32,
    pub indent_right_tw: i32,
    pub indent_first_tw: i32,
    pub space_before_tw: u32,
    pub space_after_tw: u32,
    pub line_height: Option<f32>,
    pub rtl: bool,
    pub shading: Option<Rgb>,
    pub num_id: Option<String>,
    pub num_level: u8,
    pub style_id: Option<String>,
    pub tab_stops: Vec<TabStop>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TabStop {
    pub align: TabAlign,
    pub pos_tw: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TabAlign {
    #[default]
    Left,
    Center,
    Right,
    Decimal,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
    Justify,
}

#[derive(Clone, Debug, Default)]
pub struct ListLabel {
    pub text: String,
    pub level: u8,
}

#[derive(Clone, Debug)]
pub enum Inline {
    Text {
        text: String,
        props: RunProps,
    },
    Tab,
    LineBreak,
    PageBreak,
    Image(ImageRef),
    Link {
        target: LinkTarget,
        children: Vec<Inline>,
    },
    FootnoteRef(u32),
    Placeholder(String),
    Bookmark(String),
    TextBox(Vec<Block>),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunProps {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub small_caps: bool,
    pub all_caps: bool,
    pub vert: VertAlign,
    pub size_half_pt: Option<u16>,
    pub color: Option<Rgb>,
    pub highlight: Option<Rgb>,
    pub font: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum VertAlign {
    #[default]
    Baseline,
    Super,
    Sub,
}

#[derive(Clone, Debug)]
pub enum LinkTarget {
    External(String),
    Anchor(String),
}

#[derive(Clone, Debug)]
pub struct ImageRef {
    pub index: usize,
    pub width_px: u32,
    pub height_px: u32,
    pub alt: String,
}

#[derive(Clone, Debug)]
pub struct ImageData {
    pub bytes: Arc<[u8]>,
    pub mime: String,
    pub alt: String,
}

#[derive(Clone, Debug, Default)]
pub struct Table {
    pub rows: Vec<Row>,
    pub col_widths_tw: Vec<u32>,
    pub borders: BorderStyle,
}

#[derive(Clone, Debug, Default)]
pub struct Row {
    pub cells: Vec<Cell>,
}

#[derive(Clone, Debug, Default)]
pub struct Cell {
    pub blocks: Vec<Block>,
    pub col_span: u16,
    pub v_merge: VMerge,
    pub shading: Option<Rgb>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VMerge {
    #[default]
    None,
    Restart,
    Continue,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BorderStyle {
    pub top: bool,
    pub bottom: bool,
    pub left: bool,
    pub right: bool,
}

impl BorderStyle {
    pub fn any(self) -> bool {
        self.top || self.bottom || self.left || self.right
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub fn is_dark(self) -> bool {
        let luminance = (0.2126 * f32::from(self.r))
            + (0.7152 * f32::from(self.g))
            + (0.0722 * f32::from(self.b));
        luminance < 96.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    pub kind: WarningKind,
    pub count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WarningKind {
    UnsupportedElement,
    SkippedImage,
    HeaderFooterVariants,
    Footnotes,
    Comments,
    Truncated,
    Other,
}

impl WarningKind {
    pub fn message(self) -> &'static str {
        match self {
            Self::UnsupportedElement => "Unsupported element",
            Self::SkippedImage => "Image skipped",
            Self::HeaderFooterVariants => "Multiple header/footer variants",
            Self::Footnotes => "Footnotes are not rendered",
            Self::Comments => "Comments are not rendered",
            Self::Truncated => "Document truncated",
            Self::Other => "Other warning",
        }
    }
}

impl Document {
    pub fn add_warning(&mut self, kind: WarningKind) {
        if let Some(existing) = self
            .warnings
            .iter_mut()
            .find(|warning| warning.kind == kind)
        {
            existing.count = existing.count.saturating_add(1);
        } else {
            self.warnings.push(Warning { kind, count: 1 });
        }
    }

    pub fn warning_count(&self) -> u32 {
        self.warnings
            .iter()
            .fold(0, |total, warning| total.saturating_add(warning.count))
    }

    pub fn dump_text(&self) -> String {
        let mut out = String::new();
        for header in &self.headers {
            out.push('[');
            out.push_str(header.kind.header_label());
            out.push_str("]\n");
            dump_blocks(&header.blocks, &mut out, 0);
            out.push('\n');
        }
        dump_blocks(&self.blocks, &mut out, 0);
        for footer in &self.footers {
            out.push('\n');
            out.push('[');
            out.push_str(footer.kind.footer_label());
            out.push_str("]\n");
            dump_blocks(&footer.blocks, &mut out, 0);
        }
        out
    }
}

fn dump_blocks(blocks: &[Block], out: &mut String, depth: u32) {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => {
                for _ in 0..depth {
                    out.push_str("  ");
                }
                if let Some(label) = &paragraph.list {
                    out.push_str(&label.text);
                    out.push('\t');
                }
                dump_inlines(&paragraph.inlines, out);
                out.push('\n');
            }
            Block::Table(table) => {
                for row in &table.rows {
                    for (index, cell) in row.cells.iter().enumerate() {
                        if index > 0 {
                            out.push('\t');
                        }
                        dump_blocks(&cell.blocks, out, depth.saturating_add(1));
                    }
                    out.push('\n');
                }
            }
            Block::PageBreak => {
                out.push_str("────────────────\n");
            }
        }
    }
}

fn dump_inlines(inlines: &[Inline], out: &mut String) {
    for inline in inlines {
        match inline {
            Inline::Text { text, .. } => out.push_str(text),
            Inline::Tab => out.push('\t'),
            Inline::LineBreak => out.push('\n'),
            Inline::PageBreak => out.push_str("\n────────────────\n"),
            Inline::Image(image) => {
                if image.alt.is_empty() {
                    out.push_str("[Image]");
                } else {
                    out.push_str("[Image: ");
                    out.push_str(&image.alt);
                    out.push(']');
                }
            }
            Inline::Link { children, .. } => dump_inlines(children, out),
            Inline::FootnoteRef(number) => {
                out.push('[');
                out.push_str(&number.to_string());
                out.push(']');
            }
            Inline::Placeholder(text) => out.push_str(text),
            Inline::Bookmark(_) => {}
            Inline::TextBox(blocks) => {
                out.push_str("[Text box] ");
                dump_blocks(blocks, out, 0);
            }
        }
    }
}
