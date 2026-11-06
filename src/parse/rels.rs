use std::collections::HashMap;

use roxmltree::Document;
use roxmltree::ParsingOptions;

use crate::error::{LoadError, Result};

#[derive(Clone, Debug)]
pub struct Relationship {
    pub id: String,
    pub target: String,
    pub rel_type: String,
    pub external: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Rels {
    map: HashMap<String, Relationship>,
}

impl Rels {
    pub fn get(&self, id: &str) -> Option<&Relationship> {
        self.map.get(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Relationship> {
        self.map.values()
    }

    pub fn find_by_type_suffix(&self, suffix: &str) -> Option<&Relationship> {
        self.iter()
            .find(|relationship| relationship.rel_type.ends_with(suffix))
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

pub fn parse_rels(xml: &str, base_dir: &str) -> Result<Rels> {
    let options = ParsingOptions {
        allow_dtd: false,
        nodes_limit: 100_000,
        ..ParsingOptions::default()
    };
    let document = Document::parse_with_options(xml, options).map_err(|_| LoadError::Malformed)?;
    let mut map = HashMap::new();
    for node in document.descendants().filter(|node| node.is_element()) {
        if node.tag_name().name() != "Relationship" {
            continue;
        }
        let Some(id) = node.attribute("Id") else {
            continue;
        };
        let Some(target) = node.attribute("Target") else {
            continue;
        };
        let rel_type = node.attribute("Type").unwrap_or_default().to_string();
        let external = node
            .attribute("TargetMode")
            .is_some_and(|value| value.eq_ignore_ascii_case("External"));
        let normalized = if external {
            target.to_string()
        } else {
            let Some(normalized) = normalize_target(base_dir, target) else {
                continue;
            };
            normalized
        };
        map.insert(
            id.to_string(),
            Relationship {
                id: id.to_string(),
                target: normalized,
                rel_type,
                external,
            },
        );
    }
    Ok(Rels { map })
}

pub fn normalize_target(base_dir: &str, target: &str) -> Option<String> {
    let target = target.replace('\\', "/");
    let combined = if target.starts_with('/') {
        target.trim_start_matches('/').to_string()
    } else if base_dir.is_empty() {
        target
    } else {
        format!("{}/{}", base_dir.trim_end_matches('/'), target)
    };
    let mut parts: Vec<&str> = Vec::new();
    for segment in combined.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(segment),
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

pub fn base_dir(part: &str) -> &str {
    part.rsplit_once('/').map_or("", |(dir, _)| dir)
}

pub fn file_name(part: &str) -> &str {
    part.rsplit_once('/').map_or(part, |(_, name)| name)
}

pub fn part_rels_path(part: &str) -> Option<String> {
    let dir = base_dir(part);
    let name = file_name(part);
    if name.is_empty() {
        None
    } else if dir.is_empty() {
        Some(format!("_rels/{name}.rels"))
    } else {
        Some(format!("{dir}/_rels/{name}.rels"))
    }
}
