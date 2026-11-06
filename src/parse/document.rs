use std::collections::HashMap;
use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use roxmltree::{Document as XmlDocument, Node, ParsingOptions};
use std::fs::File;
use zip::ZipArchive;

use crate::error::{LoadError, Result};
use crate::limits::Budget;
use crate::limits::{MAX_DEPTH, MAX_IMAGE_BYTES, MAX_TABLE_NESTING};
use crate::loader::{HeaderFooterPart, Package};
use crate::model::{
    Block, Cell, Document, HeaderFooter, HeaderFooterKind, ImageData, ImageRef, Inline, LinkTarget,
    Paragraph, Row, RunProps, Table, VMerge, Warning, WarningKind,
};
use crate::parse::numbering::{Numbering, parse_numbering};
use crate::parse::rels::Rels;
use crate::parse::styles::{StyleSheet, parse_para_props, parse_run_props, parse_styles};
use crate::parse::theme::{Theme, default_theme, parse_theme};
use crate::xmlutil::{
    attr_r, attr_w, emu_to_px, is_a, is_mc, is_w, is_wp, normalize_text, parse_color, parse_u32,
    val, xml_preserve,
};

pub fn parse_document(package: Package, cancel: Arc<AtomicBool>) -> Result<Document> {
    let Package {
        mut archive,
        main_part,
        main_xml,
        main_rels,
        styles_xml,
        numbering_xml,
        theme_xml,
        header_footer_parts,
        stats: _,
    } = package;

    let theme = theme_xml
        .as_deref()
        .and_then(parse_theme)
        .unwrap_or_else(default_theme);
    let styles = parse_styles(styles_xml.as_deref(), &theme)?;
    let numbering = parse_numbering(numbering_xml.as_deref())?;

    let mut parser = Parser {
        archive: &mut archive,
        styles,
        numbering,
        theme,
        budget: Budget::new(cancel),
        images: Vec::new(),
        image_index: HashMap::new(),
        warnings: Vec::new(),
        truncated: false,
    };

    validate_xml(&main_xml)?;
    let main_dir = crate::parse::rels::base_dir(&main_part).to_string();
    let blocks = parser.parse_xml_blocks(&main_xml, &main_rels, &main_dir, 0);

    let mut headers = Vec::new();
    let mut footers = Vec::new();
    for part in &header_footer_parts {
        let kind = header_footer_kind(&part.xml);
        let part_blocks = parser.parse_xml_blocks(
            &part.xml,
            &part.rels,
            crate::parse::rels::base_dir(&part.name),
            0,
        );
        let header_footer = HeaderFooter {
            kind,
            part_name: part.name.clone(),
            blocks: part_blocks,
        };
        if is_header_part(&part.xml, &part.name) {
            headers.push(header_footer);
        } else if is_footer_part(&part.xml, &part.name) {
            footers.push(header_footer);
        }
    }

    let mut document = Document {
        blocks,
        headers,
        footers,
        warnings: parser.warnings,
        truncated: parser.truncated,
        images: parser.images,
        stats: crate::model::Stats {
            nodes: parser.budget.nodes,
            xml_bytes: xml_bytes(
                &main_xml,
                styles_xml.as_deref(),
                numbering_xml.as_deref(),
                theme_xml.as_deref(),
                &header_footer_parts,
            ),
            images: 0,
            table_cells: parser.budget.table_cells,
        },
    };
    document.stats.images = u32::try_from(document.images.len()).unwrap_or(u32::MAX);
    if parser.truncated || parser.budget.truncated {
        document.truncated = true;
        document.add_warning(WarningKind::Truncated);
    }
    if document.headers.len() + document.footers.len() > 2 {
        document.add_warning(WarningKind::HeaderFooterVariants);
    }
    if document.blocks.is_empty()
        && document.headers.is_empty()
        && document.footers.is_empty()
        && !document.truncated
    {
        document.add_warning(WarningKind::Other);
    }
    Ok(document)
}

fn validate_xml(xml: &str) -> Result<()> {
    let options = ParsingOptions {
        allow_dtd: false,
        nodes_limit: u32::MAX,
        ..ParsingOptions::default()
    };
    XmlDocument::parse_with_options(xml, options)
        .map(|_| ())
        .map_err(|_| LoadError::Malformed)
}

fn xml_bytes(
    main: &str,
    styles: Option<&str>,
    numbering: Option<&str>,
    theme: Option<&str>,
    header_footer: &[HeaderFooterPart],
) -> u64 {
    let mut total = main.len();
    for part in [styles, numbering, theme].into_iter().flatten() {
        total = total.saturating_add(part.len());
    }
    for part in header_footer {
        total = total.saturating_add(part.xml.len());
    }
    u64::try_from(total).unwrap_or(u64::MAX)
}

fn part_root_name(xml: &str) -> Option<String> {
    let options = ParsingOptions {
        allow_dtd: false,
        nodes_limit: 100_000,
        ..ParsingOptions::default()
    };
    let document = XmlDocument::parse_with_options(xml, options).ok()?;
    Some(document.root_element().tag_name().name().to_string())
}

fn is_header_part(xml: &str, name: &str) -> bool {
    part_root_name(xml).as_deref() == Some("hdr") || name.contains("header")
}

fn is_footer_part(xml: &str, name: &str) -> bool {
    part_root_name(xml).as_deref() == Some("ftr") || name.contains("footer")
}

fn header_footer_kind(xml: &str) -> HeaderFooterKind {
    let options = ParsingOptions {
        allow_dtd: false,
        nodes_limit: 100_000,
        ..ParsingOptions::default()
    };
    let Ok(document) = XmlDocument::parse_with_options(xml, options) else {
        return HeaderFooterKind::Default;
    };
    let root = document.root_element();
    match attr_w(root, "type") {
        Some(value) if value.eq_ignore_ascii_case("first") => HeaderFooterKind::First,
        Some(value) if value.eq_ignore_ascii_case("even") => HeaderFooterKind::Even,
        _ => HeaderFooterKind::Default,
    }
}

struct Parser<'a> {
    archive: &'a mut ZipArchive<File>,
    styles: StyleSheet,
    numbering: Numbering,
    theme: Theme,
    budget: Budget,
    images: Vec<Arc<ImageData>>,
    image_index: HashMap<String, usize>,
    warnings: Vec<Warning>,
    truncated: bool,
}

impl Parser<'_> {
    fn parse_xml_blocks(
        &mut self,
        xml: &str,
        rels: &Rels,
        base_dir: &str,
        depth: u32,
    ) -> Vec<Block> {
        let options = ParsingOptions {
            allow_dtd: false,
            nodes_limit: u32::try_from(crate::limits::MAX_XML_NODES).unwrap_or(u32::MAX),
            ..ParsingOptions::default()
        };
        let Ok(document) = XmlDocument::parse_with_options(xml, options) else {
            self.warn(WarningKind::UnsupportedElement);
            return Vec::new();
        };
        let root = document.root_element();
        let container = root
            .children()
            .find(|node| is_w(*node, "body"))
            .or_else(|| {
                root.children()
                    .find(|node| is_w(*node, "hdr") || is_w(*node, "ftr"))
            })
            .unwrap_or(root);
        self.parse_blocks(container, rels, base_dir, depth)
    }

    fn parse_blocks(
        &mut self,
        node: Node<'_, '_>,
        rels: &Rels,
        base_dir: &str,
        depth: u32,
    ) -> Vec<Block> {
        let mut blocks = Vec::new();
        if depth >= MAX_DEPTH {
            self.warn(WarningKind::UnsupportedElement);
            return blocks;
        }
        for child in node.children().filter(|child| child.is_element()) {
            if !self.budget.tick() {
                self.truncated = true;
                break;
            }
            if is_w(child, "p") {
                if let Some(paragraph) = self.parse_paragraph(child, rels, base_dir, depth) {
                    blocks.push(Block::Paragraph(paragraph));
                }
            } else if is_w(child, "tbl") {
                if let Some(table) = self.parse_table(child, rels, base_dir, depth) {
                    blocks.push(Block::Table(table));
                }
            } else if is_w(child, "sectPr") || is_w(child, "del") || is_w(child, "moveFrom") {
                continue;
            } else if is_w(child, "sdt") {
                if let Some(content) = self.child(child, "sdtContent") {
                    blocks.extend(self.parse_blocks(
                        content,
                        rels,
                        base_dir,
                        depth.saturating_add(1),
                    ));
                }
            } else if is_w(child, "ins")
                || is_w(child, "moveTo")
                || is_w(child, "customXml")
                || is_w(child, "smartTag")
            {
                let before = blocks.len();
                blocks.extend(self.parse_blocks(child, rels, base_dir, depth.saturating_add(1)));
                if blocks.len() == before {
                    let inlines = self.parse_inlines(
                        child,
                        rels,
                        base_dir,
                        depth.saturating_add(1),
                        &RunProps::default(),
                    );
                    if !inlines.is_empty() {
                        blocks.push(Block::Paragraph(Paragraph {
                            inlines,
                            ..Paragraph::default()
                        }));
                    }
                }
            } else if is_w(child, "bookmarkStart")
                || is_w(child, "bookmarkEnd")
                || is_w(child, "proofErr")
            {
                continue;
            } else if is_mc(child, "AlternateContent") {
                let branch = child
                    .children()
                    .find(|candidate| is_mc(*candidate, "Choice"))
                    .or_else(|| {
                        child
                            .children()
                            .find(|candidate| is_mc(*candidate, "Fallback"))
                    });
                if let Some(branch) = branch {
                    blocks.extend(self.parse_blocks(
                        branch,
                        rels,
                        base_dir,
                        depth.saturating_add(1),
                    ));
                }
            } else if child
                .descendants()
                .any(|descendant| is_w(descendant, "p") || is_w(descendant, "tbl"))
            {
                blocks.extend(self.parse_blocks(child, rels, base_dir, depth.saturating_add(1)));
            } else if is_w(child, "t") {
                let text = normalize_text(child.text().unwrap_or_default(), false);
                if !text.is_empty() {
                    blocks.push(Block::Paragraph(Paragraph {
                        inlines: vec![Inline::Text {
                            text,
                            props: RunProps::default(),
                        }],
                        ..Paragraph::default()
                    }));
                }
            } else {
                let inlines = self.parse_inlines(
                    child,
                    rels,
                    base_dir,
                    depth.saturating_add(1),
                    &RunProps::default(),
                );
                if !inlines.is_empty() {
                    blocks.push(Block::Paragraph(Paragraph {
                        inlines,
                        ..Paragraph::default()
                    }));
                }
            }
        }
        blocks
    }

    fn parse_paragraph(
        &mut self,
        node: Node<'_, '_>,
        rels: &Rels,
        base_dir: &str,
        depth: u32,
    ) -> Option<Paragraph> {
        let ppr = self.child(node, "pPr");
        let direct = ppr
            .map(|ppr_node| parse_para_props(ppr_node, &self.theme))
            .unwrap_or_default();
        let style_id = direct.style_id.clone();
        let mut resolved = self.styles.resolve_paragraph(style_id.as_deref());
        direct.apply(&mut resolved.para);
        let list = resolved
            .para
            .num_id
            .as_deref()
            .and_then(|num_id| self.numbering.label(num_id, resolved.para.num_level));
        let inlines = self.parse_inlines(node, rels, base_dir, depth, &resolved.run);
        Some(Paragraph {
            props: resolved.para,
            list,
            inlines,
        })
    }

    fn parse_inlines(
        &mut self,
        node: Node<'_, '_>,
        rels: &Rels,
        base_dir: &str,
        depth: u32,
        base_run: &RunProps,
    ) -> Vec<Inline> {
        let mut out = Vec::new();
        let mut fields = Vec::new();
        self.walk_inlines(node, rels, base_dir, depth, base_run, &mut fields, &mut out);
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn walk_inlines(
        &mut self,
        node: Node<'_, '_>,
        rels: &Rels,
        base_dir: &str,
        depth: u32,
        base_run: &RunProps,
        fields: &mut Vec<bool>,
        out: &mut Vec<Inline>,
    ) {
        if depth >= MAX_DEPTH {
            self.warn(WarningKind::UnsupportedElement);
            return;
        }
        for child in node.children().filter(|child| child.is_element()) {
            if !self.budget.tick() {
                self.truncated = true;
                return;
            }
            if is_w(child, "r") {
                self.parse_run(child, rels, base_dir, depth, base_run, fields, out);
            } else if is_w(child, "hyperlink") {
                let target = self.hyperlink_target(child, rels);
                let mut children = Vec::new();
                self.walk_inlines(
                    child,
                    rels,
                    base_dir,
                    depth.saturating_add(1),
                    base_run,
                    fields,
                    &mut children,
                );
                if let Some(target) = target {
                    out.push(Inline::Link { target, children });
                } else {
                    out.extend(children);
                }
            } else if is_w(child, "fldSimple") {
                let before = out.len();
                self.walk_inlines(
                    child,
                    rels,
                    base_dir,
                    depth.saturating_add(1),
                    base_run,
                    fields,
                    out,
                );
                if out.len() == before {
                    out.push(Inline::Placeholder("[Field]".to_string()));
                }
            } else if is_w(child, "ins") || is_w(child, "moveTo") {
                self.walk_inlines(
                    child,
                    rels,
                    base_dir,
                    depth.saturating_add(1),
                    base_run,
                    fields,
                    out,
                );
            } else if is_w(child, "del") || is_w(child, "moveFrom") {
                continue;
            } else if is_w(child, "sdt") {
                if let Some(content) = self.child(child, "sdtContent") {
                    self.walk_inlines(
                        content,
                        rels,
                        base_dir,
                        depth.saturating_add(1),
                        base_run,
                        fields,
                        out,
                    );
                }
            } else if is_w(child, "smartTag") || is_w(child, "customXml") {
                self.walk_inlines(
                    child,
                    rels,
                    base_dir,
                    depth.saturating_add(1),
                    base_run,
                    fields,
                    out,
                );
            } else if is_w(child, "bookmarkStart") {
                if let Some(name) = attr_w(child, "name") {
                    out.push(Inline::Bookmark(name.to_string()));
                }
            } else if is_w(child, "bookmarkEnd")
                || is_w(child, "proofErr")
                || is_w(child, "commentReference")
            {
                continue;
            } else if is_w(child, "drawing") {
                if let Some(inline) = self.parse_image(child, rels, base_dir, depth) {
                    out.push(inline);
                } else {
                    out.push(Inline::Placeholder("[Drawing]".to_string()));
                }
            } else if is_w(child, "pict") {
                if let Some(inline) = self.parse_pict(child, rels, base_dir, depth) {
                    out.push(inline);
                }
            } else if is_w(child, "object") {
                out.push(Inline::Placeholder("[Embedded object]".to_string()));
            } else if is_mc(child, "AlternateContent") {
                let branch =
                    self.parse_alternate_content(child, rels, base_dir, depth, base_run, fields);
                out.extend(branch);
            } else if is_w(child, "t") {
                let text = normalize_text(child.text().unwrap_or_default(), xml_preserve(child));
                if !text.is_empty() {
                    out.push(Inline::Text {
                        text,
                        props: base_run.clone(),
                    });
                }
            } else if is_w(child, "comment") || is_w(child, "footnote") || is_w(child, "endnote") {
                continue;
            } else if child.descendants().any(|descendant| is_w(descendant, "t")) {
                self.extract_text(child, base_run, out);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn parse_run(
        &mut self,
        node: Node<'_, '_>,
        rels: &Rels,
        base_dir: &str,
        depth: u32,
        base_run: &RunProps,
        fields: &mut Vec<bool>,
        out: &mut Vec<Inline>,
    ) {
        let direct = self
            .child(node, "rPr")
            .map(|rpr| parse_run_props(rpr, &self.theme))
            .unwrap_or_default();
        let mut run = base_run.clone();
        if let Some(style_id) = direct.char_style.as_deref() {
            let character = self.styles.resolve_character_partial(Some(style_id));
            character.apply(&mut run);
        }
        direct.apply(&mut run);
        for child in node.children().filter(|child| child.is_element()) {
            if !self.budget.tick() {
                self.truncated = true;
                return;
            }
            if is_w(child, "fldChar") {
                match attr_w(child, "fldCharType").unwrap_or_default() {
                    "begin" => fields.push(false),
                    "separate" => {
                        if let Some(last) = fields.last_mut() {
                            *last = true;
                        }
                    }
                    "end" => {
                        fields.pop();
                    }
                    _ => {}
                }
                continue;
            }
            if is_w(child, "instrText") || is_w(child, "delText") {
                continue;
            }
            let in_instruction = fields.last().is_some_and(|separated| !*separated);
            if in_instruction {
                continue;
            }
            if is_w(child, "t") {
                let text = normalize_text(child.text().unwrap_or_default(), xml_preserve(child));
                if !text.is_empty() {
                    out.push(Inline::Text {
                        text,
                        props: run.clone(),
                    });
                }
            } else if is_w(child, "tab") {
                out.push(Inline::Tab);
            } else if is_w(child, "br") {
                if attr_w(child, "type").is_some_and(|value| value.eq_ignore_ascii_case("page")) {
                    out.push(Inline::PageBreak);
                } else {
                    out.push(Inline::LineBreak);
                }
            } else if is_w(child, "cr") {
                out.push(Inline::LineBreak);
            } else if is_w(child, "noBreakHyphen") {
                out.push(Inline::Text {
                    text: "\u{2011}".to_string(),
                    props: run.clone(),
                });
            } else if is_w(child, "softHyphen") {
                out.push(Inline::Text {
                    text: "\u{00AD}".to_string(),
                    props: run.clone(),
                });
            } else if is_w(child, "sym") {
                if let Some(character) = symbol_character(child) {
                    out.push(Inline::Text {
                        text: character.to_string(),
                        props: run.clone(),
                    });
                }
            } else if is_w(child, "drawing") {
                if let Some(inline) = self.parse_image(child, rels, base_dir, depth) {
                    out.push(inline);
                } else {
                    out.push(Inline::Placeholder("[Drawing]".to_string()));
                }
            } else if is_w(child, "pict") {
                if let Some(inline) = self.parse_pict(child, rels, base_dir, depth) {
                    out.push(inline);
                }
            } else if is_w(child, "object") {
                out.push(Inline::Placeholder("[Embedded object]".to_string()));
            } else if is_w(child, "footnoteReference") || is_w(child, "endnoteReference") {
                let number = attr_w(child, "id").and_then(parse_u32).unwrap_or(0);
                out.push(Inline::FootnoteRef(number));
            } else if is_w(child, "commentReference") {
                continue;
            } else if child.descendants().any(|descendant| is_w(descendant, "t")) {
                self.extract_text(child, &run, out);
            }
        }
    }

    fn hyperlink_target(&self, node: Node<'_, '_>, rels: &Rels) -> Option<LinkTarget> {
        if let Some(anchor) = attr_w(node, "anchor").filter(|value| !value.is_empty()) {
            return Some(LinkTarget::Anchor(anchor.to_string()));
        }
        let id = attr_r(node, "id")?;
        let relationship = rels.get(id)?;
        if relationship.external {
            Some(LinkTarget::External(relationship.target.clone()))
        } else {
            Some(LinkTarget::Anchor(relationship.target.clone()))
        }
    }

    fn parse_alternate_content(
        &mut self,
        node: Node<'_, '_>,
        rels: &Rels,
        base_dir: &str,
        depth: u32,
        base_run: &RunProps,
        fields: &mut Vec<bool>,
    ) -> Vec<Inline> {
        let mut choice = Vec::new();
        let mut fallback = Vec::new();
        for child in node.children().filter(|child| child.is_element()) {
            if is_mc(child, "Choice") {
                self.walk_inlines(
                    child,
                    rels,
                    base_dir,
                    depth.saturating_add(1),
                    base_run,
                    fields,
                    &mut choice,
                );
            } else if is_mc(child, "Fallback") {
                self.walk_inlines(
                    child,
                    rels,
                    base_dir,
                    depth.saturating_add(1),
                    base_run,
                    fields,
                    &mut fallback,
                );
            }
        }
        if choice.is_empty() { fallback } else { choice }
    }

    fn parse_pict(
        &mut self,
        node: Node<'_, '_>,
        rels: &Rels,
        base_dir: &str,
        depth: u32,
    ) -> Option<Inline> {
        if let Some(content) = node
            .descendants()
            .find(|descendant| is_w(*descendant, "txbxContent"))
        {
            let blocks = self.parse_blocks(content, rels, base_dir, depth.saturating_add(1));
            return Some(Inline::TextBox(blocks));
        }
        let mut text = String::new();
        collect_text(node, &mut text);
        if text.trim().is_empty() {
            Some(Inline::Placeholder("[Drawing]".to_string()))
        } else {
            Some(Inline::Text {
                text,
                props: RunProps::default(),
            })
        }
    }

    fn parse_image(
        &mut self,
        node: Node<'_, '_>,
        rels: &Rels,
        base_dir: &str,
        depth: u32,
    ) -> Option<Inline> {
        if let Some(content) = node
            .descendants()
            .find(|descendant| is_w(*descendant, "txbxContent"))
        {
            let blocks = self.parse_blocks(content, rels, base_dir, depth.saturating_add(1));
            if !blocks.is_empty() {
                return Some(Inline::TextBox(blocks));
            }
        }
        let blip = node
            .descendants()
            .find(|descendant| is_a(*descendant, "blip"))?;
        let alt = node
            .descendants()
            .find(|descendant| is_wp(*descendant, "docPr"))
            .and_then(|doc_pr| {
                doc_pr
                    .attribute("descr")
                    .or_else(|| doc_pr.attribute("title"))
                    .map(str::to_string)
            })
            .unwrap_or_default();
        let extent = node
            .descendants()
            .find(|descendant| is_wp(*descendant, "extent"));
        let width_px = extent
            .and_then(|extent| extent.attribute("cx"))
            .and_then(|value| value.parse::<i64>().ok())
            .map(emu_to_px)
            .unwrap_or(0);
        let height_px = extent
            .and_then(|extent| extent.attribute("cy"))
            .and_then(|value| value.parse::<i64>().ok())
            .map(emu_to_px)
            .unwrap_or(0);
        let embed = attr_r(blip, "embed");
        let link = attr_r(blip, "link");
        if link.is_some() && embed.is_none() {
            return Some(Inline::Placeholder("[External image]".to_string()));
        }
        let Some(embed) = embed else {
            return Some(Inline::Placeholder("[Image]".to_string()));
        };
        let Some(relationship) = rels.get(embed) else {
            self.warn(WarningKind::SkippedImage);
            return Some(Inline::Placeholder(alt_or("[Image]", &alt)));
        };
        if relationship.external {
            return Some(Inline::Placeholder("[External image]".to_string()));
        }
        if let Some(index) = self.image_index.get(&relationship.target).copied() {
            return Some(Inline::Image(ImageRef {
                index,
                width_px,
                height_px,
                alt,
            }));
        }
        let bytes = match self.read_image_bytes(&relationship.target) {
            Ok(bytes) => bytes,
            Err(()) => {
                self.warn(WarningKind::SkippedImage);
                return Some(Inline::Placeholder(alt_or("[Image]", &alt)));
            }
        };
        if !self
            .budget
            .add_image(u64::try_from(bytes.len()).unwrap_or(u64::MAX))
        {
            self.warn(WarningKind::SkippedImage);
            return Some(Inline::Placeholder(alt_or("[Image]", &alt)));
        }
        let index = self.images.len();
        let mime = mime_for(&relationship.target);
        self.images.push(Arc::new(ImageData {
            bytes: Arc::from(bytes.into_boxed_slice()),
            mime,
            alt: alt.clone(),
        }));
        self.image_index.insert(relationship.target.clone(), index);
        Some(Inline::Image(ImageRef {
            index,
            width_px,
            height_px,
            alt,
        }))
    }

    fn read_image_bytes(&mut self, name: &str) -> std::result::Result<Vec<u8>, ()> {
        let file = self.archive.by_name(name).map_err(|_| ())?;
        let declared = file.size();
        let compressed = file.compressed_size();
        if declared > MAX_IMAGE_BYTES {
            return Err(());
        }
        if compressed == 0 {
            if declared > 0 {
                return Err(());
            }
        } else if declared / compressed > crate::limits::MAX_COMPRESSION_RATIO {
            return Err(());
        }
        drop(file);
        let mut file = self.archive.by_name(name).map_err(|_| ())?;
        let mut bytes = Vec::new();
        let mut limited = std::io::Read::take(&mut file, MAX_IMAGE_BYTES.saturating_add(1));
        limited.read_to_end(&mut bytes).map_err(|_| ())?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_IMAGE_BYTES {
            return Err(());
        }
        Ok(bytes)
    }

    fn parse_table(
        &mut self,
        node: Node<'_, '_>,
        rels: &Rels,
        base_dir: &str,
        depth: u32,
    ) -> Option<Table> {
        if depth > MAX_TABLE_NESTING {
            self.warn(WarningKind::UnsupportedElement);
            return None;
        }
        let mut table = Table::default();
        if let Some(tbl_pr) = self.child(node, "tblPr")
            && let Some(borders) = self.child(tbl_pr, "tblBorders")
        {
            table.borders.top = border_present(self.child(borders, "top"));
            table.borders.bottom = border_present(self.child(borders, "bottom"));
            table.borders.left = border_present(self.child(borders, "left"));
            table.borders.right = border_present(self.child(borders, "right"));
        }
        if let Some(grid) = self.child(node, "tblGrid") {
            for col in grid.children().filter(|child| is_w(*child, "gridCol")) {
                if let Some(width) = attr_w(col, "w").and_then(parse_u32) {
                    table.col_widths_tw.push(width);
                }
            }
        }
        for row_node in node.children().filter(|child| is_w(*child, "tr")) {
            if !self.budget.tick() {
                self.truncated = true;
                break;
            }
            let mut row = Row::default();
            for cell_node in row_node.children().filter(|child| is_w(*child, "tc")) {
                if !self.budget.tick() || !self.budget.add_table_cell() {
                    self.truncated = true;
                    break;
                }
                let mut cell = Cell::default();
                if let Some(tc_pr) = self.child(cell_node, "tcPr") {
                    if let Some(span) = self
                        .child(tc_pr, "gridSpan")
                        .and_then(val)
                        .and_then(parse_u32)
                    {
                        cell.col_span = u16::try_from(span).unwrap_or(u16::MAX);
                    }
                    if let Some(merge) = self.child(tc_pr, "vMerge") {
                        cell.v_merge = match val(merge) {
                            Some("restart") => VMerge::Restart,
                            _ => VMerge::Continue,
                        };
                    }
                    if let Some(shd) = self.child(tc_pr, "shd") {
                        cell.shading = parse_color(shd);
                    }
                }
                cell.blocks = self.parse_blocks(cell_node, rels, base_dir, depth.saturating_add(1));
                row.cells.push(cell);
            }
            if !row.cells.is_empty() {
                table.rows.push(row);
            }
        }
        Some(table)
    }

    fn extract_text(&mut self, node: Node<'_, '_>, run: &RunProps, out: &mut Vec<Inline>) {
        for child in node.children().filter(|child| child.is_element()) {
            if !self.budget.tick() {
                self.truncated = true;
                return;
            }
            if is_w(child, "t") {
                let text = normalize_text(child.text().unwrap_or_default(), xml_preserve(child));
                if !text.is_empty() {
                    out.push(Inline::Text {
                        text,
                        props: run.clone(),
                    });
                }
            } else if is_w(child, "tab") {
                out.push(Inline::Tab);
            } else if is_w(child, "br") || is_w(child, "cr") {
                out.push(Inline::LineBreak);
            } else if is_w(child, "delText") || is_w(child, "instrText") {
                continue;
            } else {
                self.extract_text(child, run, out);
            }
        }
    }

    fn child<'a, 'input>(&self, node: Node<'a, 'input>, local: &str) -> Option<Node<'a, 'input>> {
        node.children()
            .find(|child| child.is_element() && child.tag_name().name() == local)
    }

    fn warn(&mut self, kind: WarningKind) {
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
}

fn alt_or(placeholder: &str, alt: &str) -> String {
    if alt.is_empty() {
        placeholder.to_string()
    } else {
        format!("{placeholder}: {alt}")
    }
}

fn border_present(node: Option<Node<'_, '_>>) -> bool {
    node.and_then(val).is_some_and(|value| {
        !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "nil" | "none" | "0"
        )
    })
}

fn mime_for(path: &str) -> String {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".bmp") {
        "image/bmp"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".tif") || lower.ends_with(".tiff") {
        "image/tiff"
    } else {
        "application/octet-stream"
    }
    .to_string()
}

fn symbol_character(node: Node<'_, '_>) -> Option<char> {
    let value = attr_w(node, "char")?;
    let code = u32::from_str_radix(value.trim(), 16).ok()?;
    char::from_u32(code).or(Some('\u{FFFD}'))
}

fn collect_text(node: Node<'_, '_>, out: &mut String) {
    for child in node.children() {
        if child.is_text() {
            if let Some(text) = child.text() {
                out.push_str(text);
            }
        } else if child.is_element() && !is_w(child, "delText") && !is_w(child, "instrText") {
            collect_text(child, out);
        }
    }
}
