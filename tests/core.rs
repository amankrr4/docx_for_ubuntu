#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::io::{Cursor, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use docxlite::error::LoadError;
use docxlite::load_document;
use docxlite::model::Document;
use zip::CompressionMethod;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

static FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_path(suffix: &str) -> PathBuf {
    let counter = FILE_COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "docxlite-test-{}-{counter}-{suffix}",
        std::process::id()
    ))
}

fn load_bytes(bytes: &[u8]) -> Result<Document, LoadError> {
    let path = temp_path("docx");
    std::fs::write(&path, bytes).expect("write temp docx");
    let result = load_document(&path, Arc::new(AtomicBool::new(false)));
    let _ = std::fs::remove_file(&path);
    result
}

fn build_docx(document_xml: &str, header: Option<&str>, footer: Option<&str>) -> Vec<u8> {
    let mut files: Vec<(String, String)> = Vec::new();
    let mut content_types = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#,
    );
    let rels = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#,
    );
    let mut doc_rels = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
</Relationships>"#,
    );
    if let Some(header) = header {
        content_types = content_types.replace(
            "</Types>",
            r#"<Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/></Types>"#,
        );
        doc_rels = doc_rels.replace(
            "</Relationships>",
            r#"<Relationship Id="rIdHeader" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/></Relationships>"#,
        );
        files.push(("word/header1.xml".to_string(), header.to_string()));
    }
    if let Some(footer) = footer {
        content_types = content_types.replace(
            "</Types>",
            r#"<Override PartName="/word/footer1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/></Types>"#,
        );
        doc_rels = doc_rels.replace(
            "</Relationships>",
            r#"<Relationship Id="rIdFooter" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer1.xml"/></Relationships>"#,
        );
        files.push(("word/footer1.xml".to_string(), footer.to_string()));
    }
    files.push(("[Content_Types].xml".to_string(), content_types));
    files.push(("_rels/.rels".to_string(), rels));
    files.push(("word/_rels/document.xml.rels".to_string(), doc_rels));
    files.push(("word/document.xml".to_string(), document_xml.to_string()));

    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, content) in files {
        writer.start_file(name, options).expect("start zip entry");
        writer
            .write_all(content.as_bytes())
            .expect("write zip entry");
    }
    writer.finish().expect("finish zip").into_inner()
}

fn document(body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body>{body}</w:body>
</w:document>"#
    )
}

fn paragraph(text: &str) -> String {
    format!(r#"<w:p><w:r><w:t xml:space="preserve">{text}</w:t></w:r></w:p>"#)
}

#[test]
fn extracts_body_header_footer_and_hides_deleted_text() {
    let body = format!(
        r#"{}<w:p><w:r><w:t>Deleted: </w:t></w:r><w:del><w:r><w:delText>old text</w:delText></w:r></w:del><w:r><w:t xml:space="preserve"> kept text.</w:t></w:r></w:p><w:ins><w:r><w:t>Inserted text.</w:t></w:r></w:ins>"#,
        paragraph("Hello DocxLite")
    );
    let header = r#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>Header Contact</w:t></w:r></w:p></w:hdr>"#;
    let footer = r#"<w:ftr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>Footer Contact</w:t></w:r></w:p></w:ftr>"#;
    let bytes = build_docx(&document(&body), Some(header), Some(footer));
    let document = load_bytes(&bytes).expect("load docx");
    let text = document.dump_text();
    assert!(text.contains("Hello DocxLite"));
    assert!(text.contains("Header Contact"));
    assert!(text.contains("Footer Contact"));
    assert!(text.contains("kept text."));
    assert!(text.contains("Inserted text."));
    assert!(!text.contains("old text"));
    assert_eq!(document.headers.len(), 1);
    assert_eq!(document.footers.len(), 1);
}

#[test]
fn table_cells_appear_in_text_output() {
    let body = r#"<w:tbl><w:tr><w:tc><w:p><w:r><w:t>Role</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Company</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>Engineer</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Example Ltd</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#;
    let bytes = build_docx(&document(body), None, None);
    let document = load_bytes(&bytes).expect("load table docx");
    let text = document.dump_text();
    for expected in ["Role", "Company", "Engineer", "Example Ltd"] {
        assert!(text.contains(expected), "missing {expected} in {text:?}");
    }
}

#[test]
fn text_lossless_against_independent_scan() {
    let body = format!(
        r#"{}{}{}<w:tbl><w:tr><w:tc><w:p><w:r><w:t>Cell A</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Cell B</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p><w:r><w:t>Final paragraph</w:t></w:r></w:p>"#,
        paragraph("First paragraph"),
        paragraph("Second paragraph"),
        paragraph("Third paragraph")
    );
    let xml = document(&body);
    let bytes = build_docx(&xml, None, None);
    let document = load_bytes(&bytes).expect("load text docx");
    let output = document.dump_text();
    let parsed = roxmltree::Document::parse(&xml).expect("parse test xml");
    for node in parsed
        .descendants()
        .filter(|node| node.tag_name().name() == "t")
    {
        let value = node.text().unwrap_or_default().trim();
        if !value.is_empty() {
            assert!(
                output.contains(value),
                "text loss: {value:?} not in {output:?}"
            );
        }
    }
}

#[test]
fn rejects_bad_containers_without_panicking() {
    assert_eq!(load_bytes(b"").unwrap_err(), LoadError::NotZip);
    assert_eq!(
        load_bytes(b"this is not a zip").unwrap_err(),
        LoadError::NotZip
    );
    let legacy = [0xD0_u8, 0xCF, 0x11, 0xE0, 0xA1, 0xB1];
    assert_eq!(
        load_bytes(&legacy).unwrap_err(),
        LoadError::LegacyOrEncrypted
    );
    let path = temp_path("directory");
    std::fs::create_dir_all(&path).expect("create temp dir");
    let result = load_document(&path, Arc::new(AtomicBool::new(false)));
    let _ = std::fs::remove_dir_all(&path);
    assert_eq!(result.err(), Some(LoadError::NotAFile));
}

#[test]
fn rejects_xml_with_dtd() {
    let body = r#"<!DOCTYPE foo [<!ENTITY x "boom">]><w:p><w:r><w:t>&x;</w:t></w:r></w:p>"#;
    let bytes = build_docx(&document(body), None, None);
    let result = load_bytes(&bytes);
    assert!(result.is_err(), "DTD document should not parse");
}

#[test]
fn rejects_compression_ratio_bomb() {
    let huge = "a".repeat(2 * 1024 * 1024);
    let body = format!(r#"<w:p><w:r><w:t>{huge}</w:t></w:r></w:p>"#);
    let xml = document(&body);
    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    writer
        .start_file("[Content_Types].xml", stored)
        .expect("content types");
    writer
        .write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
        )
        .expect("write content types");
    writer.start_file("_rels/.rels", stored).expect("root rels");
    writer
        .write_all(
            br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        )
        .expect("write root rels");
    writer
        .start_file("word/document.xml", deflated)
        .expect("document");
    writer.write_all(xml.as_bytes()).expect("write document");
    let bytes = writer.finish().expect("finish bomb").into_inner();
    let result = load_bytes(&bytes);
    assert_eq!(result.err(), Some(LoadError::Malformed));
}

#[test]
fn random_bytes_never_panic() {
    use proptest::prelude::*;

    proptest!(|(data in proptest::collection::vec(any::<u8>(), 0..2048))| {
        let result = std::panic::catch_unwind(|| load_bytes(&data));
        prop_assert!(result.is_ok(), "loader panicked on random bytes");
    });
}

#[test]
fn complex_fields_show_cached_result_and_skip_instructions() {
    let body = r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> PAGE </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>7</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#;
    let bytes = build_docx(&document(body), None, None);
    let document = load_bytes(&bytes).expect("load field docx");
    let text = document.dump_text();
    assert!(text.contains('7'));
    assert!(!text.contains("PAGE"));
}

#[test]
fn page_break_renders_but_last_rendered_page_break_is_ignored() {
    let body = r#"<w:p><w:r><w:t>Before</w:t></w:r><w:r><w:br w:type="page"/></w:r><w:r><w:lastRenderedPageBreak/></w:r><w:r><w:t>After</w:t></w:r></w:p>"#;
    let bytes = build_docx(&document(body), None, None);
    let document = load_bytes(&bytes).expect("load page-break docx");
    let text = document.dump_text();
    assert!(text.contains("Before"));
    assert!(text.contains("After"));
    assert_eq!(text.matches('─').count(), 16);
}

#[test]
fn text_box_content_is_not_lost() {
    let body = r#"<w:p><w:r><w:drawing xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"><wp:inline><a:graphic><a:graphicData uri="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"><wps:wsp><wps:txbx><w:txbxContent><w:p><w:r><w:t>Boxed text</w:t></w:r></w:p></w:txbxContent></wps:txbx></wps:wsp></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"#;
    let bytes = build_docx(&document(body), None, None);
    let document = load_bytes(&bytes).expect("load text-box docx");
    let text = document.dump_text();
    assert!(
        text.contains("Boxed text"),
        "text box text missing: {text:?}"
    );
}

#[test]
fn alternate_content_uses_one_branch_without_duplication() {
    let body = r#"<mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:Choice Requires="wps"><w:p><w:r><w:t>Choice text</w:t></w:r></w:p></mc:Choice><mc:Fallback><w:p><w:r><w:t>Fallback text</w:t></w:r></w:p></mc:Fallback></mc:AlternateContent>"#;
    let bytes = build_docx(&document(body), None, None);
    let document = load_bytes(&bytes).expect("load alternate-content docx");
    let text = document.dump_text();
    assert!(text.contains("Choice text"));
    assert!(!text.contains("Fallback text"));
    assert_eq!(text.matches("Choice text").count(), 1);
}
