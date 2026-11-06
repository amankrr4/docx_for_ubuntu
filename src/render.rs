use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use gtk::prelude::*;
use gtk::{gdk, gdk_pixbuf, glib, pango};

use crate::fonts::resolve_font;
use crate::limits::{
    MAX_IMAGE_DIMENSION, MAX_IMAGE_PIXELS, MAX_RENDERED_IMAGES, MAX_TABULAR_CELL_CHARS,
};
use crate::model::{
    Align, Block, Document, HeaderFooter, ImageRef, Inline, LinkTarget, ParaProps, Paragraph, Rgb,
    RunProps, Table, VertAlign,
};
use crate::theme::AppTheme;
use crate::xmlutil::{twips_to_px_i32, twips_to_px_u32};

#[derive(Default)]
pub struct RenderOutput {
    pub link_targets: Vec<LinkTarget>,
    pub bookmarks: HashMap<String, gtk::TextMark>,
}

pub fn render(
    view: gtk::TextView,
    document: Arc<Document>,
    zoom: f64,
    theme: AppTheme,
    output: Rc<RefCell<RenderOutput>>,
    on_done: impl Fn() + 'static,
) -> Arc<AtomicBool> {
    let cancel = Arc::new(AtomicBool::new(false));
    view.remove_css_class("docx-paper");
    let buffer = view.buffer();
    buffer.set_text("");
    let state = Rc::new(RefCell::new(RenderState {
        buffer,
        view,
        document,
        zoom,
        theme,
        run_tags: HashMap::new(),
        para_tags: HashMap::new(),
        link_tags: HashMap::new(),
        shade_tags: HashMap::new(),
        next_tag: 0,
        output,
        image_cache: HashMap::new(),
        rendered_images: 0,
        phase: Phase::Headers,
        header_index: 0,
        block_index: 0,
        footer_index: 0,
        done: false,
        on_done: Some(Box::new(on_done)),
    }));
    let state_clone = state.clone();
    let cancel_clone = cancel.clone();
    glib::idle_add_local(move || {
        let Ok(mut state) = state_clone.try_borrow_mut() else {
            return glib::ControlFlow::Break;
        };
        if cancel_clone.load(Ordering::Relaxed) {
            state.finish();
            return glib::ControlFlow::Break;
        }
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(8) {
            if !state.step() {
                state.finish();
                return glib::ControlFlow::Break;
            }
        }
        glib::ControlFlow::Continue
    });
    cancel
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Headers,
    Body,
    Footers,
    Done,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct RunKey {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    small_caps: bool,
    all_caps: bool,
    vert: VertAlign,
    size_half_pt: Option<u16>,
    color: Option<Rgb>,
    highlight: Option<Rgb>,
    font: Option<String>,
    muted: bool,
}

impl From<&RunProps> for RunKey {
    fn from(value: &RunProps) -> Self {
        Self {
            bold: value.bold,
            italic: value.italic,
            underline: value.underline,
            strike: value.strike,
            small_caps: value.small_caps,
            all_caps: value.all_caps,
            vert: value.vert,
            size_half_pt: value.size_half_pt,
            color: value.color,
            highlight: value.highlight,
            font: value.font.clone(),
            muted: false,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct ParaKey {
    heading: Option<u8>,
    align: Align,
    left: i32,
    right: i32,
    first: i32,
    before: u32,
    after: u32,
    line_height: Option<u32>,
    rtl: bool,
    muted: bool,
    list: bool,
    extra: i32,
    shading: Option<Rgb>,
    tab_stops: Vec<(u8, i32)>,
}

struct RenderState {
    buffer: gtk::TextBuffer,
    view: gtk::TextView,
    document: Arc<Document>,
    zoom: f64,
    theme: AppTheme,
    run_tags: HashMap<RunKey, gtk::TextTag>,
    para_tags: HashMap<ParaKey, gtk::TextTag>,
    link_tags: HashMap<usize, gtk::TextTag>,
    shade_tags: HashMap<Rgb, gtk::TextTag>,
    next_tag: u32,
    output: Rc<RefCell<RenderOutput>>,
    image_cache: HashMap<usize, gdk::Texture>,
    rendered_images: u32,
    phase: Phase,
    header_index: usize,
    block_index: usize,
    footer_index: usize,
    done: bool,
    on_done: Option<Box<dyn Fn()>>,
}

impl RenderState {
    fn step(&mut self) -> bool {
        if self.done {
            return false;
        }
        match self.phase {
            Phase::Headers => {
                if let Some(header) = self.document.headers.get(self.header_index).cloned() {
                    self.render_header_footer(&header, true);
                    self.header_index = self.header_index.saturating_add(1);
                } else {
                    self.phase = Phase::Body;
                    self.block_index = 0;
                }
            }
            Phase::Body => {
                if let Some(block) = self.document.blocks.get(self.block_index).cloned() {
                    self.render_block(&block, 0, false);
                    self.block_index = self.block_index.saturating_add(1);
                } else {
                    self.phase = Phase::Footers;
                    self.footer_index = 0;
                }
            }
            Phase::Footers => {
                if let Some(footer) = self.document.footers.get(self.footer_index).cloned() {
                    self.render_header_footer(&footer, false);
                    self.footer_index = self.footer_index.saturating_add(1);
                } else {
                    self.phase = Phase::Done;
                }
            }
            Phase::Done => {
                self.done = true;
                return false;
            }
        }
        true
    }

    fn finish(&mut self) {
        if let Some(callback) = self.on_done.take() {
            callback();
        }
    }

    fn render_header_footer(&mut self, item: &HeaderFooter, header: bool) {
        if !header {
            self.insert_rule();
        }
        self.insert_text(
            &format!(
                "{}\n",
                if header {
                    item.kind.header_label()
                } else {
                    item.kind.footer_label()
                }
            ),
            &RunProps {
                italic: true,
                ..RunProps::default()
            },
            None,
            true,
        );
        for block in &item.blocks {
            self.render_block(block, 0, true);
        }
        if header {
            self.insert_rule();
        }
    }

    fn render_block(&mut self, block: &Block, extra: i32, muted: bool) {
        match block {
            Block::Paragraph(paragraph) => self.render_paragraph(paragraph, extra, muted),
            Block::Table(table) => self.render_table(table, extra, muted),
            Block::PageBreak => self.insert_rule(),
        }
    }

    fn render_paragraph(&mut self, paragraph: &Paragraph, extra: i32, muted: bool) {
        let start_mark = self.buffer.create_mark(None, &self.buffer.end_iter(), true);
        let mut base_run = paragraph_run(&paragraph.props);
        if muted {
            base_run.color = None;
        }
        if let Some(list) = &paragraph.list {
            let label_run = base_run.clone();
            self.insert_text(&list.text, &label_run, None, muted);
            self.insert_text("\t", &label_run, None, muted);
        }
        for inline in &paragraph.inlines {
            self.render_inline(inline, &base_run, None, extra, muted);
        }
        if !inlines_end_with_break(&paragraph.inlines) {
            self.insert_text("\n", &base_run, None, muted);
        }
        let start = self.buffer.iter_at_mark(&start_mark);
        let end = self.buffer.end_iter();
        let tag = self.para_tag(&paragraph.props, extra, muted, paragraph.list.is_some());
        self.buffer.apply_tag(&tag, &start, &end);
    }

    fn render_inline(
        &mut self,
        inline: &Inline,
        base: &RunProps,
        link: Option<usize>,
        extra: i32,
        muted: bool,
    ) {
        match inline {
            Inline::Text { text, props } => {
                self.insert_text(text, props, link, muted);
            }
            Inline::Tab => self.insert_text("\t", base, link, muted),
            Inline::LineBreak => self.insert_text("\n", base, link, muted),
            Inline::PageBreak => self.insert_rule(),
            Inline::Image(image) => self.render_image(image),
            Inline::Link { target, children } => {
                let index = {
                    let mut output = self.output.borrow_mut();
                    let index = output.link_targets.len();
                    output.link_targets.push(target.clone());
                    index
                };
                for child in children {
                    self.render_inline(child, base, Some(index), extra, muted);
                }
            }
            Inline::FootnoteRef(number) => {
                self.insert_text(&format!("[{number}]"), base, link, true);
            }
            Inline::Placeholder(text) => {
                self.insert_text(text, &placeholder_run(), link, true);
            }
            Inline::Bookmark(name) => {
                let mark = self
                    .buffer
                    .create_mark(Some(name), &self.buffer.end_iter(), true);
                self.output
                    .borrow_mut()
                    .bookmarks
                    .insert(name.clone(), mark);
            }
            Inline::TextBox(blocks) => {
                self.insert_text("[Text box]\n", &placeholder_run(), link, true);
                for block in blocks {
                    self.render_block(block, extra.saturating_add(24), muted);
                }
            }
        }
    }

    fn insert_text(&mut self, text: &str, run: &RunProps, link: Option<usize>, muted: bool) {
        if text.is_empty() {
            return;
        }
        let run_tag = self.run_tag(run, muted);
        let mut tags: Vec<&gtk::TextTag> = vec![&run_tag];
        let link_tag;
        if let Some(index) = link {
            link_tag = self.link_tag(index);
            tags.push(&link_tag);
        }
        let mut end = self.buffer.end_iter();
        self.buffer.insert_with_tags(&mut end, text, &tags);
    }

    fn insert_rule(&mut self) {
        self.insert_text(
            "────────────────────────────\n",
            &RunProps::default(),
            None,
            true,
        );
    }

    fn run_tag(&mut self, run: &RunProps, muted: bool) -> gtk::TextTag {
        let mut key = RunKey::from(run);
        key.muted = muted;
        if let Some(tag) = self.run_tags.get(&key) {
            return tag.clone();
        }
        let name = self.new_tag_name();
        let tag = gtk::TextTag::new(Some(&name));
        if run.bold {
            tag.set_weight(700);
        }
        if run.italic {
            tag.set_style(pango::Style::Italic);
        }
        if run.underline {
            tag.set_underline(pango::Underline::Single);
        }
        if run.strike {
            tag.set_strikethrough(true);
        }
        if run.small_caps {
            tag.set_variant(pango::Variant::SmallCaps);
        }
        if run.all_caps {
            tag.set_text_transform(pango::TextTransform::Uppercase);
        }
        let base_points = run
            .size_half_pt
            .map(|size| f64::from(size) / 2.0)
            .unwrap_or(11.0);
        tag.set_size_points(base_points * self.zoom);
        if let Some(color) = run.color {
            tag.set_foreground_rgba(Some(&rgba(color)));
        }
        if let Some(highlight) = run.highlight {
            tag.set_background_rgba(Some(&rgba(highlight)));
        }
        if let Some(font) = run
            .font
            .as_deref()
            .and_then(|font| resolve_font(Some(font)))
        {
            tag.set_family(Some(&font));
        }
        match run.vert {
            VertAlign::Super => {
                tag.set_rise(5 * 1024);
                tag.set_scale(0.8);
            }
            VertAlign::Sub => {
                tag.set_rise(-3 * 1024);
                tag.set_scale(0.8);
            }
            VertAlign::Baseline => {}
        }
        if muted {
            tag.set_foreground_rgba(Some(&self.theme.muted));
        }
        self.buffer.tag_table().add(&tag);
        self.run_tags.insert(key, tag.clone());
        tag
    }

    fn para_tag(&mut self, props: &ParaProps, extra: i32, muted: bool, list: bool) -> gtk::TextTag {
        let line_height = props
            .line_height
            .map(|value| (value * 1000.0).round() as u32);
        let key = ParaKey {
            heading: props.heading_level,
            align: props.align,
            left: props.indent_left_tw,
            right: props.indent_right_tw,
            first: props.indent_first_tw,
            before: props.space_before_tw,
            after: props.space_after_tw,
            line_height,
            rtl: props.rtl,
            muted,
            list,
            extra,
            shading: props.shading,
            tab_stops: props
                .tab_stops
                .iter()
                .map(|stop| {
                    let align = match stop.align {
                        crate::model::TabAlign::Left => 0,
                        crate::model::TabAlign::Center => 1,
                        crate::model::TabAlign::Right => 2,
                        crate::model::TabAlign::Decimal => 3,
                    };
                    (align, stop.pos_tw)
                })
                .collect(),
        };
        if let Some(tag) = self.para_tags.get(&key) {
            return tag.clone();
        }
        let name = self.new_tag_name();
        let tag = gtk::TextTag::new(Some(&name));
        tag.set_justification(match props.align {
            Align::Center => gtk::Justification::Center,
            Align::End => gtk::Justification::Right,
            Align::Justify => gtk::Justification::Fill,
            Align::Start => gtk::Justification::Left,
        });
        let hanging = if list { 24 } else { 0 };
        let indent_left = twips_to_px_i32(props.indent_left_tw, self.zoom);
        let left_extra = extra.saturating_add(hanging);
        if indent_left != 0 || left_extra != 0 || list {
            tag.set_left_margin(
                self.view
                    .left_margin()
                    .saturating_add(indent_left)
                    .saturating_add(left_extra),
            );
        }
        let indent_right = twips_to_px_i32(props.indent_right_tw, self.zoom);
        if indent_right != 0 {
            tag.set_right_margin(self.view.right_margin().saturating_add(indent_right));
        }
        let first = if list {
            -hanging
        } else {
            twips_to_px_i32(props.indent_first_tw, self.zoom)
        };
        if first != 0 {
            tag.set_indent(first);
        }
        tag.set_pixels_above_lines(twips_to_px_u32(props.space_before_tw, self.zoom));
        tag.set_pixels_below_lines(twips_to_px_u32(props.space_after_tw, self.zoom));
        if let Some(line) = props.line_height {
            tag.set_line_height(line);
        }
        if !props.tab_stops.is_empty() {
            let mut tabs = pango::TabArray::new(0, true);
            for (index, stop) in props.tab_stops.iter().enumerate() {
                let alignment = match stop.align {
                    crate::model::TabAlign::Left => pango::TabAlign::Left,
                    crate::model::TabAlign::Center => pango::TabAlign::Center,
                    crate::model::TabAlign::Right => pango::TabAlign::Right,
                    crate::model::TabAlign::Decimal => pango::TabAlign::Decimal,
                };
                let index = i32::try_from(index).unwrap_or(i32::MAX);
                tabs.set_tab(index, alignment, twips_to_px_i32(stop.pos_tw, self.zoom));
            }
            tag.set_tabs(Some(&tabs));
        }
        if props.rtl {
            tag.set_direction(gtk::TextDirection::Rtl);
        }
        if muted {
            tag.set_foreground_rgba(Some(&self.theme.muted));
        }
        if let Some(shading) = props.shading {
            tag.set_paragraph_background_rgba(Some(&rgba(shading)));
        }
        self.buffer.tag_table().add(&tag);
        self.para_tags.insert(key, tag.clone());
        tag
    }

    fn link_tag(&mut self, index: usize) -> gtk::TextTag {
        if let Some(tag) = self.link_tags.get(&index) {
            return tag.clone();
        }
        let name = format!("link:{index}");
        if let Some(tag) = self.buffer.tag_table().lookup(&name) {
            self.link_tags.insert(index, tag.clone());
            return tag;
        }
        let tag = gtk::TextTag::new(Some(&name));
        tag.set_underline(pango::Underline::Single);
        tag.set_foreground_rgba(Some(&self.theme.link));
        self.buffer.tag_table().add(&tag);
        self.link_tags.insert(index, tag.clone());
        tag
    }

    fn new_tag_name(&mut self) -> String {
        let name = format!("docxlite-tag-{}", self.next_tag);
        self.next_tag = self.next_tag.saturating_add(1);
        name
    }

    fn render_image(&mut self, image: &ImageRef) {
        if self.rendered_images >= MAX_RENDERED_IMAGES {
            self.insert_text("[Image limit reached]", &placeholder_run(), None, true);
            return;
        }
        let texture = if let Some(texture) = self.image_cache.get(&image.index).cloned() {
            Some(texture)
        } else {
            let Some(data) = self.document.images.get(image.index) else {
                self.insert_text("[Image missing]", &placeholder_run(), None, true);
                return;
            };
            let Some(texture) = decode_image(data) else {
                self.insert_text(
                    &alt_text(&image.alt, "[Unsupported image]"),
                    &placeholder_run(),
                    None,
                    true,
                );
                return;
            };
            self.image_cache.insert(image.index, texture.clone());
            Some(texture)
        };
        let Some(texture) = texture else {
            return;
        };
        self.rendered_images = self.rendered_images.saturating_add(1);
        let natural_width = if image.width_px > 0 {
            image.width_px
        } else {
            texture.width().max(0) as u32
        };
        let natural_height = if image.height_px > 0 {
            image.height_px
        } else {
            texture.height().max(0) as u32
        };
        let content_width = self.content_width() as u32;
        let scale = if natural_width > content_width && natural_width > 0 {
            f64::from(content_width) / f64::from(natural_width)
        } else {
            1.0
        };
        let width = (f64::from(natural_width) * scale).max(1.0) as i32;
        let height = (f64::from(natural_height) * scale).max(1.0) as i32;
        let picture = gtk::Picture::for_paintable(&texture);
        picture.set_can_shrink(true);
        picture.set_size_request(width, height);
        picture.set_halign(gtk::Align::Start);
        let mut end = self.buffer.end_iter();
        let anchor = self.buffer.create_child_anchor(&mut end);
        self.view.add_child_at_anchor(&picture, &anchor);
        self.insert_text("\n", &RunProps::default(), None, false);
    }

    fn content_width(&self) -> i32 {
        let width = self.view.width();
        if width > 100 {
            width
                .saturating_sub(self.view.left_margin())
                .saturating_sub(self.view.right_margin())
        } else {
            700
        }
    }

    fn render_table(&mut self, table: &Table, extra: i32, muted: bool) {
        if self.table_is_tabular(table) {
            self.render_table_tabular(table, extra, muted);
        } else {
            self.render_table_stacked(table, extra, muted);
        }
    }

    fn table_is_tabular(&self, table: &Table) -> bool {
        let Some(first) = table.rows.first() else {
            return false;
        };
        let columns = first.cells.len();
        if columns == 0 || columns > 8 {
            return false;
        }
        let widths = self.table_widths(table, columns);
        for row in &table.rows {
            if row.cells.len() != columns {
                return false;
            }
            for (index, cell) in row.cells.iter().enumerate() {
                if cell.col_span > 1 || cell.v_merge != crate::model::VMerge::None {
                    return false;
                }
                if cell.blocks.len() != 1 {
                    return false;
                }
                let Some(Block::Paragraph(paragraph)) = cell.blocks.first() else {
                    return false;
                };
                if paragraph.list.is_some() {
                    return false;
                }
                let text = inline_text(&paragraph.inlines);
                if text.chars().count() > MAX_TABULAR_CELL_CHARS || text.contains('\n') {
                    return false;
                }
                if paragraph
                    .inlines
                    .iter()
                    .any(|inline| !matches!(inline, Inline::Text { .. } | Inline::Link { .. }))
                {
                    return false;
                }
                if let Some(width) = widths.get(index).copied() {
                    let layout = self.view.create_pango_layout(Some(&text));
                    let (text_width, _) = layout.pixel_size();
                    if text_width > width.saturating_sub(8) {
                        return false;
                    }
                }
            }
        }
        true
    }

    fn table_widths(&self, table: &Table, columns: usize) -> Vec<i32> {
        let content = self.content_width();
        let mut widths: Vec<i32> = table
            .col_widths_tw
            .iter()
            .take(columns)
            .map(|width| twips_to_px_u32(*width, self.zoom))
            .collect();
        if widths.len() < columns {
            let base = content / i32::try_from(columns).unwrap_or(1);
            widths.resize(columns, base.max(40));
        }
        widths
    }

    fn render_table_tabular(&mut self, table: &Table, extra: i32, muted: bool) {
        let columns = table.rows.first().map_or(0, |row| row.cells.len());
        if columns == 0 {
            return;
        }
        let widths = self.table_widths(table, columns);
        let mut positions = Vec::with_capacity(widths.len());
        let mut position: i32 = 0;
        for width in &widths {
            position = position.saturating_add(*width);
            positions.push(position);
        }
        let start_mark = self.buffer.create_mark(None, &self.buffer.end_iter(), true);
        for row in &table.rows {
            for (index, cell) in row.cells.iter().enumerate() {
                if index > 0 {
                    self.insert_text("\t", &RunProps::default(), None, muted);
                }
                let cell_mark = self.buffer.create_mark(None, &self.buffer.end_iter(), true);
                if let Some(Block::Paragraph(paragraph)) = cell.blocks.first() {
                    for inline in &paragraph.inlines {
                        self.render_inline(
                            inline,
                            &paragraph_run(&paragraph.props),
                            None,
                            extra,
                            muted,
                        );
                    }
                }
                if let Some(shading) = cell.shading {
                    let start = self.buffer.iter_at_mark(&cell_mark);
                    let end = self.buffer.end_iter();
                    let tag = self.shade_tag(shading);
                    self.buffer.apply_tag(&tag, &start, &end);
                }
            }
            self.insert_text("\n", &RunProps::default(), None, muted);
        }
        let start = self.buffer.iter_at_mark(&start_mark);
        let end = self.buffer.end_iter();
        let name = self.new_tag_name();
        let tag = gtk::TextTag::new(Some(&name));
        let mut tabs = pango::TabArray::new(0, true);
        for (index, position) in positions.iter().enumerate() {
            let index = i32::try_from(index).unwrap_or(i32::MAX);
            tabs.set_tab(index, pango::TabAlign::Left, *position);
        }
        tag.set_tabs(Some(&tabs));
        tag.set_left_margin(self.view.left_margin().saturating_add(extra));
        tag.set_right_margin(self.view.right_margin());
        if muted {
            tag.set_foreground_rgba(Some(&self.theme.muted));
        }
        self.buffer.tag_table().add(&tag);
        self.buffer.apply_tag(&tag, &start, &end);
    }

    fn shade_tag(&mut self, color: Rgb) -> gtk::TextTag {
        if let Some(tag) = self.shade_tags.get(&color) {
            return tag.clone();
        }
        let name = self.new_tag_name();
        let tag = gtk::TextTag::new(Some(&name));
        tag.set_background_rgba(Some(&rgba(color)));
        self.buffer.tag_table().add(&tag);
        self.shade_tags.insert(color, tag.clone());
        tag
    }

    fn render_table_stacked(&mut self, table: &Table, extra: i32, muted: bool) {
        for (row_index, row) in table.rows.iter().enumerate() {
            if row_index > 0 {
                self.insert_text("\n", &RunProps::default(), None, muted);
            }
            for (cell_index, cell) in row.cells.iter().enumerate() {
                if cell_index > 0 {
                    self.insert_text("\n", &RunProps::default(), None, muted);
                }
                let cell_mark = self.buffer.create_mark(None, &self.buffer.end_iter(), true);
                for block in &cell.blocks {
                    self.render_block(block, extra.saturating_add(24), muted);
                }
                if let Some(shading) = cell.shading {
                    let start = self.buffer.iter_at_mark(&cell_mark);
                    let end = self.buffer.end_iter();
                    let tag = self.shade_tag(shading);
                    self.buffer.apply_tag(&tag, &start, &end);
                }
            }
        }
    }
}

fn inlines_end_with_break(inlines: &[Inline]) -> bool {
    match inlines.last() {
        Some(Inline::LineBreak | Inline::PageBreak) => true,
        Some(Inline::Text { text, .. }) => text.ends_with('\n'),
        _ => false,
    }
}

fn paragraph_run(props: &ParaProps) -> RunProps {
    let mut run = RunProps::default();
    if let Some(level) = props.heading_level {
        run.bold = true;
        if run.size_half_pt.is_none() {
            run.size_half_pt = Some(match level {
                1 => 48,
                2 => 38,
                3 => 32,
                4 => 28,
                _ => 24,
            });
        }
    }
    run
}

fn placeholder_run() -> RunProps {
    RunProps {
        italic: true,
        ..RunProps::default()
    }
}

fn rgba(color: Rgb) -> gdk::RGBA {
    gdk::RGBA::new(
        f32::from(color.r) / 255.0,
        f32::from(color.g) / 255.0,
        f32::from(color.b) / 255.0,
        1.0,
    )
}

fn inline_text(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for inline in inlines {
        match inline {
            Inline::Text { text, .. } => out.push_str(text),
            Inline::Tab => out.push('\t'),
            Inline::LineBreak | Inline::PageBreak => out.push('\n'),
            Inline::Link { children, .. } => out.push_str(&inline_text(children)),
            Inline::Placeholder(text) => out.push_str(text),
            Inline::FootnoteRef(number) => {
                out.push('[');
                out.push_str(&number.to_string());
                out.push(']');
            }
            Inline::Image(_) | Inline::Bookmark(_) | Inline::TextBox(_) => {}
        }
    }
    out
}

fn alt_text(alt: &str, fallback: &str) -> String {
    if alt.is_empty() {
        fallback.to_string()
    } else {
        format!("{fallback}: {alt}")
    }
}

fn decode_image(data: &crate::model::ImageData) -> Option<gdk::Texture> {
    let loader = gdk_pixbuf::PixbufLoader::new();
    let weak_loader = loader.downgrade();
    loader.connect_size_prepared(move |_, width, height| {
        let Some(loader) = weak_loader.upgrade() else {
            return;
        };
        let width = u64::try_from(width.max(1)).unwrap_or(1);
        let height = u64::try_from(height.max(1)).unwrap_or(1);
        let pixels = width.saturating_mul(height);
        let max_dimension = u64::from(MAX_IMAGE_DIMENSION);
        if pixels > MAX_IMAGE_PIXELS || width > max_dimension || height > max_dimension {
            let scale =
                (f64::from(MAX_IMAGE_DIMENSION) / f64::from(width.max(height) as u32)).min(1.0);
            let new_width = ((width as f64) * scale).round().max(1.0) as i32;
            let new_height = ((height as f64) * scale).round().max(1.0) as i32;
            loader.set_size(new_width, new_height);
        }
    });
    loader.write(data.bytes.as_ref()).ok()?;
    loader.close().ok()?;
    let pixbuf = loader.pixbuf()?;
    let width = u64::try_from(pixbuf.width()).ok()?;
    let height = u64::try_from(pixbuf.height()).ok()?;
    if width.saturating_mul(height) > MAX_IMAGE_PIXELS {
        return None;
    }
    let max_dimension = i32::try_from(MAX_IMAGE_DIMENSION).unwrap_or(i32::MAX);
    let pixbuf = if pixbuf.width() > max_dimension || pixbuf.height() > max_dimension {
        let scale = f64::from(max_dimension) / f64::from(pixbuf.width().max(pixbuf.height()));
        let new_width = (f64::from(pixbuf.width()) * scale).round().max(1.0) as i32;
        let new_height = (f64::from(pixbuf.height()) * scale).round().max(1.0) as i32;
        pixbuf.scale_simple(new_width, new_height, gdk_pixbuf::InterpType::Bilinear)?
    } else {
        pixbuf
    };
    Some(gdk::Texture::for_pixbuf(&pixbuf))
}
