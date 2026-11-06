use std::collections::{HashMap, HashSet};

use roxmltree::{Document, Node, ParsingOptions};

use crate::error::{LoadError, Result};
use crate::model::{Align, ParaProps, Rgb, RunProps, VertAlign};
use crate::parse::theme::Theme;
use crate::xmlutil::{
    attr_w, bool_element, parse_align, parse_bool, parse_f32, parse_hex_rgb, parse_i32, parse_u32,
    parse_underline, parse_vert_align, val,
};

#[derive(Clone, Debug, Default)]
pub struct PartialRunProps {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strike: Option<bool>,
    pub small_caps: Option<bool>,
    pub all_caps: Option<bool>,
    pub vert: Option<VertAlign>,
    pub size_half_pt: Option<u16>,
    pub color: Option<Rgb>,
    pub highlight: Option<Rgb>,
    pub font: Option<String>,
    pub char_style: Option<String>,
}

impl PartialRunProps {
    pub fn apply(&self, target: &mut RunProps) {
        if let Some(value) = self.bold {
            target.bold = value;
        }
        if let Some(value) = self.italic {
            target.italic = value;
        }
        if let Some(value) = self.underline {
            target.underline = value;
        }
        if let Some(value) = self.strike {
            target.strike = value;
        }
        if let Some(value) = self.small_caps {
            target.small_caps = value;
        }
        if let Some(value) = self.all_caps {
            target.all_caps = value;
        }
        if let Some(value) = self.vert {
            target.vert = value;
        }
        if let Some(value) = self.size_half_pt {
            target.size_half_pt = Some(value);
        }
        if let Some(value) = self.color {
            target.color = Some(value);
        }
        if let Some(value) = self.highlight {
            target.highlight = Some(value);
        }
        if let Some(value) = &self.font {
            target.font = Some(value.clone());
        }
    }

    pub fn merge(&mut self, other: &Self) {
        if other.bold.is_some() {
            self.bold = other.bold;
        }
        if other.italic.is_some() {
            self.italic = other.italic;
        }
        if other.underline.is_some() {
            self.underline = other.underline;
        }
        if other.strike.is_some() {
            self.strike = other.strike;
        }
        if other.small_caps.is_some() {
            self.small_caps = other.small_caps;
        }
        if other.all_caps.is_some() {
            self.all_caps = other.all_caps;
        }
        if other.vert.is_some() {
            self.vert = other.vert;
        }
        if other.size_half_pt.is_some() {
            self.size_half_pt = other.size_half_pt;
        }
        if other.color.is_some() {
            self.color = other.color;
        }
        if other.highlight.is_some() {
            self.highlight = other.highlight;
        }
        if other.font.is_some() {
            self.font = other.font.clone();
        }
        if other.char_style.is_some() {
            self.char_style = other.char_style.clone();
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct PartialParaProps {
    pub heading_level: Option<u8>,
    pub align: Option<Align>,
    pub indent_left_tw: Option<i32>,
    pub indent_right_tw: Option<i32>,
    pub indent_first_tw: Option<i32>,
    pub space_before_tw: Option<u32>,
    pub space_after_tw: Option<u32>,
    pub line_height: Option<f32>,
    pub rtl: Option<bool>,
    pub shading: Option<Rgb>,
    pub num_id: Option<String>,
    pub num_level: Option<u8>,
    pub style_id: Option<String>,
    pub tab_stops: Option<Vec<crate::model::TabStop>>,
}

impl PartialParaProps {
    pub fn apply(&self, target: &mut ParaProps) {
        if let Some(value) = self.heading_level {
            target.heading_level = Some(value);
        }
        if let Some(value) = self.align {
            target.align = value;
        }
        if let Some(value) = self.indent_left_tw {
            target.indent_left_tw = value;
        }
        if let Some(value) = self.indent_right_tw {
            target.indent_right_tw = value;
        }
        if let Some(value) = self.indent_first_tw {
            target.indent_first_tw = value;
        }
        if let Some(value) = self.space_before_tw {
            target.space_before_tw = value;
        }
        if let Some(value) = self.space_after_tw {
            target.space_after_tw = value;
        }
        if let Some(value) = self.line_height {
            target.line_height = Some(value);
        }
        if let Some(value) = self.rtl {
            target.rtl = value;
        }
        if let Some(value) = self.shading {
            target.shading = Some(value);
        }
        if let Some(value) = &self.num_id {
            target.num_id = Some(value.clone());
        }
        if let Some(value) = self.num_level {
            target.num_level = value;
        }
        if let Some(value) = &self.style_id {
            target.style_id = Some(value.clone());
        }
        if let Some(value) = &self.tab_stops {
            target.tab_stops = value.clone();
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StyleKind {
    Paragraph,
    Character,
}

#[derive(Clone, Debug)]
pub struct Style {
    pub id: String,
    pub kind: StyleKind,
    pub name: Option<String>,
    pub based_on: Option<String>,
    pub is_default: bool,
    pub para: PartialParaProps,
    pub run: PartialRunProps,
}

#[derive(Clone, Debug, Default)]
pub struct StyleSheet {
    pub defaults_run: PartialRunProps,
    pub defaults_para: PartialParaProps,
    styles: HashMap<String, Style>,
    pub default_paragraph_style: Option<String>,
    pub default_character_style: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ResolvedParagraph {
    pub para: ParaProps,
    pub run: RunProps,
    pub style_name: Option<String>,
}

impl StyleSheet {
    pub fn resolve_paragraph(&self, style_id: Option<&str>) -> ResolvedParagraph {
        let mut run = RunProps::default();
        let mut para = ParaProps::default();
        self.defaults_run.apply(&mut run);
        self.defaults_para.apply(&mut para);

        let initial = style_id
            .map(str::to_string)
            .or_else(|| self.default_paragraph_style.clone());
        let mut chain = Vec::new();
        let mut visited = HashSet::new();
        let mut current = initial.clone();
        let mut depth = 0_u32;
        while let Some(id) = current {
            if depth >= crate::limits::MAX_STYLE_CHAIN || !visited.insert(id.clone()) {
                break;
            }
            depth = depth.saturating_add(1);
            let Some(style) = self.styles.get(&id) else {
                break;
            };
            chain.push(style);
            current = style.based_on.clone();
        }
        chain.reverse();

        let mut style_name = None;
        for style in &chain {
            style.para.apply(&mut para);
            style.run.apply(&mut run);
            if style.name.is_some() {
                style_name = style.name.clone();
            }
        }
        if para.heading_level.is_none() {
            para.heading_level = style_name.as_deref().and_then(heading_level_from_name);
        }
        if para.style_id.is_none() {
            para.style_id = Some(initial.clone().unwrap_or_default());
        }
        ResolvedParagraph {
            para,
            run,
            style_name,
        }
    }

    pub fn resolve_character(&self, style_id: Option<&str>) -> RunProps {
        let mut run = RunProps::default();
        self.defaults_run.apply(&mut run);
        let initial = style_id
            .map(str::to_string)
            .or_else(|| self.default_character_style.clone());
        let mut chain = Vec::new();
        let mut visited = HashSet::new();
        let mut current = initial.clone();
        let mut depth = 0_u32;
        while let Some(id) = current {
            if depth >= crate::limits::MAX_STYLE_CHAIN || !visited.insert(id.clone()) {
                break;
            }
            depth = depth.saturating_add(1);
            let Some(style) = self.styles.get(&id) else {
                break;
            };
            chain.push(style);
            current = style.based_on.clone();
        }
        chain.reverse();
        for style in chain {
            style.run.apply(&mut run);
        }
        run
    }

    pub fn resolve_character_partial(&self, style_id: Option<&str>) -> PartialRunProps {
        let mut result = PartialRunProps::default();
        let mut chain = Vec::new();
        let mut visited = HashSet::new();
        let mut current = style_id.map(str::to_string);
        let mut depth = 0_u32;
        while let Some(id) = current {
            if depth >= crate::limits::MAX_STYLE_CHAIN || !visited.insert(id.clone()) {
                break;
            }
            depth = depth.saturating_add(1);
            let Some(style) = self.styles.get(&id) else {
                break;
            };
            chain.push(style);
            current = style.based_on.clone();
        }
        chain.reverse();
        for style in chain {
            result.merge(&style.run);
        }
        result
    }
}

pub fn parse_styles(xml: Option<&str>, theme: &Theme) -> Result<StyleSheet> {
    let Some(xml) = xml else {
        return Ok(StyleSheet::default());
    };
    let options = ParsingOptions {
        allow_dtd: false,
        nodes_limit: 500_000,
        ..ParsingOptions::default()
    };
    let document = Document::parse_with_options(xml, options).map_err(|_| LoadError::Malformed)?;
    let mut sheet = StyleSheet::default();

    for node in document.descendants().filter(|node| node.is_element()) {
        if node.tag_name().name() == "rPrDefault" {
            if let Some(rpr) = child(node, "rPr") {
                sheet.defaults_run = parse_run_props(rpr, theme);
            }
        } else if node.tag_name().name() == "pPrDefault"
            && let Some(ppr) = child(node, "pPr")
        {
            sheet.defaults_para = parse_para_props(ppr, theme);
        }
    }

    for node in document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "style")
    {
        let Some(id) = attr_w(node, "styleId").or_else(|| node.attribute("styleId")) else {
            continue;
        };
        let style_type = attr_w(node, "type")
            .or_else(|| node.attribute("type"))
            .unwrap_or("paragraph");
        let kind = if style_type.eq_ignore_ascii_case("character") {
            StyleKind::Character
        } else {
            StyleKind::Paragraph
        };
        let name = child(node, "name").and_then(val).map(str::to_string);
        let based_on = child(node, "basedOn").and_then(val).map(str::to_string);
        let is_default = attr_w(node, "default")
            .or_else(|| node.attribute("default"))
            .is_some_and(parse_bool);
        let para = child(node, "pPr")
            .map(|ppr| parse_para_props(ppr, theme))
            .unwrap_or_default();
        let run = child(node, "rPr")
            .map(|rpr| parse_run_props(rpr, theme))
            .unwrap_or_default();
        let style = Style {
            id: id.to_string(),
            kind,
            name,
            based_on,
            is_default,
            para,
            run,
        };
        if is_default {
            match kind {
                StyleKind::Paragraph => sheet.default_paragraph_style = Some(id.to_string()),
                StyleKind::Character => sheet.default_character_style = Some(id.to_string()),
            }
        }
        sheet.styles.insert(id.to_string(), style);
    }

    if sheet.default_paragraph_style.is_none() {
        sheet.default_paragraph_style = sheet
            .styles
            .values()
            .find(|style| {
                style.kind == StyleKind::Paragraph && style.name.as_deref() == Some("Normal")
            })
            .map(|style| style.id.clone());
    }
    if sheet.default_character_style.is_none() {
        sheet.default_character_style = sheet
            .styles
            .values()
            .find(|style| {
                style.kind == StyleKind::Character
                    && style.name.as_deref() == Some("Default Paragraph Font")
            })
            .map(|style| style.id.clone());
    }

    Ok(sheet)
}

pub fn parse_run_props(node: Node<'_, '_>, theme: &Theme) -> PartialRunProps {
    let mut props = PartialRunProps::default();
    for child_node in node.children().filter(|child| child.is_element()) {
        match child_node.tag_name().name() {
            "rStyle" => props.char_style = val(child_node).map(str::to_string),
            "b" => props.bold = Some(bool_element(child_node)),
            "i" => props.italic = Some(bool_element(child_node)),
            "u" => {
                props.underline = Some(val(child_node).map(parse_underline).unwrap_or(true));
            }
            "strike" | "dstrike" => props.strike = Some(bool_element(child_node)),
            "smallCaps" => props.small_caps = Some(bool_element(child_node)),
            "caps" => props.all_caps = Some(bool_element(child_node)),
            "vertAlign" => {
                if let Some(value) = val(child_node) {
                    props.vert = Some(parse_vert_align(value));
                }
            }
            "sz" => {
                if let Some(value) = val(child_node).and_then(parse_u32) {
                    props.size_half_pt = Some(u16::try_from(value).unwrap_or(u16::MAX));
                }
            }
            "color" => {
                if let Some(value) = val(child_node) {
                    if value.eq_ignore_ascii_case("auto") {
                        props.color = None;
                    } else if let Some(color) = parse_hex_rgb(value) {
                        props.color = Some(color);
                    } else if let Some(theme_name) = attr_w(child_node, "themeColor") {
                        props.color = theme.color_for_theme(theme_name);
                    }
                } else if let Some(theme_name) = attr_w(child_node, "themeColor") {
                    props.color = theme.color_for_theme(theme_name);
                }
            }
            "highlight" => {
                if let Some(value) = val(child_node) {
                    props.highlight = highlight_color(value);
                }
            }
            "shd" => {
                if let Some(value) =
                    attr_w(child_node, "fill").or_else(|| child_node.attribute("fill"))
                {
                    props.highlight = parse_hex_rgb(value);
                }
            }
            "rFonts" => {
                let explicit = attr_w(child_node, "ascii")
                    .or_else(|| attr_w(child_node, "hAnsi"))
                    .filter(|value| !value.is_empty());
                if let Some(value) = explicit {
                    props.font = Some(value.to_string());
                } else if let Some(theme_name) =
                    attr_w(child_node, "asciiTheme").or_else(|| attr_w(child_node, "hAnsiTheme"))
                {
                    props.font = theme.font_for_theme(theme_name);
                }
            }
            _ => {}
        }
    }
    props
}

pub fn parse_para_props(node: Node<'_, '_>, theme: &Theme) -> PartialParaProps {
    let mut props = PartialParaProps::default();
    for child_node in node.children().filter(|child| child.is_element()) {
        match child_node.tag_name().name() {
            "pStyle" => props.style_id = val(child_node).map(str::to_string),
            "jc" => {
                if let Some(value) = val(child_node) {
                    props.align = Some(parse_align(value));
                }
            }
            "ind" => {
                if let Some(value) = attr_w(child_node, "left")
                    .or_else(|| attr_w(child_node, "start"))
                    .and_then(parse_i32)
                {
                    props.indent_left_tw = Some(value);
                }
                if let Some(value) = attr_w(child_node, "right")
                    .or_else(|| attr_w(child_node, "end"))
                    .and_then(parse_i32)
                {
                    props.indent_right_tw = Some(value);
                }
                if let Some(value) = attr_w(child_node, "firstLine").and_then(parse_i32) {
                    props.indent_first_tw = Some(value);
                } else if let Some(value) = attr_w(child_node, "hanging").and_then(parse_i32) {
                    props.indent_first_tw = Some(value.saturating_neg());
                }
            }
            "spacing" => {
                if let Some(value) = attr_w(child_node, "before").and_then(parse_u32) {
                    props.space_before_tw = Some(value);
                }
                if let Some(value) = attr_w(child_node, "after").and_then(parse_u32) {
                    props.space_after_tw = Some(value);
                }
                if let Some(value) = attr_w(child_node, "line").and_then(parse_f32) {
                    let _line_rule = attr_w(child_node, "lineRule").unwrap_or("auto");
                    props.line_height = Some((value / 240.0).clamp(0.8, 4.0));
                }
            }
            "shd" => {
                if let Some(value) = attr_w(child_node, "fill") {
                    props.shading = parse_hex_rgb(value);
                }
            }
            "bidi" => props.rtl = Some(bool_element(child_node)),
            "outlineLvl" => {
                if let Some(level) = val(child_node).and_then(parse_u32)
                    && level < 9
                {
                    props.heading_level = Some(u8::try_from(level.saturating_add(1)).unwrap_or(9));
                }
            }
            "tabs" => {
                let mut stops = Vec::new();
                for tab in child_node.children().filter(|child| child.is_element()) {
                    if tab.tag_name().name() != "tab" {
                        continue;
                    }
                    let Some(position) = attr_w(tab, "pos").and_then(parse_i32) else {
                        continue;
                    };
                    let align = match val(tab).unwrap_or("left").to_ascii_lowercase().as_str() {
                        "center" => crate::model::TabAlign::Center,
                        "right" | "end" => crate::model::TabAlign::Right,
                        "decimal" => crate::model::TabAlign::Decimal,
                        _ => crate::model::TabAlign::Left,
                    };
                    stops.push(crate::model::TabStop {
                        align,
                        pos_tw: position,
                    });
                }
                if !stops.is_empty() {
                    props.tab_stops = Some(stops);
                }
            }
            "numPr" => {
                let num_id = child(child_node, "numId")
                    .and_then(val)
                    .map(str::to_string)
                    .filter(|value| value != "0");
                let level = child(child_node, "ilvl")
                    .and_then(val)
                    .and_then(parse_u32)
                    .and_then(|value| u8::try_from(value).ok())
                    .unwrap_or(0)
                    .min(crate::limits::MAX_LIST_LEVELS.saturating_sub(1));
                if num_id.is_some() {
                    props.num_id = num_id;
                    props.num_level = Some(level);
                }
            }
            "rPr" => {
                let run = parse_run_props(child_node, theme);
                if let Some(font) = run.font {
                    let _ = font;
                }
            }
            _ => {}
        }
    }
    props
}

fn child<'a, 'input>(node: Node<'a, 'input>, local: &str) -> Option<Node<'a, 'input>> {
    node.children()
        .find(|child| child.is_element() && child.tag_name().name() == local)
}

fn heading_level_from_name(name: &str) -> Option<u8> {
    let lower = name.trim().to_ascii_lowercase();
    if lower == "title" {
        return Some(1);
    }
    if lower.starts_with("heading") {
        let digits: String = lower.chars().filter(char::is_ascii_digit).collect();
        if let Ok(level) = digits.parse::<u8>()
            && (1..=9).contains(&level)
        {
            return Some(level);
        }
    }
    None
}

fn highlight_color(value: &str) -> Option<Rgb> {
    match value.trim().to_ascii_lowercase().as_str() {
        "yellow" => Some(Rgb::new(255, 255, 0)),
        "green" => Some(Rgb::new(0, 255, 0)),
        "cyan" => Some(Rgb::new(0, 255, 255)),
        "magenta" => Some(Rgb::new(255, 0, 255)),
        "blue" => Some(Rgb::new(0, 0, 255)),
        "red" => Some(Rgb::new(255, 0, 0)),
        "darkblue" => Some(Rgb::new(0, 0, 139)),
        "darkcyan" => Some(Rgb::new(0, 139, 139)),
        "darkgreen" => Some(Rgb::new(0, 100, 0)),
        "darkmagenta" => Some(Rgb::new(139, 0, 139)),
        "darkred" => Some(Rgb::new(139, 0, 0)),
        "darkyellow" => Some(Rgb::new(128, 128, 0)),
        "darkgray" | "darkgrey" => Some(Rgb::new(169, 169, 169)),
        "lightgray" | "lightgrey" => Some(Rgb::new(211, 211, 211)),
        "black" => Some(Rgb::new(0, 0, 0)),
        "white" => Some(Rgb::new(255, 255, 255)),
        "none" => None,
        _ => None,
    }
}
