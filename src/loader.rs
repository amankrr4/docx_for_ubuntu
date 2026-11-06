use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use roxmltree::{Document, ParsingOptions};
use zip::ZipArchive;
use zip::result::ZipError;

use crate::error::{LoadError, Result};
use crate::limits::{
    MAX_COMPRESSION_RATIO, MAX_FILE_BYTES, MAX_TOTAL_DECOMPRESSED_BYTES, MAX_XML_PART_BYTES,
    MAX_ZIP_ENTRIES,
};
use crate::model::Stats;
use crate::parse::rels::{Rels, base_dir, normalize_target, parse_rels, part_rels_path};

#[derive(Debug)]
pub struct HeaderFooterPart {
    pub name: String,
    pub xml: String,
    pub rels: Rels,
}

#[derive(Debug)]
pub struct Package {
    pub(crate) archive: ZipArchive<File>,
    pub main_part: String,
    pub main_xml: String,
    pub main_rels: Rels,
    pub styles_xml: Option<String>,
    pub numbering_xml: Option<String>,
    pub theme_xml: Option<String>,
    pub header_footer_parts: Vec<HeaderFooterPart>,
    pub stats: Stats,
}

impl Package {
    pub fn read_binary(&mut self, name: &str, limit: u64) -> Result<Vec<u8>> {
        read_entry(&mut self.archive, name, limit)
    }
}

pub fn load_package(path: &Path) -> Result<Package> {
    let metadata = std::fs::metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            LoadError::NotAFile
        } else {
            LoadError::Io(error.kind())
        }
    })?;
    if !metadata.is_file() {
        return Err(LoadError::NotAFile);
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err(LoadError::TooLarge);
    }
    let mut file = File::open(path).map_err(LoadError::from)?;
    let mut magic = [0_u8; 8];
    let read = file.read(&mut magic).map_err(LoadError::from)?;
    if read < 4 {
        return Err(LoadError::NotZip);
    }
    file.seek(SeekFrom::Start(0)).map_err(LoadError::from)?;
    if magic.starts_with(b"\xD0\xCF\x11\xE0") {
        return Err(LoadError::LegacyOrEncrypted);
    }
    if !magic.starts_with(b"PK\x03\x04") {
        return Err(LoadError::NotZip);
    }

    let mut archive = ZipArchive::new(file).map_err(map_zip_error)?;
    if archive.len() > MAX_ZIP_ENTRIES {
        return Err(LoadError::Malformed);
    }
    if let Some(total) = archive.decompressed_size()
        && total > u128::from(MAX_TOTAL_DECOMPRESSED_BYTES)
    {
        return Err(LoadError::TooLarge);
    }
    if archive.has_overlapping_files().map_err(map_zip_error)? {
        return Err(LoadError::Malformed);
    }
    if archive.validate_header_consistency().is_err() {
        return Err(LoadError::Malformed);
    }

    let root_rels_xml = read_entry(&mut archive, "_rels/.rels", MAX_XML_PART_BYTES)?;
    let root_rels_text = String::from_utf8(root_rels_xml).map_err(|_| LoadError::Malformed)?;
    let root_rels = parse_rels(&root_rels_text, "")?;
    let main_rel = root_rels
        .find_by_type_suffix("/officeDocument")
        .ok_or(LoadError::NotWordDocument)?;
    if main_rel.external {
        return Err(LoadError::NotWordDocument);
    }
    let main_part = main_rel.target.clone();
    let main_xml_bytes = read_entry(&mut archive, &main_part, MAX_XML_PART_BYTES)?;
    let main_xml = String::from_utf8(main_xml_bytes).map_err(|_| LoadError::Malformed)?;

    check_content_type(&mut archive, &main_part)?;

    let main_dir = base_dir(&main_part).to_string();
    let main_rels = match part_rels_path(&main_part) {
        Some(path) => {
            let xml = read_optional_entry(&mut archive, &path, MAX_XML_PART_BYTES)?;
            match xml {
                Some(bytes) => {
                    let text = String::from_utf8(bytes).map_err(|_| LoadError::Malformed)?;
                    parse_rels(&text, &main_dir)?
                }
                None => Rels::default(),
            }
        }
        None => Rels::default(),
    };

    let styles_xml =
        read_related_xml(&mut archive, &main_rels, "/styles", &main_dir, "styles.xml")?;
    let numbering_xml = read_related_xml(
        &mut archive,
        &main_rels,
        "/numbering",
        &main_dir,
        "numbering.xml",
    )?;
    let theme_xml = read_related_xml(
        &mut archive,
        &main_rels,
        "/theme",
        &main_dir,
        "theme/theme1.xml",
    )?;

    let mut header_footer_parts = Vec::new();
    for relationship in main_rels.iter() {
        if relationship.external {
            continue;
        }
        let is_header = relationship.rel_type.ends_with("/header");
        let is_footer = relationship.rel_type.ends_with("/footer");
        if !is_header && !is_footer {
            continue;
        }
        let Some(bytes) =
            read_optional_entry(&mut archive, &relationship.target, MAX_XML_PART_BYTES)?
        else {
            continue;
        };
        let xml = String::from_utf8(bytes).map_err(|_| LoadError::Malformed)?;
        let rels = match part_rels_path(&relationship.target) {
            Some(path) => match read_optional_entry(&mut archive, &path, MAX_XML_PART_BYTES)? {
                Some(rels_bytes) => {
                    let rels_text =
                        String::from_utf8(rels_bytes).map_err(|_| LoadError::Malformed)?;
                    parse_rels(&rels_text, base_dir(&relationship.target))?
                }
                None => Rels::default(),
            },
            None => Rels::default(),
        };
        header_footer_parts.push(HeaderFooterPart {
            name: relationship.target.clone(),
            xml,
            rels,
        });
    }

    let stats = Stats {
        nodes: 0,
        xml_bytes: 0,
        images: 0,
        table_cells: 0,
    };
    Ok(Package {
        archive,
        main_part,
        main_xml,
        main_rels,
        styles_xml,
        numbering_xml,
        theme_xml,
        header_footer_parts,
        stats,
    })
}

fn read_related_xml(
    archive: &mut ZipArchive<File>,
    rels: &Rels,
    suffix: &str,
    base_dir: &str,
    fallback: &str,
) -> Result<Option<String>> {
    let target = rels
        .find_by_type_suffix(suffix)
        .filter(|relationship| !relationship.external)
        .map(|relationship| relationship.target.clone())
        .or_else(|| {
            if base_dir.is_empty() {
                Some(fallback.to_string())
            } else {
                normalize_target(base_dir, fallback)
            }
        });
    let Some(target) = target else {
        return Ok(None);
    };
    match read_optional_entry(archive, &target, MAX_XML_PART_BYTES)? {
        Some(bytes) => String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| LoadError::Malformed),
        None => Ok(None),
    }
}

fn check_content_type(archive: &mut ZipArchive<File>, main_part: &str) -> Result<()> {
    let Some(bytes) = read_optional_entry(archive, "[Content_Types].xml", MAX_XML_PART_BYTES)?
    else {
        return Err(LoadError::NotWordDocument);
    };
    let xml = String::from_utf8(bytes).map_err(|_| LoadError::Malformed)?;
    let options = ParsingOptions {
        allow_dtd: false,
        nodes_limit: 200_000,
        ..ParsingOptions::default()
    };
    let document = Document::parse_with_options(&xml, options).map_err(|_| LoadError::Malformed)?;
    let wanted = format!("/{main_part}");
    for node in document.descendants().filter(|node| node.is_element()) {
        if node.tag_name().name() != "Override" {
            continue;
        }
        if node.attribute("PartName") != Some(wanted.as_str()) {
            continue;
        }
        let content_type = node.attribute("ContentType").unwrap_or_default();
        if content_type.contains("wordprocessingml") || content_type.contains("ms-word") {
            return Ok(());
        }
    }
    Err(LoadError::NotWordDocument)
}

fn read_entry(archive: &mut ZipArchive<File>, name: &str, limit: u64) -> Result<Vec<u8>> {
    let file = archive.by_name(name).map_err(map_zip_error)?;
    let declared = file.size();
    let compressed = file.compressed_size();
    if declared > limit {
        return Err(LoadError::TooLarge);
    }
    if compressed == 0 {
        if declared > 0 {
            return Err(LoadError::Malformed);
        }
    } else if declared / compressed > MAX_COMPRESSION_RATIO {
        return Err(LoadError::Malformed);
    }
    let mut data = Vec::new();
    let mut limited = file.take(limit.saturating_add(1));
    limited.read_to_end(&mut data).map_err(LoadError::from)?;
    if u64::try_from(data.len()).unwrap_or(u64::MAX) > limit {
        return Err(LoadError::TooLarge);
    }
    Ok(data)
}

fn read_optional_entry(
    archive: &mut ZipArchive<File>,
    name: &str,
    limit: u64,
) -> Result<Option<Vec<u8>>> {
    match archive.by_name(name) {
        Ok(_) => {}
        Err(ZipError::FileNotFound) => return Ok(None),
        Err(error) => return Err(map_zip_error(error)),
    }
    read_entry(archive, name, limit).map(Some)
}

fn map_zip_error(error: ZipError) -> LoadError {
    match error {
        ZipError::FileNotFound => LoadError::Malformed,
        ZipError::InvalidArchive(_) => LoadError::NotZip,
        _ => LoadError::Malformed,
    }
}
