use std::collections::HashSet;
use std::sync::OnceLock;

use gtk::pango;
use gtk::prelude::*;

fn installed_families() -> &'static HashSet<String> {
    static FAMILIES: OnceLock<HashSet<String>> = OnceLock::new();
    FAMILIES.get_or_init(|| {
        let mut set = HashSet::new();
        let context = pango::Context::new();
        for family in context.list_families() {
            let name = family.name().to_string();
            if !name.is_empty() {
                set.insert(name.to_ascii_lowercase());
            }
        }
        set
    })
}

pub fn resolve_font(requested: Option<&str>) -> Option<String> {
    let families = installed_families();
    let requested = requested.unwrap_or_default();
    let lower = requested.to_ascii_lowercase();
    let candidates: &[&str] = match lower.as_str() {
        "calibri" => &["Carlito", "Calibri", "Sans"],
        "cambria" => &["Caladea", "Cambria", "Serif"],
        "times new roman" => &["Liberation Serif", "Tinos", "Times New Roman", "Serif"],
        "arial" => &["Liberation Sans", "Arimo", "Arial", "Sans"],
        "courier new" => &["Liberation Mono", "Cousine", "Courier New", "Monospace"],
        _ => &[],
    };
    for candidate in candidates {
        if families.contains(&candidate.to_ascii_lowercase()) {
            return Some((*candidate).to_string());
        }
    }
    if !requested.is_empty() && families.contains(&lower) {
        return Some(requested.to_string());
    }
    if families.contains("sans") {
        Some("Sans".to_string())
    } else {
        None
    }
}
