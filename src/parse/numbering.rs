use std::collections::HashMap;

use roxmltree::{Document, Node, ParsingOptions};

use crate::error::{LoadError, Result};
use crate::limits::MAX_LIST_LEVELS;
use crate::model::ListLabel;
use crate::xmlutil::{attr_w, parse_u32, val};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NumFmt {
    #[default]
    Decimal,
    Bullet,
    LowerLetter,
    UpperLetter,
    LowerRoman,
    UpperRoman,
    None,
}

#[derive(Clone, Debug)]
pub struct Level {
    pub start: u32,
    pub num_fmt: NumFmt,
    pub lvl_text: String,
    pub restart: Option<u8>,
    pub font: Option<String>,
}

impl Default for Level {
    fn default() -> Self {
        Self {
            start: 1,
            num_fmt: NumFmt::Decimal,
            lvl_text: String::new(),
            restart: None,
            font: None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct AbstractNum {
    pub levels: Vec<Level>,
}

#[derive(Clone, Debug, Default)]
pub struct NumOverride {
    pub start_override: Option<u32>,
    pub level: Option<Level>,
}

#[derive(Clone, Debug, Default)]
pub struct Num {
    pub abstract_id: String,
    pub overrides: HashMap<u8, NumOverride>,
}

#[derive(Clone, Debug, Default)]
pub struct Numbering {
    abstract_nums: HashMap<String, AbstractNum>,
    nums: HashMap<String, Num>,
    counters: HashMap<(String, u8), u32>,
}

impl Numbering {
    pub fn label(&mut self, num_id: &str, level: u8) -> Option<ListLabel> {
        let level = level.min(MAX_LIST_LEVELS.saturating_sub(1));
        let num = self.nums.get(num_id)?;
        let abstract_num = self.abstract_nums.get(&num.abstract_id)?;
        let definition = num
            .overrides
            .get(&level)
            .and_then(|override_value| override_value.level.as_ref())
            .or_else(|| abstract_num.levels.get(usize::from(level)))?;
        if definition.num_fmt == NumFmt::None {
            return None;
        }
        let start = num
            .overrides
            .get(&level)
            .and_then(|override_value| override_value.start_override)
            .unwrap_or(definition.start);
        let key = (num_id.to_string(), level);
        let current = self
            .counters
            .get(&key)
            .copied()
            .unwrap_or(start.saturating_sub(1));
        let next = current.saturating_add(1).min(99_999);
        self.counters.insert(key, next);
        for deeper in (level.saturating_add(1))..MAX_LIST_LEVELS {
            self.counters.remove(&(num_id.to_string(), deeper));
        }
        let label = if definition.num_fmt == NumFmt::Bullet {
            bullet_text(&definition.lvl_text, definition.font.as_deref())
        } else {
            format_label(
                &definition.lvl_text,
                definition.num_fmt,
                next,
                level,
                &self.counters,
                num_id,
            )
        };
        Some(ListLabel { text: label, level })
    }
}

pub fn parse_numbering(xml: Option<&str>) -> Result<Numbering> {
    let Some(xml) = xml else {
        return Ok(Numbering::default());
    };
    let options = ParsingOptions {
        allow_dtd: false,
        nodes_limit: 500_000,
        ..ParsingOptions::default()
    };
    let document = Document::parse_with_options(xml, options).map_err(|_| LoadError::Malformed)?;
    let mut numbering = Numbering::default();

    for node in document.descendants().filter(|node| node.is_element()) {
        if node.tag_name().name() == "abstractNum" {
            let Some(id) = attr_w(node, "abstractNumId")
                .or_else(|| node.attribute("abstractNumId"))
                .map(str::to_string)
            else {
                continue;
            };
            let mut abstract_num = AbstractNum::default();
            for lvl in node.children().filter(|child| is_lvl(child)) {
                let level = parse_level(lvl);
                let index = attr_w(lvl, "ilvl")
                    .or_else(|| lvl.attribute("ilvl"))
                    .and_then(parse_u32)
                    .and_then(|value| usize::try_from(value).ok())
                    .unwrap_or(abstract_num.levels.len());
                while abstract_num.levels.len() <= index {
                    abstract_num.levels.push(Level::default());
                }
                if let Some(slot) = abstract_num.levels.get_mut(index) {
                    *slot = level;
                }
            }
            numbering.abstract_nums.insert(id, abstract_num);
        } else if node.tag_name().name() == "num" {
            let Some(id) = attr_w(node, "numId")
                .or_else(|| node.attribute("numId"))
                .map(str::to_string)
            else {
                continue;
            };
            let abstract_id = node
                .children()
                .find(|child| child.is_element() && child.tag_name().name() == "abstractNumId")
                .and_then(val)
                .unwrap_or_default()
                .to_string();
            let mut num = Num {
                abstract_id,
                overrides: HashMap::new(),
            };
            for override_node in node
                .children()
                .filter(|child| child.is_element() && child.tag_name().name() == "lvlOverride")
            {
                let Some(level) = attr_w(override_node, "ilvl")
                    .or_else(|| override_node.attribute("ilvl"))
                    .and_then(parse_u32)
                    .and_then(|value| u8::try_from(value).ok())
                else {
                    continue;
                };
                let start_override = override_node
                    .children()
                    .find(|child| child.is_element() && child.tag_name().name() == "startOverride")
                    .and_then(val)
                    .and_then(parse_u32);
                let level_definition = override_node
                    .children()
                    .find(|child| is_lvl(child))
                    .map(parse_level);
                num.overrides.insert(
                    level.min(MAX_LIST_LEVELS.saturating_sub(1)),
                    NumOverride {
                        start_override,
                        level: level_definition,
                    },
                );
            }
            numbering.nums.insert(id, num);
        }
    }

    Ok(numbering)
}

fn is_lvl(node: &Node<'_, '_>) -> bool {
    node.is_element() && node.tag_name().name() == "lvl"
}

fn parse_level(node: Node<'_, '_>) -> Level {
    let mut level = Level::default();
    if let Some(value) = child_val(node, "start").and_then(parse_u32) {
        level.start = value;
    }
    if let Some(value) = child_val(node, "numFmt") {
        level.num_fmt = parse_num_fmt(value);
    }
    if let Some(value) = child_val(node, "lvlText") {
        level.lvl_text = value.to_string();
    }
    if let Some(value) = child_val(node, "lvlRestart").and_then(parse_u32) {
        level.restart = u8::try_from(value).ok();
    }
    if let Some(rpr) = node
        .children()
        .find(|child| child.is_element() && child.tag_name().name() == "rPr")
        && let Some(fonts) = rpr
            .children()
            .find(|child| child.is_element() && child.tag_name().name() == "rFonts")
    {
        level.font = attr_w(fonts, "ascii")
            .or_else(|| attr_w(fonts, "hAnsi"))
            .map(str::to_string);
    }
    level
}

fn child_val<'a>(node: Node<'a, 'a>, name: &str) -> Option<&'a str> {
    node.children()
        .find(|child| child.is_element() && child.tag_name().name() == name)
        .and_then(val)
}

fn parse_num_fmt(value: &str) -> NumFmt {
    match value.trim().to_ascii_lowercase().as_str() {
        "bullet" => NumFmt::Bullet,
        "lowerletter" => NumFmt::LowerLetter,
        "upperletter" => NumFmt::UpperLetter,
        "lowerroman" => NumFmt::LowerRoman,
        "upperroman" => NumFmt::UpperRoman,
        "none" => NumFmt::None,
        _ => NumFmt::Decimal,
    }
}

fn format_label(
    lvl_text: &str,
    num_fmt: NumFmt,
    current: u32,
    current_level: u8,
    counters: &HashMap<(String, u8), u32>,
    num_id: &str,
) -> String {
    if lvl_text.is_empty() {
        return format_number(num_fmt, current);
    }
    let mut out = String::new();
    let mut chars = lvl_text.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '%' {
            if let Some(digit) = chars.next()
                && let Some(level) = digit.to_digit(10)
            {
                let index = u8::try_from(level.saturating_sub(1)).unwrap_or(0);
                let value = counters
                    .get(&(num_id.to_string(), index))
                    .copied()
                    .unwrap_or(if index == current_level { current } else { 0 });
                out.push_str(&format_number(num_fmt, value));
                continue;
            }
            out.push('%');
        } else {
            out.push(character);
        }
    }
    out
}

fn format_number(format: NumFmt, value: u32) -> String {
    match format {
        NumFmt::Bullet => "•".to_string(),
        NumFmt::LowerLetter => letters(value, false),
        NumFmt::UpperLetter => letters(value, true),
        NumFmt::LowerRoman => roman(value, false),
        NumFmt::UpperRoman => roman(value, true),
        NumFmt::None => String::new(),
        NumFmt::Decimal => value.to_string(),
    }
}

fn letters(value: u32, upper: bool) -> String {
    if value == 0 {
        return "0".to_string();
    }
    let mut value = value;
    let mut out = String::new();
    while value > 0 {
        let remainder = ((value.saturating_sub(1)) % 26) as u8;
        let character = char::from(b'a'.saturating_add(remainder));
        out.insert(
            0,
            if upper {
                character.to_ascii_uppercase()
            } else {
                character
            },
        );
        value = value.saturating_sub(1) / 26;
    }
    out
}

fn roman(value: u32, upper: bool) -> String {
    if value == 0 {
        return "0".to_string();
    }
    let mut value = value.min(99_999);
    let table = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut out = String::new();
    for (amount, text) in table {
        while value >= amount {
            out.push_str(text);
            value = value.saturating_sub(amount);
        }
    }
    if upper { out.to_ascii_uppercase() } else { out }
}

fn is_private_use(character: char) -> bool {
    matches!(character,
        '\u{E000}'..='\u{F8FF}'
        | '\u{F0000}'..='\u{FFFFD}'
        | '\u{100000}'..='\u{10FFFD}')
}

fn bullet_text(text: &str, font: Option<&str>) -> String {
    let first = text.chars().next().unwrap_or('\u{F0B7}');
    let mapped = match (font, first) {
        (Some(font), '\u{F0B7}') if font.eq_ignore_ascii_case("Symbol") => '•',
        (Some(font), '\u{F0A7}') if font.eq_ignore_ascii_case("Wingdings") => '▪',
        (Some(font), '\u{F0D8}') if font.eq_ignore_ascii_case("Wingdings") => '➢',
        (Some(font), '\u{F0FC}') if font.eq_ignore_ascii_case("Wingdings") => '✓',
        (Some(font), 'o') if font.eq_ignore_ascii_case("Courier New") => '◦',
        (_, character) if is_private_use(character) => '•',
        (_, character) => character,
    };
    mapped.to_string()
}
