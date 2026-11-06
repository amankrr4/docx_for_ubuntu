use roxmltree::Node;

use crate::model::{Align, Rgb, VertAlign};

pub const NS_W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
pub const NS_W_STRICT: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";
pub const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
pub const NS_R_STRICT: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships";
pub const NS_WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
pub const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
pub const NS_MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
pub const NS_V: &str = "urn:schemas-microsoft-com:vml";
pub const NS_WPS: &str = "http://schemas.microsoft.com/office/word/2010/wordprocessingShape";
pub const NS_XML: &str = "http://www.w3.org/XML/1998/namespace";

pub fn is_w(node: Node<'_, '_>, local: &str) -> bool {
    node.tag_name().name() == local
        && matches!(node.tag_name().namespace(), Some(NS_W | NS_W_STRICT))
}

pub fn is_ns(node: Node<'_, '_>, transitional: &str, strict: &str, local: &str) -> bool {
    node.tag_name().name() == local
        && matches!(node.tag_name().namespace(), Some(ns) if ns == transitional || ns == strict)
}

pub fn is_r(node: Node<'_, '_>, local: &str) -> bool {
    is_ns(node, NS_R, NS_R_STRICT, local)
}

pub fn is_a(node: Node<'_, '_>, local: &str) -> bool {
    is_ns(node, NS_A, NS_A, local)
}

pub fn is_wp(node: Node<'_, '_>, local: &str) -> bool {
    is_ns(node, NS_WP, NS_WP, local)
}

pub fn is_mc(node: Node<'_, '_>, local: &str) -> bool {
    is_ns(node, NS_MC, NS_MC, local)
}

pub fn is_v(node: Node<'_, '_>, local: &str) -> bool {
    is_ns(node, NS_V, NS_V, local)
}

pub fn attr_w<'a>(node: Node<'a, 'a>, name: &str) -> Option<&'a str> {
    node.attribute((NS_W, name))
        .or_else(|| node.attribute((NS_W_STRICT, name)))
}

pub fn attr_r<'a>(node: Node<'a, 'a>, name: &str) -> Option<&'a str> {
    node.attribute((NS_R, name))
        .or_else(|| node.attribute((NS_R_STRICT, name)))
}

pub fn attr<'a>(node: Node<'a, 'a>, name: &str) -> Option<&'a str> {
    node.attribute(name)
}

pub fn attr_local<'a>(node: Node<'a, 'a>, name: &str) -> Option<&'a str> {
    node.attributes()
        .find(|attribute| attribute.name() == name)
        .map(|attribute| attribute.value())
}

pub fn val<'a>(node: Node<'a, 'a>) -> Option<&'a str> {
    attr_w(node, "val").or_else(|| attr(node, "val"))
}

pub fn bool_element(node: Node<'_, '_>) -> bool {
    match val(node) {
        Some(value) => parse_bool(value),
        None => true,
    }
}

pub fn parse_bool(value: &str) -> bool {
    !matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "0" | "false" | "off" | "none" | "no"
    )
}

pub fn parse_i32(value: &str) -> Option<i32> {
    value.trim().parse::<i32>().ok()
}

pub fn parse_u32(value: &str) -> Option<u32> {
    value.trim().parse::<u32>().ok()
}

pub fn parse_f32(value: &str) -> Option<f32> {
    value.trim().parse::<f32>().ok()
}

pub fn parse_hex_rgb(value: &str) -> Option<Rgb> {
    let value = value.trim().trim_start_matches('#');
    if value.len() != 6 || !value.is_ascii() {
        return None;
    }
    let r = u8::from_str_radix(value.get(0..2)?, 16).ok()?;
    let g = u8::from_str_radix(value.get(2..4)?, 16).ok()?;
    let b = u8::from_str_radix(value.get(4..6)?, 16).ok()?;
    Some(Rgb::new(r, g, b))
}

pub fn parse_color(node: Node<'_, '_>) -> Option<Rgb> {
    let value = val(node)?;
    if value.eq_ignore_ascii_case("auto") {
        None
    } else {
        parse_hex_rgb(value)
    }
}

pub fn parse_align(value: &str) -> Align {
    match value.trim().to_ascii_lowercase().as_str() {
        "center" => Align::Center,
        "right" | "end" => Align::End,
        "both" | "distribute" | "justify" => Align::Justify,
        _ => Align::Start,
    }
}

pub fn parse_vert_align(value: &str) -> VertAlign {
    match value.trim().to_ascii_lowercase().as_str() {
        "superscript" | "super" => VertAlign::Super,
        "subscript" | "sub" => VertAlign::Sub,
        _ => VertAlign::Baseline,
    }
}

pub fn parse_underline(value: &str) -> bool {
    !matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "none" | "0" | "false" | "off"
    )
}

pub fn twips_to_px_i32(twips: i32, zoom: f64) -> i32 {
    (f64::from(twips) / 20.0 * (96.0 / 72.0) * zoom).round() as i32
}

pub fn twips_to_px_u32(twips: u32, zoom: f64) -> i32 {
    (f64::from(twips) / 20.0 * (96.0 / 72.0) * zoom).round() as i32
}

pub fn emu_to_px(emu: i64) -> u32 {
    let px = (emu as f64 / 914_400.0 * 96.0).round();
    if px <= 0.0 {
        0
    } else if px >= f64::from(u32::MAX) {
        u32::MAX
    } else {
        px as u32
    }
}

pub fn half_points_to_points(half_points: u16) -> f32 {
    f32::from(half_points) / 2.0
}

pub fn xml_preserve(node: Node<'_, '_>) -> bool {
    node.attribute((NS_XML, "space"))
        .is_some_and(|value| value.eq_ignore_ascii_case("preserve"))
}

pub fn normalize_text(text: &str, _preserve: bool) -> String {
    let mut normalized = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\r' => {}
            '\0' => {}
            '\t' | '\n' => normalized.push(character),
            c if c.is_control() => {}
            c => normalized.push(c),
        }
    }
    normalized
}
