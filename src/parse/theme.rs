use std::collections::HashMap;

use roxmltree::{Document, Node, ParsingOptions};

use crate::model::Rgb;
use crate::xmlutil::parse_hex_rgb;

#[derive(Clone, Debug, Default)]
pub struct Theme {
    pub major_font: Option<String>,
    pub minor_font: Option<String>,
    pub colors: HashMap<String, Rgb>,
}

impl Theme {
    pub fn font_for_theme(&self, name: &str) -> Option<String> {
        match name {
            "majorHAnsi" | "majorAscii" | "majorEastAsia" | "majorBidi" => self.major_font.clone(),
            "minorHAnsi" | "minorAscii" | "minorEastAsia" | "minorBidi" => self.minor_font.clone(),
            _ => None,
        }
    }

    pub fn color_for_theme(&self, name: &str) -> Option<Rgb> {
        self.colors.get(name).copied().or_else(|| match name {
            "text1" | "dk1" => Some(Rgb::new(0, 0, 0)),
            "background1" | "lt1" => Some(Rgb::new(255, 255, 255)),
            "text2" | "dk2" => Some(Rgb::new(0x44, 0x54, 0x6A)),
            "background2" | "lt2" => Some(Rgb::new(0xE7, 0xE6, 0xE6)),
            _ => None,
        })
    }
}

pub fn parse_theme(xml: &str) -> Option<Theme> {
    let options = ParsingOptions {
        allow_dtd: false,
        nodes_limit: 100_000,
        ..ParsingOptions::default()
    };
    let document = Document::parse_with_options(xml, options).ok()?;
    let mut theme = Theme::default();
    for node in document.descendants().filter(|node| node.is_element()) {
        if node.tag_name().name() == "clrScheme" {
            for color_node in node.children().filter(|child| child.is_element()) {
                let name = color_node.tag_name().name().to_string();
                if let Some(value) = color_value(color_node) {
                    theme.colors.insert(name, value);
                }
            }
        } else if node.tag_name().name() == "majorFont" {
            if theme.major_font.is_none() {
                theme.major_font = latin_typeface(node);
            }
        } else if node.tag_name().name() == "minorFont" && theme.minor_font.is_none() {
            theme.minor_font = latin_typeface(node);
        }
    }
    if theme.colors.is_empty() && theme.major_font.is_none() && theme.minor_font.is_none() {
        None
    } else {
        Some(theme)
    }
}

fn color_value(node: Node<'_, '_>) -> Option<Rgb> {
    for child in node.children().filter(|child| child.is_element()) {
        match child.tag_name().name() {
            "srgbClr" => {
                if let Some(value) = child.attribute("val") {
                    return parse_hex_rgb(value);
                }
            }
            "sysClr" => {
                if let Some(value) = child.attribute("lastClr") {
                    return parse_hex_rgb(value);
                }
            }
            _ => {}
        }
    }
    None
}

fn latin_typeface(node: Node<'_, '_>) -> Option<String> {
    node.children()
        .filter(|child| child.is_element() && child.tag_name().name() == "latin")
        .find_map(|child| child.attribute("typeface").map(str::to_string))
}

pub fn default_theme() -> Theme {
    let mut colors = HashMap::new();
    colors.insert("accent1".to_string(), Rgb::new(0x44, 0x72, 0xC4));
    colors.insert("accent2".to_string(), Rgb::new(0xED, 0x7D, 0x31));
    colors.insert("accent3".to_string(), Rgb::new(0xA5, 0xA5, 0xA5));
    colors.insert("accent4".to_string(), Rgb::new(0xFF, 0xC0, 0x00));
    colors.insert("accent5".to_string(), Rgb::new(0x5B, 0x9B, 0xD5));
    colors.insert("accent6".to_string(), Rgb::new(0x70, 0xAD, 0x47));
    colors.insert("hyperlink".to_string(), Rgb::new(0x05, 0x63, 0xC1));
    colors.insert("followedHyperlink".to_string(), Rgb::new(0x95, 0x4F, 0x72));
    Theme {
        major_font: Some("Cambria".to_string()),
        minor_font: Some("Calibri".to_string()),
        colors,
    }
}
