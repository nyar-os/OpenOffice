//! Integration tests for the `open-office` facade.

use std::{
    fs,
    path::{Path, PathBuf},
};

use open_office::*;
use serde_json::{Value, json};

#[test]
fn convert_many_requires_inputs_format_and_out_dir() {
    let office = Office::new();
    let dir = Path::new("out");
    assert!(office.convert_many(&[], "pdf", dir, false).expect_err("empty inputs").is_usage());
    assert!(office.convert_many(&[PathBuf::from("a.docx")], "  ", dir, false).expect_err("empty format").is_usage());
    assert!(
        office.convert_many(&[PathBuf::from("a.docx")], "pdf", Path::new(""), false).expect_err("empty out dir").is_usage()
    );
}

#[test]
fn probe_many_reports_each_input_in_order() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-probe-many-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let pdf = dir.join("paper.pdf");
    fs::write(&pdf, b"%PDF-1.7\n").expect("write pdf");
    let markdown = dir.join("notes.md");
    fs::write(&markdown, b"# Title\n\nBody.\n").expect("write markdown");
    let unproven = dir.join("notes.docx");
    fs::write(&unproven, b"just words").expect("write unproven");
    let missing = dir.join("missing.pdf");

    let inputs = vec![pdf.clone(), markdown.clone(), unproven.clone(), missing.clone()];
    let report = office.probe_many(&inputs).expect("probe batch");
    assert_eq!(report["probed"], json!(3));
    assert_eq!(report["failed"], json!(1));
    let results = report["results"].as_array().expect("results array");
    assert_eq!(results.len(), 4);

    assert_eq!(results[0]["source"], json!(pdf.display().to_string()));
    assert_eq!(results[0]["ok"], json!(true));
    assert_eq!(results[0]["format"], json!("pdf"));
    assert_eq!(results[0]["container"], json!("pdf"));
    assert_eq!(results[0]["confidence"], json!("verified"));
    assert_eq!(results[0]["candidates"], json!(["pdf"]));
    assert!(results[0]["evidence"].as_str().expect("evidence").contains("%PDF-"));
    assert!(results[0].get("path").is_none());

    assert_eq!(results[1]["source"], json!(markdown.display().to_string()));
    assert_eq!(results[1]["ok"], json!(true));
    assert_eq!(results[1]["format"], json!("markdown"));
    assert_eq!(results[1]["confidence"], json!("hinted"));
    assert!(results[1]["evidence"].as_str().is_some());

    assert_eq!(results[2]["source"], json!(unproven.display().to_string()));
    assert_eq!(results[2]["ok"], json!(true));
    assert_eq!(results[2]["format"], Value::Null);
    assert_eq!(results[2]["confidence"], json!("unknown"));
    assert!(results[2]["evidence"].as_str().is_some());

    assert_eq!(results[3]["source"], json!(missing.display().to_string()));
    assert_eq!(results[3]["ok"], json!(false));
    assert_eq!(results[3]["code"], json!("not-found"));

    assert!(office.probe_many(&[]).expect_err("empty inputs").is_usage());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn outline_many_reports_each_input_in_order() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-outline-many-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let source = dir.join("notes.md");
    fs::write(&source, b"# Title\n\n## Child\n\nBody.\n").expect("write markdown");

    let inputs = vec![source.clone(), PathBuf::from("missing-outline.docx")];
    let report = office.outline_many(&inputs, false, None).expect("outline batch");
    assert_eq!(report["outlined"], json!(1));
    assert_eq!(report["failed"], json!(1));
    let results = report["results"].as_array().expect("results array");
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["source"], json!(source.display().to_string()));
    assert_eq!(results[0]["ok"], json!(true));
    assert_eq!(results[0]["kind"], json!("word"));
    assert_eq!(results[0]["outline"]["items"][0]["title"], json!("Title"));
    assert_eq!(results[1]["source"], json!("missing-outline.docx"));
    assert_eq!(results[1]["ok"], json!(false));
    assert_eq!(results[1]["code"], json!("not-found"));

    let points = office.outline_many(std::slice::from_ref(&source), true, None).expect("points batch");
    assert_eq!(points["outlined"], json!(0));
    assert_eq!(points["failed"], json!(1));
    assert_eq!(points["results"][0]["code"], json!("unsupported-option"));

    let shallow = office.outline(&source, false, Some(1)).expect("shallow outline");
    let items = shallow["outline"]["items"].as_array().expect("items");
    assert_eq!(items.len(), 1, "only the top heading survives: {shallow}");
    assert_eq!(items[0]["title"], json!("Title"));
    assert!(items[0]["children"].as_array().expect("children").is_empty());

    assert!(office.outline_many(&[], false, None).expect_err("empty inputs").is_usage());
    assert!(office.outline(&source, false, Some(0)).expect_err("zero depth").is_usage());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn open_routes_a_file_and_can_print_the_route() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-open-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let source = dir.join("notes.md");
    fs::write(&source, b"# Title\n").expect("write markdown");

    let report = office.open(&source, true).expect("print route");
    assert_eq!(report["kind"], json!("word"));
    assert_eq!(report["path"], json!(source.display().to_string()));
    assert_eq!(report["opened"], json!(false));
    assert!(!report["opener"].as_str().expect("opener").is_empty());

    let missing = office.open(&dir.join("missing.docx"), true).expect_err("missing file");
    assert_eq!(missing.code(), "not-found");
    let unsupported = office.open(&dir.join("notes.zip"), true).expect_err("unsupported extension");
    assert_eq!(unsupported.code(), "unsupported-extension");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn open_print_routes_every_document_family() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-open-families-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let cases = [("report.docx", "word"), ("book.xlsx", "excel"), ("deck.pptx", "presentation"), ("scan.pdf", "pdf")];
    for (name, family) in cases {
        let path = dir.join(name);
        fs::write(&path, b"placeholder").expect("write fixture");
        let report = office.open(&path, true).expect("print route");
        assert_eq!(report["kind"], json!(family), "{name} routes to the right family");
        assert_eq!(report["path"], json!(path.display().to_string()));
        assert_eq!(report["opened"], json!(false), "print mode never launches a viewer");
        assert!(!report["opener"].as_str().expect("opener").is_empty());
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn search_many_reports_each_input_in_order() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-search-many-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let source = dir.join("notes.md");
    fs::write(&source, b"# Title\n\nBody text.\n").expect("write markdown");

    let inputs = vec![source.clone(), PathBuf::from("missing-search.docx")];
    let report = office.search_many(&inputs, "Title", None).expect("search batch");
    assert_eq!(report["query"], json!("Title"));
    assert_eq!(report["searched"], json!(1));
    assert_eq!(report["failed"], json!(1));
    let results = report["results"].as_array().expect("results array");
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["source"], json!(source.display().to_string()));
    assert_eq!(results[0]["ok"], json!(true));
    assert_eq!(results[0]["kind"], json!("word"));
    assert_eq!(results[0]["search"]["query"], json!("Title"));
    assert!(!results[0]["search"]["hits"].as_array().expect("hits").is_empty());
    assert_eq!(results[1]["source"], json!("missing-search.docx"));
    assert_eq!(results[1]["ok"], json!(false));
    assert_eq!(results[1]["code"], json!("not-found"));

    assert!(office.search_many(&[], "Title", None).expect_err("empty inputs").is_usage());
    assert!(office.search_many(&inputs, "", None).expect_err("empty query").is_usage());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn stats_many_reports_each_input_in_order() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-stats-many-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let source = dir.join("notes.md");
    fs::write(&source, b"# Title\n\nBody text.\n").expect("write markdown");

    let inputs = vec![source.clone(), PathBuf::from("missing-stats.docx")];
    let report = office.stats_many(&inputs).expect("stats batch");
    assert_eq!(report["inspected"], json!(1));
    assert_eq!(report["failed"], json!(1));
    let results = report["results"].as_array().expect("results array");
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["source"], json!(source.display().to_string()));
    assert_eq!(results[0]["ok"], json!(true));
    assert_eq!(results[0]["kind"], json!("word"));
    assert!(results[0]["stats"].is_object());
    assert_eq!(results[1]["source"], json!("missing-stats.docx"));
    assert_eq!(results[1]["ok"], json!(false));
    assert_eq!(results[1]["code"], json!("not-found"));

    assert!(office.stats_many(&[]).expect_err("empty inputs").is_usage());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn totals_aggregates_statistics_across_families() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-totals-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let first = dir.join("a.md");
    fs::write(&first, b"# Alpha\n\none two three\n").expect("write first");
    let second = dir.join("b.md");
    fs::write(&second, b"# Beta\n\nfour five\n").expect("write second");
    let created = open_excel_workbook::create(&json!({ "title": "Totals" })).expect("create");
    let handle = handle_of(&created).expect("handle");
    open_excel_workbook::edit(
        handle,
        &json!({ "operations": [{ "op": "cell.set", "sheet": "Sheet1", "reference": "A1", "value": 42 }] }),
    )
    .expect("seed");
    let sheet = dir.join("budget.oexcel");
    open_excel_workbook::save(handle, &sheet, true).expect("save");
    let _ = open_excel_workbook::close(handle);
    let deck = open_ppt_deck::create(open_ppt_deck::CreateRequest {
        title: Some("Totals".to_owned()),
        size: None,
        theme: None,
        slides: vec!["Intro".to_owned(), "Plan".to_owned()],
    })
    .expect("create deck");
    let slides = dir.join("deck.project");
    open_ppt_deck::save(&deck, &slides, true).expect("save deck");
    let pdf = two_page_pdf(&dir);

    let inputs = vec![first, second, sheet, slides, pdf, PathBuf::from("missing-totals.docx")];
    let report = office.totals(&inputs).expect("totals batch");
    assert_eq!(report["counted"], json!(5));
    assert_eq!(report["failed"], json!(1));
    let families = &report["families"];
    assert_eq!(families["word"]["documents"], json!(2));
    assert_eq!(families["word"]["words"], json!(7));
    assert_eq!(families["word"]["headings"], json!(2));
    assert_eq!(families["word"]["paragraphs"], json!(2));
    assert_eq!(families["excel"]["documents"], json!(1));
    assert_eq!(families["excel"]["sheets"], json!(1));
    assert_eq!(families["excel"]["cells"], json!(1));
    assert_eq!(families["presentation"]["documents"], json!(1));
    assert_eq!(families["presentation"]["slides"], json!(2));
    assert_eq!(families["pdf"]["documents"], json!(1));
    assert_eq!(families["pdf"]["pages"], json!(2));
    let results = report["results"].as_array().expect("results array");
    assert_eq!(results[0]["totals"]["words"], json!(4));
    assert_eq!(results[5]["ok"], json!(false));
    assert_eq!(results[5]["code"], json!("not-found"));

    assert!(office.totals(&[]).expect_err("empty inputs").is_usage());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn inspect_many_reports_each_input_in_order() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-inspect-many-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let source = dir.join("notes.md");
    fs::write(&source, b"# Title\n\nBody text.\n").expect("write markdown");

    let inputs = vec![source.clone(), PathBuf::from("missing-inspect.docx")];
    let report = office.inspect_many(&inputs, None).expect("inspect batch");
    assert_eq!(report["inspected"], json!(1));
    assert_eq!(report["failed"], json!(1));
    let results = report["results"].as_array().expect("results array");
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["source"], json!(source.display().to_string()));
    assert_eq!(results[0]["ok"], json!(true));
    assert_eq!(results[0]["kind"], json!("word"));
    assert!(results[0]["inspect"].is_object());
    assert_eq!(results[1]["source"], json!("missing-inspect.docx"));
    assert_eq!(results[1]["ok"], json!(false));
    assert_eq!(results[1]["code"], json!("not-found"));

    assert!(office.inspect_many(&[], None).expect_err("empty inputs").is_usage());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn inspect_applies_requested_checks_to_presentations() {
    let dir = std::env::temp_dir().join(format!("open-office-inspect-checks-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let deck = open_ppt_deck::create(open_ppt_deck::CreateRequest {
        title: Some("Checks".to_owned()),
        size: None,
        theme: None,
        slides: vec!["Intro".to_owned()],
    })
    .expect("create deck");
    let path = dir.join("checks.project");
    open_ppt_deck::save(&deck, &path, true).expect("save deck");

    let report = Office::new().inspect(&path, Some(&["contrast".to_owned()])).expect("inspect deck");
    assert_eq!(report["checks"], json!(["contrast"]));
    assert!(report["inspect"]["issues"].is_array(), "presentations expose rule-check findings");

    let error = Office::new().inspect(&path, Some(&["no-such-check".to_owned()])).expect_err("unknown check");
    assert!(error.message().contains("unknown check"), "unexpected error: {}", error.message());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn convert_many_reports_missing_sources_per_item() {
    let office = Office::new();
    let out_dir = std::env::temp_dir().join(format!("open-office-convert-many-{}", std::process::id()));
    let inputs = vec![PathBuf::from("missing-a.docx"), PathBuf::from("missing-b.xlsx")];
    let report = office.convert_many(&inputs, "pdf", &out_dir, false).expect("batch result");
    assert_eq!(report["converted"], json!(0));
    assert_eq!(report["failed"], json!(2));
    let results = report["results"].as_array().expect("results array");
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["source"], json!("missing-a.docx"));
    assert_eq!(results[0]["target"], json!(out_dir.join("missing-a.pdf").display().to_string()));
    assert_eq!(results[0]["ok"], json!(false));
    assert_eq!(results[0]["code"], json!("not-found"));
    assert!(results[0]["error"].as_str().expect("error message").contains("does not exist"));
    assert_eq!(results[1]["ok"], json!(false));
    let _ = fs::remove_dir_all(&out_dir);
}

/// Batch failures carry a stable code next to the unchanged human message.
#[test]
fn convert_many_reports_stable_error_codes() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-error-codes-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    fs::write(dir.join("notes.bin"), b"just words").expect("write unknown");
    let inputs = vec![PathBuf::from("missing-c.docx"), dir.join("notes.bin")];
    let report = office.convert_many(&inputs, "pdf", &dir.join("out"), false).expect("batch result");
    assert_eq!(report["converted"], json!(0));
    assert_eq!(report["failed"], json!(2));
    let results = report["results"].as_array().expect("results array");
    assert_eq!(results[0]["code"], json!("not-found"));
    assert_eq!(results[1]["code"], json!("unsupported-extension"));
    assert!(results[1]["error"].is_string());

    let source = dir.join("notes.md");
    fs::write(&source, b"# Title\n").expect("write markdown");
    let target = dir.join("notes.pdf");
    fs::write(&target, b"%PDF-1.7\n").expect("write target");
    let error = office.convert(&source, "pdf", &target, false).expect_err("target exists");
    assert_eq!(error.code(), "target-exists");
    assert!(!error.is_usage());

    let error = office.convert(&source, "xlsx", &dir.join("notes.xlsx"), false).expect_err("unsupported target");
    assert_eq!(error.code(), "unsupported-target");
    assert!(error.is_usage());
    let _ = fs::remove_dir_all(&dir);
}

/// A `.pdf` route must not accept plain text that has no PDF header.
#[test]
fn container_probe_refuses_a_pdf_route_for_plain_text() {
    let dir = std::env::temp_dir().join(format!("open-office-probe-text-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("notes.pdf");
    fs::write(&path, b"just words").expect("write probe");
    let error = verify_container(&path, Kind::Pdf).expect_err("plain text is not a PDF");
    assert!(error.message().contains("notes.pdf is routed as pdf"), "unexpected message: {}", error.message());
    assert!(
        error.message().contains("plain text has no container header"),
        "the probe evidence is quoted: {}",
        error.message()
    );
    let _ = fs::remove_dir_all(&dir);
}

/// PDF bytes routed by a spreadsheets extension are refused with the probe evidence.
#[test]
fn container_probe_refuses_pdf_bytes_routed_as_a_sheet() {
    let dir = std::env::temp_dir().join(format!("open-office-probe-sheet-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("sheet.xlsx");
    fs::write(&path, b"%PDF-1.7\n").expect("write probe");
    let error = verify_container(&path, Kind::Excel).expect_err("PDF bytes are not a sheet");
    assert!(error.message().contains("sheet.xlsx is routed as excel"), "unexpected message: {}", error.message());
    assert!(error.message().contains("identifies a PDF stream"), "the probe evidence is quoted: {}", error.message());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn container_probe_accepts_a_pdf_that_really_is_one() {
    let dir = std::env::temp_dir().join(format!("open-office-probe-pdf-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("notes.pdf");
    fs::write(&path, b"%PDF-1.7\n").expect("write probe");
    verify_container(&path, Kind::Pdf).expect("a PDF header matches the PDF route");
    let _ = fs::remove_dir_all(&dir);
}

/// A probe reports the verified container facts and the quoted evidence.
#[test]
fn probe_reports_pdf_bytes_as_verified() {
    let dir = std::env::temp_dir().join(format!("open-office-format-probe-pdf-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("notes.pdf");
    fs::write(&path, b"%PDF-1.7\n").expect("write probe");
    let report = Office::new().probe(&path).expect("probe");
    assert_eq!(report["path"], json!(path.display().to_string()));
    assert_eq!(report["bytes"], json!(9));
    assert_eq!(report["extension"], json!("pdf"));
    assert_eq!(report["format"], json!("pdf"));
    assert_eq!(report["container"], json!("pdf"));
    assert_eq!(report["confidence"], json!("verified"));
    assert_eq!(report["candidates"], json!(["pdf"]));
    assert!(report["evidence"].as_str().expect("evidence").contains("%PDF-"), "unexpected evidence: {}", report["evidence"]);
    let _ = fs::remove_dir_all(&dir);
}

/// The extension is a hint for plain text, not proof of the format.
#[test]
fn probe_uses_the_extension_as_a_hint_only() {
    let dir = std::env::temp_dir().join(format!("open-office-format-probe-hint-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let hinted = dir.join("notes.md");
    fs::write(&hinted, b"# Title\n\nBody.\n").expect("write hinted");
    let report = Office::new().probe(&hinted).expect("probe hinted");
    assert_eq!(report["format"], json!("markdown"));
    assert_eq!(report["container"], json!("plain"));
    assert_eq!(report["confidence"], json!("hinted"));
    assert_eq!(report["candidates"], json!(["markdown"]));

    let unproven = dir.join("notes.docx");
    fs::write(&unproven, b"just words").expect("write unproven");
    let report = Office::new().probe(&unproven).expect("probe unproven");
    assert_eq!(report["format"], Value::Null);
    assert_eq!(report["confidence"], json!("unknown"));
    assert_eq!(report["candidates"], json!([]));
    let _ = fs::remove_dir_all(&dir);
}

/// A ZIP container without readable parts keeps every package candidate.
#[test]
fn probe_keeps_zip_package_candidates_ambiguous() {
    let dir = std::env::temp_dir().join(format!("open-office-format-probe-zip-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("payload.bin");
    fs::write(&path, b"PK\x03\x04not-a-package").expect("write probe");
    let report = Office::new().probe(&path).expect("probe");
    assert_eq!(report["format"], Value::Null);
    assert_eq!(report["container"], json!("zip"));
    assert_eq!(report["confidence"], json!("ambiguous"));
    assert_eq!(report["candidates"], json!(["docx", "xlsx", "pptx", "epub"]));
    assert!(report["evidence"].as_str().expect("evidence").contains("ZIP container"));
    let _ = fs::remove_dir_all(&dir);
}

/// Probing a path that is not there is a runtime failure, not a usage error.
#[test]
fn probe_rejects_a_missing_file() {
    let path = std::env::temp_dir().join("open-office-format-probe-missing.bin");
    let _ = fs::remove_file(&path);
    let error = Office::new().probe(&path).expect_err("missing file");
    assert!(!error.is_usage());
    assert!(error.message().contains("does not exist"), "unexpected message: {}", error.message());
}

/// Builds a small multi-page PDF so page-range behaviour can be exercised.
fn two_page_pdf(dir: &Path) -> PathBuf {
    let source = dir.join("source.pdf");
    let request = open_pdf_core::CreateRequest {
        out: source.clone(),
        pages: vec!["Alpha page".to_string(), "Beta page".to_string()],
        overwrite: true,
        ..Default::default()
    };
    open_pdf_core::create(&request).expect("create pdf");
    source
}

#[test]
fn pdf_conversion_extracts_only_the_requested_pages() {
    let dir = std::env::temp_dir().join(format!("open-office-pages-range-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let source = two_page_pdf(&dir);
    let target = dir.join("page-two.md");
    let options = ConvertOptions { pages: Some("2".to_string()) };
    let report = Office::new().convert_with(&source, "md", &target, false, &options).expect("page conversion");
    assert_eq!(report["report"]["pages"]["requested"], json!("2"));
    assert_eq!(report["report"]["pages"]["extracted"], json!(1));
    assert_eq!(report["report"]["pages"]["numbers"], json!([2]));
    let markdown = fs::read_to_string(&target).expect("read markdown");
    assert!(markdown.contains("Beta page"), "unexpected markdown: {markdown}");
    assert!(!markdown.contains("Alpha page"), "page 1 must be excluded: {markdown}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn pdf_page_ranges_refuse_invalid_requests() {
    let dir = std::env::temp_dir().join(format!("open-office-pages-invalid-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let source = two_page_pdf(&dir);
    let office = Office::new();
    let target = dir.join("out.md");
    let range = |pages: &str| ConvertOptions { pages: Some(pages.to_string()) };

    let error = office.convert_with(&source, "md", &target, false, &range("3")).expect_err("out of range");
    assert_eq!(error.code(), "invalid-pages");
    assert!(error.is_usage());

    let error = office.convert_with(&source, "md", &target, false, &range("1,1")).expect_err("duplicate page");
    assert_eq!(error.code(), "invalid-pages");

    let error = office.convert_with(&source, "md", &target, false, &range(" ")).expect_err("empty range");
    assert_eq!(error.code(), "invalid-pages");

    let markdown = dir.join("notes.md");
    fs::write(&markdown, b"# Title\n").expect("write markdown");
    let error = office.convert_with(&markdown, "pdf", &dir.join("notes.pdf"), false, &range("1")).expect_err("not a PDF");
    assert_eq!(error.code(), "unsupported-option");
    assert!(error.is_usage());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn outline_nests_word_headings() {
    let dir = std::env::temp_dir().join(format!("open-office-outline-word-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let created = open_word_document::create(&json!({ "title": "Outline" })).expect("create");
    let handle = handle_of(&created).expect("handle");
    open_word_document::edit(
        handle,
        &json!({
            "operations": [
                { "type": "heading.append", "level": 1, "text": "Overview" },
                { "type": "heading.append", "level": 2, "text": "Details" },
            ],
        }),
    )
    .expect("seed headings");
    let path = dir.join("outline.oword");
    open_word_document::save(handle, &path, true).expect("save");
    let _ = open_word_document::close(handle);

    let report = Office::new().outline(&path, false, None).expect("outline");
    assert_eq!(report["kind"], json!("word"));
    assert_eq!(report["outline"]["items"][0]["title"], json!("Overview"));
    assert_eq!(report["outline"]["items"][0]["children"][0]["title"], json!("Details"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn outline_lists_workbook_sheets_and_refuses_points() {
    let dir = std::env::temp_dir().join(format!("open-office-outline-excel-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let created = open_excel_workbook::create(&json!({ "title": "Outline" })).expect("create");
    let handle = handle_of(&created).expect("handle");
    let path = dir.join("outline.oexcel");
    open_excel_workbook::save(handle, &path, true).expect("save");
    let _ = open_excel_workbook::close(handle);

    let office = Office::new();
    let report = office.outline(&path, false, None).expect("outline");
    assert_eq!(report["kind"], json!("excel"));
    assert_eq!(report["outline"]["sheets"][0]["name"], json!("Sheet1"));

    let error = office.outline(&path, true, None).expect_err("points are presentations only");
    assert_eq!(error.code(), "unsupported-option");
    assert!(error.is_usage());
    let depth = office.outline(&path, false, Some(1)).expect_err("depth needs levels");
    assert_eq!(depth.code(), "unsupported-option");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn outline_reads_pdf_bookmarks() {
    let dir = std::env::temp_dir().join(format!("open-office-outline-pdf-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let source = two_page_pdf(&dir);
    let marked = dir.join("marked.pdf");
    open_pdf_core::set_bookmarks(
        &source,
        &marked,
        &[
            open_pdf_core::Bookmark { title: "Top".to_owned(), page: 1, depth: Some(0) },
            open_pdf_core::Bookmark { title: "Child".to_owned(), page: 2, depth: Some(1) },
        ],
    )
    .expect("bookmarks");

    let report = Office::new().outline(&marked, false, None).expect("outline");
    assert_eq!(report["kind"], json!("pdf"));
    assert_eq!(report["outline"]["pages"], json!(2));
    assert_eq!(report["outline"]["bookmarks"][0]["title"], json!("Top"));
    assert_eq!(report["outline"]["bookmarks"][0]["depth"], json!(1));
    assert_eq!(report["outline"]["bookmarks"][1]["depth"], json!(2));

    let shallow = Office::new().outline(&marked, false, Some(1)).expect("shallow bookmarks");
    assert_eq!(shallow["outline"]["bookmarks"].as_array().expect("bookmarks").len(), 1);
    assert_eq!(shallow["outline"]["bookmarks"][0]["title"], json!("Top"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn outline_lists_presentation_slides_with_points() {
    let dir = std::env::temp_dir().join(format!("open-office-outline-deck-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let deck = open_ppt_deck::create(open_ppt_deck::CreateRequest {
        title: Some("Outline".to_owned()),
        size: None,
        theme: None,
        slides: vec!["Intro".to_owned(), "Plan".to_owned()],
    })
    .expect("create");
    let path = dir.join("outline.project");
    open_ppt_deck::save(&deck, &path, true).expect("save");

    let report = Office::new().outline(&path, true, None).expect("outline");
    assert_eq!(report["kind"], json!("presentation"));
    assert_eq!(report["outline"]["slides"][0]["title"], json!("Intro"));
    assert_eq!(report["outline"]["slides"][1]["title"], json!("Plan"));
    assert_eq!(report["outline"]["slides"][0]["points"], json!([]));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn match_spans_returns_character_offsets() {
    assert_eq!(match_spans("标题和标题", "标题", 10), vec![(0, 2, "标题".to_owned()), (3, 5, "标题".to_owned())]);
    assert_eq!(match_spans("aaaa", "aa", 1).len(), 1);
    assert!(match_spans("text", "", 10).is_empty());
    assert!(match_spans("text", "missing", 10).is_empty());
}

#[test]
fn collect_json_text_reads_only_text_leaves() {
    let value = json!({
        "paragraphs": [{ "runs": [{ "text": "Hello", "style": { "color": "#ff0000" } }] }],
        "color": "#00ff00",
        "cells": [{ "text": { "paragraphs": [{ "runs": [{ "text": "Cell" }] }] } }],
    });
    let mut texts = Vec::new();
    collect_json_text(Some(&value), &mut texts);
    texts.sort();
    assert_eq!(texts, vec!["Cell".to_owned(), "Hello".to_owned()]);
}

#[test]
fn presentation_hits_cover_text_tables_and_notes() {
    let document = json!({
        "slides": [{
            "id": "slide-1",
            "notes": "Roadmap notes",
            "elements": [
                { "id": "element-1", "content": { "paragraphs": [{ "runs": [{ "text": "Roadmap" }] }] } },
                {
                    "id": "element-2",
                    "content": {
                        "rows": [{ "cells": [{ "text": { "paragraphs": [{ "runs": [{ "text": "Roadmap" }] }] } }] }],
                    },
                },
            ],
        }],
    });
    let (hits, truncated, _) = presentation_hits(&document, "Roadmap", 10);
    assert_eq!(hits.len(), 3);
    assert_eq!(hits[0]["source"], json!("notes"));
    assert_eq!(hits[1]["source"], json!("text"));
    assert_eq!(hits[2]["source"], json!("table"));
    assert!(!truncated);
    let (limited, truncated, _) = presentation_hits(&document, "Roadmap", 1);
    assert_eq!(limited.len(), 1);
    assert!(truncated);
}

#[test]
fn search_finds_presentation_text_through_the_office_facade() {
    let dir = std::env::temp_dir().join(format!("open-office-search-deck-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let deck = open_ppt_deck::create(open_ppt_deck::CreateRequest {
        title: Some("Search".to_owned()),
        size: None,
        theme: None,
        slides: vec!["Roadmap".to_owned(), "Budget".to_owned()],
    })
    .expect("create");
    let path = dir.join("search.project");
    open_ppt_deck::save(&deck, &path, true).expect("save");

    let report = Office::new().search(&path, "Roadmap", Some(10)).expect("search");
    assert_eq!(report["kind"], json!("presentation"));
    assert_eq!(report["query"], json!("Roadmap"));
    assert!(!report["hits"].as_array().expect("hits").is_empty());
    assert_eq!(report["hits"][0]["slideIndex"], json!(0));
    assert_eq!(report["hits"][0]["text"], json!("Roadmap"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn compare_diffs_word_outlines_with_rename_pairing() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-compare-word-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let left = dir.join("draft.md");
    fs::write(&left, b"# Intro\n\n## Details\n\n## Plans\n").expect("write draft");
    let right = dir.join("final.md");
    fs::write(&right, b"# Intro\n\n## Summary\n\n## Plans\n\n## Appendix\n").expect("write final");

    let report = office.compare(&left, &right).expect("compare drafts");
    assert_eq!(report["kind"], json!("word"));
    assert_eq!(report["left"]["sections"], json!(3));
    assert_eq!(report["right"]["sections"], json!(4));
    assert_eq!(report["identical"], json!(false));
    assert_eq!(report["renamed"], json!([{ "from": "Details", "to": "Summary" }]));
    assert_eq!(report["added"], json!(["Appendix"]));
    assert_eq!(report["removed"], json!([]));

    let same = office.compare(&left, &left).expect("compare with itself");
    assert_eq!(same["identical"], json!(true));
    assert_eq!(same["added"], json!([]));
    assert_eq!(same["removed"], json!([]));
    assert_eq!(same["renamed"], json!([]));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn compare_can_limit_itself_to_shallow_headings() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-compare-depth-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let left = dir.join("draft.md");
    fs::write(&left, b"# Intro\n\n## Details\n\n### Deep\n").expect("write draft");
    let right = dir.join("final.md");
    fs::write(&right, b"# Intro\n\n## Summary\n\n### Deep\n").expect("write final");

    let shallow = office.compare_at(&left, &right, false, Some(1), false, false).expect("shallow compare");
    assert_eq!(shallow["depth"], json!(1));
    assert_eq!(shallow["left"]["sections"], json!(1));
    assert_eq!(shallow["right"]["sections"], json!(1));
    assert_eq!(shallow["identical"], json!(true), "the deeper rename is below the depth");

    let deeper = office.compare_at(&left, &right, false, Some(2), false, false).expect("deeper compare");
    assert_eq!(deeper["left"]["sections"], json!(2));
    assert_eq!(deeper["renamed"], json!([{ "from": "Details", "to": "Summary" }]));

    let zero = office.compare_at(&left, &right, false, Some(0), false, false).expect_err("depth zero");
    assert_eq!(zero.code(), "invalid-depth");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn compare_expands_presentation_points_when_asked() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-compare-points-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let build = |name: &str, point: &str| {
        let mut deck = open_ppt_deck::create(open_ppt_deck::CreateRequest {
            title: Some("Points".to_owned()),
            size: None,
            theme: None,
            slides: vec!["Plan".to_owned()],
        })
        .expect("create deck");
        let slide = open_ppt_deck::summary(&deck)["slides"][0]["id"].as_str().expect("slide id").to_owned();
        open_ppt_deck::edit::edit(
            &mut deck,
            &json!({ "operations": [{
                "op": "text.add",
                "slideId": slide,
                "text": { "paragraphs": [{ "runs": [{ "text": point }] }] },
            }] }),
        )
        .expect("add point");
        let path = dir.join(name);
        open_ppt_deck::save(&deck, &path, true).expect("save deck");
        path
    };
    let before = build("before.project", "Alpha");
    let after = build("after.project", "Beta");

    let titles = office.compare_at(&before, &after, false, None, false, false).expect("title compare");
    assert_eq!(titles["points"], json!(false));
    assert_eq!(titles["identical"], json!(true), "slide titles alone are unchanged");

    let points = office.compare_at(&before, &after, false, None, true, false).expect("point compare");
    assert_eq!(points["points"], json!(true));
    assert_eq!(points["left"]["sections"], json!(1));
    assert_eq!(
        points["renamed"],
        json!([{ "from": "Plan · Alpha", "to": "Plan · Beta" }]),
        "body points participate in the diff"
    );

    let word = dir.join("plain.md");
    fs::write(&word, b"# Plan\n").expect("write word");
    let unsupported = office.compare_at(&word, &word, false, None, true, false).expect_err("points on word");
    assert_eq!(unsupported.code(), "unsupported-option");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn compare_expands_presentation_notes_when_asked() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-compare-notes-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let build = |name: &str, notes: Option<&str>| {
        let mut deck = open_ppt_deck::create(open_ppt_deck::CreateRequest {
            title: Some("Notes".to_owned()),
            size: None,
            theme: None,
            slides: vec!["Plan".to_owned()],
        })
        .expect("create deck");
        let slide = open_ppt_deck::summary(&deck)["slides"][0]["id"].as_str().expect("slide id").to_owned();
        if let Some(notes) = notes {
            open_ppt_deck::edit::edit(
                &mut deck,
                &json!({ "operations": [{ "op": "slide.setNotes", "slideId": slide, "text": notes }] }),
            )
            .expect("set notes");
        }
        let path = dir.join(name);
        open_ppt_deck::save(&deck, &path, true).expect("save deck");
        path
    };
    let before = build("before.project", Some("Alpha\n  Beta \n\n"));
    let after = build("after.project", Some("Alpha\nGamma"));

    let titles = office.compare_at(&before, &after, false, None, false, false).expect("title compare");
    assert_eq!(titles["identical"], json!(true), "slide titles alone are unchanged");

    let notes = office.compare_at(&before, &after, false, None, false, true).expect("note compare");
    assert_eq!(notes["notes"], json!(true));
    assert_eq!(notes["points"], json!(false));
    assert_eq!(notes["left"]["sections"], json!(2), "blank lines are dropped and lines are trimmed");
    assert_eq!(notes["right"]["sections"], json!(2));
    assert_eq!(
        notes["renamed"],
        json!([{ "from": "Plan · Beta", "to": "Plan · Gamma" }]),
        "note lines participate in the diff"
    );

    let plain = build("plain.project", None);
    let fallback = office.compare_at(&plain, &plain, false, None, false, true).expect("fallback compare");
    assert_eq!(fallback["left"]["sections"], json!(1));
    assert_eq!(fallback["identical"], json!(true), "a slide without notes falls back to its title");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn compare_refuses_notes_for_other_families_and_points() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-compare-notes-guards-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let word = dir.join("plain.md");
    fs::write(&word, b"# Plan\n").expect("write word");
    let unsupported = office.compare_at(&word, &word, false, None, false, true).expect_err("notes on word");
    assert_eq!(unsupported.code(), "unsupported-option");
    assert!(unsupported.is_usage());

    let deck = open_ppt_deck::create(open_ppt_deck::CreateRequest {
        title: Some("Plan".to_owned()),
        size: None,
        theme: None,
        slides: vec!["Plan".to_owned()],
    })
    .expect("create deck");
    open_ppt_deck::save(&deck, &dir.join("plan.project"), true).expect("save deck");
    let plan = dir.join("plan.project");
    let mixed = office.compare_at(&plan, &plan, false, None, true, true).expect_err("points and notes together");
    assert_eq!(mixed.code(), "unsupported-option");
    assert!(mixed.message().contains("mutually exclusive"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn compare_diffs_presentation_slide_titles() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-compare-deck-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let before = open_ppt_deck::create(open_ppt_deck::CreateRequest {
        title: Some("Before".to_owned()),
        size: None,
        theme: None,
        slides: vec!["Intro".to_owned(), "Plan".to_owned()],
    })
    .expect("create before");
    let before_path = dir.join("before.project");
    open_ppt_deck::save(&before, &before_path, true).expect("save before");
    let after = open_ppt_deck::create(open_ppt_deck::CreateRequest {
        title: Some("After".to_owned()),
        size: None,
        theme: None,
        slides: vec!["Plan".to_owned(), "Review".to_owned()],
    })
    .expect("create after");
    let after_path = dir.join("after.project");
    open_ppt_deck::save(&after, &after_path, true).expect("save after");

    let report = office.compare(&before_path, &after_path).expect("compare decks");
    assert_eq!(report["kind"], json!("presentation"));
    assert_eq!(report["left"]["sections"], json!(2));
    assert_eq!(report["right"]["sections"], json!(2));
    assert_eq!(report["removed"], json!(["Intro"]));
    assert_eq!(report["added"], json!(["Review"]));
    assert_eq!(report["renamed"], json!([]));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn compare_refuses_documents_of_different_families() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-compare-kinds-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let document = dir.join("notes.md");
    fs::write(&document, b"# Notes\n").expect("write markdown");
    let pdf = two_page_pdf(&dir);

    let error = office.compare(&document, &pdf).expect_err("cross-family compare");
    assert_eq!(error.code(), "kind-mismatch");
    assert!(error.is_usage());
    let missing = office.compare(&document, &dir.join("gone.md")).expect_err("missing right side");
    assert_eq!(missing.code(), "not-found");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn replace_saves_a_word_document_in_place() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-replace-word-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let source = dir.join("notes.md");
    fs::write(&source, b"# Title\n\nAcme corp. and Acme Ltd.\n").expect("write markdown");

    let report = office.replace(&source, "Acme", "Globex", &ReplaceOptions::default()).expect("replace");
    assert_eq!(report["kind"], json!("word"));
    assert_eq!(report["replaced"], json!(2));
    assert_eq!(report["matchCase"], json!(true));
    assert_eq!(report["saved"]["format"], json!("md"));
    let text = fs::read_to_string(&source).expect("read back");
    assert!(text.contains("Globex corp. and Globex Ltd."), "unexpected markdown: {text}");
    assert!(!text.contains("Acme"));

    let untouched = office.replace(&source, "globex", "x", &ReplaceOptions::default()).expect("case-sensitive miss");
    assert_eq!(untouched["replaced"], json!(0));
    assert_eq!(untouched["saved"], Value::Null);
    assert!(fs::read_to_string(&source).expect("still there").contains("Globex corp. and Globex Ltd."));

    let loosened =
        office
            .replace(&source, "globex", "G", &ReplaceOptions { ignore_case: true, limit: Some(1), dry_run: false })
            .expect("limited replace");
    assert_eq!(loosened["replaced"], json!(1));
    let text = fs::read_to_string(&source).expect("read back");
    assert!(text.contains("G corp") ^ text.contains("G Ltd"), "exactly one match was replaced: {text}");

    let error = office.replace(&source, "", "x", &ReplaceOptions::default()).expect_err("empty find");
    assert_eq!(error.code(), "invalid-query");
    assert!(error.is_usage());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn replace_saves_a_workbook_in_place() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-replace-excel-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let created = open_excel_workbook::create(&json!({ "title": "Replace" })).expect("create");
    let handle = handle_of(&created).expect("handle");
    open_excel_workbook::edit(
        handle,
        &json!({ "operations": [
            { "op": "cell.set", "sheet": "Sheet1", "reference": "A1", "value": "Widget cost" },
            { "op": "cell.set", "sheet": "Sheet1", "reference": "B2", "value": "Two Widgets" },
        ] }),
    )
    .expect("seed");
    let path = dir.join("budget.oexcel");
    open_excel_workbook::save(handle, &path, true).expect("save");
    let _ = open_excel_workbook::close(handle);

    let report = office.replace(&path, "Widget", "Gadget", &ReplaceOptions::default()).expect("replace sheet");
    assert_eq!(report["kind"], json!("excel"));
    assert_eq!(report["replaced"], json!(2));
    assert_eq!(report["saved"]["format"], json!("oexcel"));

    let search = office.search(&path, "Gadget", None).expect("search after replace");
    assert_eq!(search["hits"].as_array().expect("hits").len(), 2);
    let stale = office.search(&path, "Widget", None).expect("search old text");
    assert!(stale["hits"].as_array().expect("hits").is_empty());

    let empty = dir.join("empty.oexcel");
    let created = open_excel_workbook::create(&json!({ "title": "Empty" })).expect("create");
    let handle = handle_of(&created).expect("handle");
    open_excel_workbook::save(handle, &empty, true).expect("save empty");
    let _ = open_excel_workbook::close(handle);
    let report = office.replace(&empty, "Widget", "Gadget", &ReplaceOptions::default()).expect("empty sheet batch");
    assert_eq!(report["replaced"], json!(0));
    assert_eq!(report["saved"], Value::Null);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn replace_rewrites_presentation_text_in_place() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-replace-deck-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let deck = open_ppt_deck::create(open_ppt_deck::CreateRequest {
        title: Some("Replace deck".to_owned()),
        size: None,
        theme: None,
        slides: vec!["Roadmap".to_owned(), "Roadmap review".to_owned()],
    })
    .expect("create deck");
    let project = dir.join("deck.project");
    open_ppt_deck::save(&deck, &project, true).expect("save project");

    let report = office.replace(&project, "Roadmap", "Plan", &ReplaceOptions::default()).expect("replace project");
    assert_eq!(report["kind"], json!("presentation"));
    assert_eq!(report["replaced"], json!(2));
    assert_eq!(report["saved"]["format"], json!("project"));
    let search = office.search(&project, "Plan", None).expect("search new text");
    assert_eq!(search["hits"].as_array().expect("hits").len(), 2);
    let stale = office.search(&project, "Roadmap", None).expect("search old text");
    assert!(stale["hits"].as_array().expect("hits").is_empty());

    let pptx = dir.join("deck.pptx");
    office.convert(&project, "pptx", &pptx, false).expect("convert to pptx");
    let report = office.replace(&pptx, "Plan", "Route", &ReplaceOptions::default()).expect("replace pptx");
    assert_eq!(report["kind"], json!("presentation"));
    assert_eq!(report["replaced"], json!(2));
    assert_eq!(report["saved"]["format"], json!("pptx"));
    let search = office.search(&pptx, "Route", None).expect("search exported text");
    assert_eq!(search["hits"].as_array().expect("hits").len(), 2);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn replace_refuses_a_limit_for_presentations() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-replace-deck-limit-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let deck = open_ppt_deck::create(open_ppt_deck::CreateRequest {
        title: Some("Limited deck".to_owned()),
        size: None,
        theme: None,
        slides: vec!["Roadmap".to_owned(), "Roadmap review".to_owned()],
    })
    .expect("create deck");
    let project = dir.join("deck.project");
    open_ppt_deck::save(&deck, &project, true).expect("save project");

    let error = office
        .replace(&project, "Roadmap", "Plan", &ReplaceOptions { limit: Some(1), ..Default::default() })
        .expect_err("a limited deck request is refused");
    assert!(error.is_usage());
    assert_eq!(error.code(), "unsupported-option");
    let untouched = office.search(&project, "Roadmap", None).expect("search after refusal");
    assert_eq!(untouched["hits"].as_array().expect("hits").len(), 2);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn replace_many_reports_each_input_in_order() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-replace-many-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let first = dir.join("a.md");
    fs::write(&first, b"# Alpha\n").expect("write first");
    let second = dir.join("b.md");
    fs::write(&second, b"Alpha beta.\n").expect("write second");
    let deck = open_ppt_deck::create(open_ppt_deck::CreateRequest {
        title: Some("No replace".to_owned()),
        size: None,
        theme: None,
        slides: vec!["Intro".to_owned()],
    })
    .expect("create deck");
    let slides = dir.join("deck.project");
    open_ppt_deck::save(&deck, &slides, true).expect("save deck");

    let inputs = vec![first.clone(), second.clone(), slides.clone(), PathBuf::from("missing-replace.docx")];
    let report = office.replace_many(&inputs, "Alpha", "Omega", &ReplaceOptions::default()).expect("replace batch");
    assert_eq!(report["find"], json!("Alpha"));
    assert_eq!(report["replace"], json!("Omega"));
    assert_eq!(report["updated"], json!(3));
    assert_eq!(report["failed"], json!(1));
    let results = report["results"].as_array().expect("results array");
    assert_eq!(results[0]["ok"], json!(true));
    assert_eq!(results[0]["kind"], json!("word"));
    assert_eq!(results[0]["replaced"], json!(1));
    assert_eq!(results[1]["ok"], json!(true));
    assert_eq!(results[1]["replaced"], json!(1));
    assert_eq!(results[2]["ok"], json!(true), "presentations now replace");
    assert_eq!(results[2]["kind"], json!("presentation"));
    assert_eq!(results[2]["replaced"], json!(0), "the deck has no match");
    assert_eq!(results[3]["code"], json!("not-found"));
    assert!(fs::read_to_string(&first).expect("read first").contains("Omega"));
    assert!(fs::read_to_string(&second).expect("read second").contains("Omega"));

    assert!(office.replace_many(&[], "a", "b", &ReplaceOptions::default()).expect_err("empty inputs").is_usage());
    assert!(office.replace_many(&inputs, "", "b", &ReplaceOptions::default()).expect_err("empty find").is_usage());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn replace_many_survives_a_mid_batch_failure_without_leaked_artifacts() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-replace-mid-failure-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp dir");
    let first = dir.join("a.md");
    fs::write(&first, b"# Alpha\n").expect("write first");
    let third = dir.join("c.md");
    fs::write(&third, b"Alpha again.\n").expect("write third");

    let inputs = vec![first.clone(), dir.join("missing.md"), third.clone()];
    let report = office.replace_many(&inputs, "Alpha", "Omega", &ReplaceOptions::default()).expect("replace batch");
    assert_eq!(report["updated"], json!(2));
    assert_eq!(report["failed"], json!(1));
    let results = report["results"].as_array().expect("results array");
    assert_eq!(results.len(), 3, "one entry per input, in input order");
    assert_eq!(results[0]["source"], json!(first.display().to_string()));
    assert_eq!(results[0]["ok"], json!(true));
    assert_eq!(results[0]["replaced"], json!(1), "file 1 keeps its result after file 2 failed");
    assert!(results[0]["revision"].is_number());
    assert_eq!(results[1]["ok"], json!(false));
    assert_eq!(results[1]["code"], json!("not-found"));
    assert!(results[1].get("params").is_some(), "the batch envelope carries the params field");
    assert!(results[1]["error"].is_string(), "the failure message stays with the stable code");
    assert_eq!(results[2]["ok"], json!(true), "the batch continues after the failure");
    assert_eq!(results[2]["replaced"], json!(1));
    assert!(fs::read_to_string(&first).expect("read first").contains("Omega"));
    assert!(fs::read_to_string(&third).expect("read third").contains("Omega"));

    // The replace path holds its engine handle in a Drop guard, so a failing
    // document never leaves a live session behind: the word engine can still
    // open the changed document, and a closed handle stops resolving.
    let opened = open_word_document::open(&first).expect("reopen after the failed batch");
    let handle = handle_of(&opened).expect("handle");
    assert!(open_word_document::summary(handle).is_ok(), "the reopened handle is live");
    open_word_document::close(handle).expect("close");
    assert_eq!(open_word_document::summary(handle).expect_err("closed handles stop resolving").code(), "document/not-found");

    // No leaked temp or lock artifacts: only the two changed documents remain.
    let mut names: Vec<String> = fs::read_dir(&dir)
        .expect("read dir")
        .map(|entry| entry.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, vec!["a.md".to_owned(), "c.md".to_owned()]);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn render_writes_a_workbook_sheet_and_protects_the_target() {
    let dir = std::env::temp_dir().join(format!("open-office-render-excel-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let created = open_excel_workbook::create(&json!({ "title": "Render" })).expect("create");
    let handle = handle_of(&created).expect("handle");
    open_excel_workbook::edit(
        handle,
        &json!({ "operations": [{ "op": "cell.set", "sheet": "Sheet1", "reference": "A1", "value": "Widget" }] }),
    )
    .expect("seed");
    let path = dir.join("render.oexcel");
    open_excel_workbook::save(handle, &path, true).expect("save");
    let _ = open_excel_workbook::close(handle);

    let office = Office::new();
    let report = office.render(&path, &RenderOptions::default()).expect("render");
    assert_eq!(report["kind"], json!("excel"));
    assert_eq!(report["format"], json!("svg"));
    assert_eq!(report["totalPages"], json!(1));
    assert_eq!(report["pages"][0]["sheet"], json!("Sheet1"));
    assert!(report["pages"][0]["svg"].as_str().expect("svg").starts_with("<svg"));

    let out = dir.join("sheet.svg");
    let options = RenderOptions { out: Some(out.clone()), ..RenderOptions::default() };
    let written = office.render(&path, &options).expect("write svg");
    assert_eq!(written["out"], json!(out.display().to_string()));
    assert!(fs::read_to_string(&out).expect("read svg").starts_with("<svg"));

    let error = office.render(&path, &options).expect_err("existing target");
    assert_eq!(error.code(), "target-exists");

    let error = office.render(&path, &RenderOptions { page: Some(1), ..RenderOptions::default() }).expect_err("page");
    assert_eq!(error.code(), "unsupported-option");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn render_lists_presentation_pages_and_selects_one() {
    let dir = std::env::temp_dir().join(format!("open-office-render-deck-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let deck = open_ppt_deck::create(open_ppt_deck::CreateRequest {
        title: Some("Render".to_owned()),
        size: None,
        theme: None,
        slides: vec!["Intro".to_owned(), "Plan".to_owned()],
    })
    .expect("create");
    let path = dir.join("render.project");
    open_ppt_deck::save(&deck, &path, true).expect("save");

    let office = Office::new();
    let report = office.render(&path, &RenderOptions::default()).expect("render");
    assert_eq!(report["kind"], json!("presentation"));
    assert_eq!(report["totalPages"], json!(2));
    assert_eq!(report["pages"].as_array().expect("pages").len(), 2);
    assert!(report["pages"][1]["svg"].as_str().expect("svg").starts_with("<svg"));

    let selected =
        office.render(&path, &RenderOptions { page: Some(2), ..RenderOptions::default() }).expect("page 2");
    assert_eq!(selected["pages"].as_array().expect("pages").len(), 1);
    assert_eq!(selected["pages"][0]["index"], json!(1));

    let error = office.render(&path, &RenderOptions { page: Some(3), ..RenderOptions::default() }).expect_err("range");
    assert_eq!(error.code(), "page-out-of-range");

    let ordered =
        office.render(&path, &RenderOptions { pages: Some("2,1".to_owned()), ..RenderOptions::default() }).expect("pages");
    assert_eq!(ordered["pages"].as_array().expect("pages").len(), 2);
    assert_eq!(ordered["pages"][0]["index"], json!(1), "requested order is kept");
    assert_eq!(ordered["pages"][1]["index"], json!(0));

    let outside =
        office.render(&path, &RenderOptions { pages: Some("3".to_owned()), ..RenderOptions::default() }).expect_err("pages range");
    assert_eq!(outside.code(), "invalid-pages");
    let both = office
        .render(&path, &RenderOptions { page: Some(1), pages: Some("1".to_owned()), ..RenderOptions::default() })
        .expect_err("ambiguous selection");
    assert_eq!(both.code(), "unsupported-option");
    let workbook = dir.join("book.oexcel");
    open_excel_workbook::create(&json!({ "title": "Render" })).map(|created| {
        let handle = created["handle"].as_u64().expect("handle");
        open_excel_workbook::save(handle, &workbook, true).expect("save workbook");
        open_excel_workbook::close(handle).expect("close");
    }).expect("create workbook");
    let unsupported = office
        .render(&workbook, &RenderOptions { pages: Some("1".to_owned()), ..RenderOptions::default() })
        .expect_err("pages on a workbook");
    assert_eq!(unsupported.code(), "unsupported-option");

    let out = dir.join("one.svg");
    let error =
        office.render(&path, &RenderOptions { out: Some(out), ..RenderOptions::default() }).expect_err("ambiguous out");
    assert_eq!(error.code(), "multiple-pages");

    let preview = dir.join("preview");
    let written =
        office.render(&path, &RenderOptions { out_dir: Some(preview.clone()), ..RenderOptions::default() }).expect("dir");
    assert_eq!(written["written"].as_array().expect("written").len(), 2);
    assert!(preview.join("page-1.svg").exists());
    assert!(preview.join("page-2.svg").exists());
    assert!(fs::read_to_string(preview.join("page-2.svg")).expect("page two").starts_with("<svg"));

    let error = office
        .render(&path, &RenderOptions { out_dir: Some(preview), ..RenderOptions::default() })
        .expect_err("existing pages");
    assert_eq!(error.code(), "target-exists");

    let error = office
        .render(
            &path,
            &RenderOptions {
                out: Some(dir.join("x.svg")),
                out_dir: Some(dir.join("y")),
                ..RenderOptions::default()
            },
        )
        .expect_err("mutually exclusive");
    assert_eq!(error.code(), "unsupported-option");

    let error = office
        .render(&path, &RenderOptions { sheet: Some("Sheet1".to_owned()), ..RenderOptions::default() })
        .expect_err("sheet is for workbooks");
    assert_eq!(error.code(), "unsupported-option");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn render_many_writes_one_directory_per_document() {
    let dir = std::env::temp_dir().join(format!("open-office-render-many-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");

    let created = open_excel_workbook::create(&json!({ "title": "Batch" })).expect("create");
    let handle = handle_of(&created).expect("handle");
    let sheet = dir.join("budget.oexcel");
    open_excel_workbook::save(handle, &sheet, true).expect("save sheet");
    let _ = open_excel_workbook::close(handle);

    let deck = open_ppt_deck::create(open_ppt_deck::CreateRequest {
        title: Some("Batch".to_owned()),
        size: None,
        theme: None,
        slides: vec!["Intro".to_owned(), "Plan".to_owned()],
    })
    .expect("create deck");
    let slides = dir.join("deck.project");
    open_ppt_deck::save(&deck, &slides, true).expect("save deck");

    let word = dir.join("notes.oword");
    let document = open_word_document::create(&json!({ "title": "No renderer" })).expect("create word");
    let word_handle = handle_of(&document).expect("handle");
    open_word_document::save(word_handle, &word, true).expect("save word");
    let _ = open_word_document::close(word_handle);

    let office = Office::new();
    let out = dir.join("preview");
    let inputs = vec![sheet, slides, word];
    let report = office
        .render_many(&inputs, &RenderOptions { out_dir: Some(out.clone()), ..RenderOptions::default() })
        .expect("render batch");
    assert_eq!(report["rendered"], json!(2));
    assert_eq!(report["failed"], json!(1));
    let results = report["results"].as_array().expect("results");
    assert_eq!(results.len(), 3);
    assert_eq!(results[0]["ok"], json!(true));
    assert_eq!(results[0]["totalPages"], json!(1));
    assert!(out.join("budget").join("page-1.svg").exists());
    assert_eq!(results[1]["ok"], json!(true));
    assert!(out.join("deck").join("page-2.svg").exists());
    assert_eq!(results[2]["ok"], json!(false));
    assert_eq!(results[2]["code"], json!("unsupported-target"));

    let error = office
        .render_many(&inputs, &RenderOptions { out: Some(dir.join("one.svg")), ..RenderOptions::default() })
        .expect_err("out is for one page");
    assert_eq!(error.code(), "unsupported-option");

    let error = office.render_many(&inputs, &RenderOptions::default()).expect_err("out dir is required for a batch");
    assert!(error.is_usage());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn render_refuses_families_without_a_renderer() {
    let dir = std::env::temp_dir().join(format!("open-office-render-word-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let created = open_word_document::create(&json!({ "title": "No renderer" })).expect("create");
    let handle = handle_of(&created).expect("handle");
    let path = dir.join("render.oword");
    open_word_document::save(handle, &path, true).expect("save");
    let _ = open_word_document::close(handle);

    let error = Office::new().render(&path, &RenderOptions::default()).expect_err("no word renderer");
    assert_eq!(error.code(), "unsupported-target");
    assert!(error.is_usage());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn compare_reports_sections_that_only_moved() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-compare-moved-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let left = dir.join("left.md");
    fs::write(&left, b"# Intro\n\n## Alpha\n\n## Beta\n").expect("write left");
    let right = dir.join("right.md");
    fs::write(&right, b"# Alpha\n\n## Beta\n\n## Intro\n").expect("write right");

    let report = office.compare(&left, &right).expect("compare reordered outlines");
    assert_eq!(report["identical"], json!(false));
    assert_eq!(report["added"], json!([]));
    assert_eq!(report["removed"], json!([]));
    assert_eq!(report["renamed"], json!([]));
    assert_eq!(report["moved"], json!([{ "title": "Intro", "from": 0, "to": 2 }]));

    let same = office.compare(&left, &left).expect("compare with itself");
    assert_eq!(same["moved"], json!([]));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn metadata_reports_the_title_each_family_stores() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-metadata-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");

    let word = dir.join("brief.oword");
    let created = open_word_document::create(&json!({ "title": "Brief" })).expect("create word");
    let handle = created["handle"].as_u64().expect("word handle");
    open_word_document::save(handle, &word, true).expect("save word");
    let _ = open_word_document::close(handle);
    let report = office.metadata(&word).expect("word metadata");
    assert_eq!(report["kind"], json!("word"));
    assert_eq!(report["metadata"]["title"], json!("Brief"));

    let sheet = dir.join("ledger.oexcel");
    let created = open_excel_workbook::create(&json!({ "title": "Ledger" })).expect("create workbook");
    let handle = created["handle"].as_u64().expect("workbook handle");
    open_excel_workbook::save(handle, &sheet, true).expect("save workbook");
    let _ = open_excel_workbook::close(handle);
    let report = office.metadata(&sheet).expect("workbook metadata");
    assert_eq!(report["kind"], json!("excel"));
    assert_eq!(report["metadata"]["title"], json!("Ledger"));

    let deck = open_ppt_deck::create(open_ppt_deck::CreateRequest {
        title: Some("Roadmap".to_owned()),
        size: None,
        theme: None,
        slides: vec!["Intro".to_owned()],
    })
    .expect("create deck");
    let slides = dir.join("roadmap.project");
    open_ppt_deck::save(&deck, &slides, true).expect("save deck");
    let report = office.metadata(&slides).expect("presentation metadata");
    assert_eq!(report["kind"], json!("presentation"));
    assert_eq!(report["metadata"]["title"], json!("Roadmap"));

    let paper = dir.join("paper.pdf");
    let request = open_pdf_core::CreateRequest {
        out: paper.clone(),
        title: Some("Paper".to_owned()),
        pages: vec!["One".to_owned()],
        overwrite: true,
        ..Default::default()
    };
    open_pdf_core::create(&request).expect("create pdf");
    let report = office.metadata(&paper).expect("pdf metadata");
    assert_eq!(report["kind"], json!("pdf"));
    assert_eq!(report["metadata"]["title"], json!("Paper"));
    assert_eq!(report["metadata"]["producer"], json!("OpenPDF"));

    let batch = office
        .metadata_many(&[word.clone(), dir.join("missing.oword"), paper.clone()])
        .expect("metadata batch");
    assert_eq!(batch["counted"], json!(2));
    assert_eq!(batch["failed"], json!(1));
    assert_eq!(batch["results"][0]["ok"], json!(true));
    assert_eq!(batch["results"][0]["metadata"]["title"], json!("Brief"));
    assert_eq!(batch["results"][1]["ok"], json!(false));
    assert!(batch["results"][1]["code"].is_string());
    assert_eq!(batch["results"][2]["metadata"]["title"], json!("Paper"));

    assert!(office.metadata(&dir.join("missing.oword")).is_err());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn replace_dry_run_counts_matches_without_writing_any_family() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-replace-dry-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");

    let word = dir.join("notes.md");
    fs::write(&word, b"# Acme\n\nAcme and Acme.\n").expect("write markdown");
    let before = fs::read(&word).expect("read word bytes");
    let report =
        office.replace(&word, "Acme", "Globex", &ReplaceOptions { dry_run: true, ..Default::default() }).expect("word dry run");
    assert_eq!(report["replaced"], json!(3));
    assert_eq!(report["dryRun"], json!(true));
    assert_eq!(report["saved"], Value::Null, "a dry run writes nothing");
    assert_eq!(fs::read(&word).expect("read word bytes"), before);
    assert_eq!(office.search(&word, "Acme", None).expect("search")["hits"].as_array().unwrap().len(), 3);

    let workbook = dir.join("budget.oexcel");
    let created = open_excel_workbook::create(&json!({ "title": "Dry" })).expect("create");
    let handle = handle_of(&created).expect("handle");
    open_excel_workbook::edit(
        handle,
        &json!({ "operations": [{ "op": "cell.set", "sheet": "Sheet1", "reference": "A1", "value": "Widget cost" }] }),
    )
    .expect("seed");
    open_excel_workbook::save(handle, &workbook, true).expect("save");
    let _ = open_excel_workbook::close(handle);
    let before = fs::read(&workbook).expect("read workbook bytes");
    let report =
        office.replace(&workbook, "Widget", "Gadget", &ReplaceOptions { dry_run: true, ..Default::default() }).expect("excel dry run");
    assert_eq!(report["replaced"], json!(1));
    assert_eq!(report["saved"], Value::Null);
    assert_eq!(fs::read(&workbook).expect("read workbook bytes"), before);
    assert_eq!(office.search(&workbook, "Widget", None).expect("search")["hits"].as_array().unwrap().len(), 1);

    let deck = open_ppt_deck::create(open_ppt_deck::CreateRequest {
        title: Some("Dry deck".to_owned()),
        size: None,
        theme: None,
        slides: vec!["Roadmap".to_owned()],
    })
    .expect("create deck");
    let project = dir.join("deck.project");
    open_ppt_deck::save(&deck, &project, true).expect("save project");
    let before = fs::read(&project).expect("read project bytes");
    let report = office
        .replace(&project, "Roadmap", "Plan", &ReplaceOptions { dry_run: true, ..Default::default() })
        .expect("presentation dry run");
    assert_eq!(report["replaced"], json!(1));
    assert_eq!(report["saved"], Value::Null);
    assert_eq!(fs::read(&project).expect("read project bytes"), before);

    let word_before = fs::read(&word).expect("read word bytes");
    let batch = office
        .replace_many(&[word.clone(), workbook.clone()], "Acme", "Globex", &ReplaceOptions { dry_run: true, ..Default::default() })
        .expect("batch dry run");
    assert_eq!(batch["dryRun"], json!(true));
    assert_eq!(batch["updated"], json!(2));
    assert_eq!(batch["results"][0]["replaced"], json!(3));
    assert_eq!(fs::read(&word).expect("read word bytes"), word_before, "the batch leaves the word file untouched");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn compare_can_fold_case_in_section_titles() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-compare-case-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let left = dir.join("left.md");
    fs::write(&left, b"# Intro\n\n## Details\n").expect("write left");
    let right = dir.join("right.md");
    fs::write(&right, b"# INTRO\n\n## details\n").expect("write right");

    let strict = office.compare(&left, &right).expect("strict compare");
    assert_eq!(strict["identical"], json!(false));
    assert_eq!(strict["ignoreCase"], json!(false));
    assert_eq!(strict["renamed"], json!([{ "from": "Intro", "to": "INTRO" }, { "from": "Details", "to": "details" }]));

    let folded = office.compare_with(&left, &right, true).expect("case-folded compare");
    assert_eq!(folded["identical"], json!(true));
    assert_eq!(folded["ignoreCase"], json!(true));
    assert_eq!(folded["added"], json!([]));
    assert_eq!(folded["removed"], json!([]));
    assert_eq!(folded["renamed"], json!([]));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn validate_opens_each_family_and_reports_the_engine_own_read() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-validate-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let markdown = dir.join("notes.md");
    fs::write(&markdown, b"# Title\n\nBody.\n").expect("write markdown");
    let created = open_excel_workbook::create(&json!({ "title": "Validated" })).expect("create");
    let handle = handle_of(&created).expect("handle");
    open_excel_workbook::edit(
        handle,
        &json!({ "operations": [{ "op": "cell.set", "sheet": "Sheet1", "reference": "A1", "value": 42 }] }),
    )
    .expect("seed");
    let sheet = dir.join("budget.oexcel");
    open_excel_workbook::save(handle, &sheet, true).expect("save");
    let _ = open_excel_workbook::close(handle);
    let deck = open_ppt_deck::create(open_ppt_deck::CreateRequest {
        title: Some("Validated".to_owned()),
        size: None,
        theme: None,
        slides: vec!["Intro".to_owned(), "Plan".to_owned()],
    })
    .expect("create deck");
    let slides = dir.join("deck.project");
    open_ppt_deck::save(&deck, &slides, true).expect("save deck");
    let pdf = two_page_pdf(&dir);

    let word = office.validate(&markdown).expect("validate word");
    assert_eq!(word["kind"], json!("word"));
    assert_eq!(word["readable"], json!(true));
    assert_eq!(word["container"], json!("plain"));
    assert!(word["revision"].is_number(), "the word engine reports its revision: {word}");
    assert_eq!(word["summary"]["sections"].as_array().expect("sections").len(), 1);
    assert!(word["losses"].as_array().expect("losses").is_empty());

    let workbook = office.validate(&sheet).expect("validate workbook");
    assert_eq!(workbook["kind"], json!("excel"));
    assert_eq!(workbook["summary"]["cells"], json!(1));
    assert_eq!(workbook["summary"]["sheets"].as_array().expect("sheets").len(), 1);
    assert!(workbook["losses"].as_array().expect("losses").is_empty());

    let presentation = office.validate(&slides).expect("validate deck");
    assert_eq!(presentation["kind"], json!("presentation"));
    assert_eq!(presentation["summary"]["slides"].as_array().expect("slides").len(), 2);
    assert!(presentation["losses"].as_array().expect("losses").is_empty());

    let document = office.validate(&pdf).expect("validate pdf");
    assert_eq!(document["kind"], json!("pdf"));
    assert_eq!(document["container"], json!("pdf"));
    assert_eq!(document["revision"], Value::Null);
    assert_eq!(document["summary"]["pages"], json!(2));
    assert!(document["warnings"].as_array().expect("warnings").is_empty());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn validate_many_reports_each_input_in_order() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-validate-many-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let source = dir.join("notes.md");
    fs::write(&source, b"# Title\n\nBody text.\n").expect("write markdown");
    let mismatched = dir.join("notes.pdf");
    fs::write(&mismatched, b"just words").expect("write mismatched");

    let inputs = vec![source.clone(), mismatched.clone(), PathBuf::from("missing-validate.docx")];
    let report = office.validate_many(&inputs).expect("validate batch");
    assert_eq!(report["validated"], json!(1));
    assert_eq!(report["failed"], json!(2));
    let results = report["results"].as_array().expect("results array");
    assert_eq!(results.len(), 3);
    assert_eq!(results[0]["source"], json!(source.display().to_string()));
    assert_eq!(results[0]["ok"], json!(true));
    assert_eq!(results[0]["kind"], json!("word"));
    assert_eq!(results[0]["container"], json!("plain"));
    assert!(results[0]["summary"].is_object());
    assert_eq!(results[1]["source"], json!(mismatched.display().to_string()));
    assert_eq!(results[1]["ok"], json!(false));
    assert_eq!(results[1]["code"], json!("container-mismatch"));
    assert_eq!(results[2]["source"], json!("missing-validate.docx"));
    assert_eq!(results[2]["ok"], json!(false));
    assert_eq!(results[2]["code"], json!("not-found"));

    assert!(office.validate_many(&[]).expect_err("empty inputs").is_usage());
    let unsupported = office.validate(&dir.join("notes.zip")).expect_err("unsupported extension");
    assert_eq!(unsupported.code(), "unsupported-extension");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn extract_writes_plain_text_for_every_family() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-extract-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let markdown = dir.join("notes.md");
    fs::write(&markdown, b"# Title\n\nBody.\n").expect("write markdown");
    let created = open_excel_workbook::create(&json!({ "title": "Extracted" })).expect("create");
    let handle = handle_of(&created).expect("handle");
    open_excel_workbook::edit(
        handle,
        &json!({ "operations": [{ "op": "cell.set", "sheet": "Sheet1", "reference": "A1", "value": 42 }] }),
    )
    .expect("seed");
    let sheet = dir.join("budget.oexcel");
    open_excel_workbook::save(handle, &sheet, true).expect("save");
    let _ = open_excel_workbook::close(handle);
    let deck = open_ppt_deck::create(open_ppt_deck::CreateRequest {
        title: Some("Extracted".to_owned()),
        size: None,
        theme: None,
        slides: vec!["Intro".to_owned(), "Plan".to_owned()],
    })
    .expect("create deck");
    let slides = dir.join("deck.project");
    open_ppt_deck::save(&deck, &slides, true).expect("save deck");
    let pdf = two_page_pdf(&dir);

    let word = office.extract(&markdown, &dir.join("notes.txt"), false).expect("extract word");
    assert_eq!(word["kind"], json!("word"));
    assert_eq!(word["format"], json!("txt"));
    let text = fs::read_to_string(dir.join("notes.txt")).expect("read word text");
    assert!(text.contains("Title") && text.contains("Body."), "the text projection kept the document: {text}");

    let workbook = office.extract(&sheet, &dir.join("budget.csv"), false).expect("extract workbook");
    assert_eq!(workbook["kind"], json!("excel"));
    assert_eq!(workbook["format"], json!("csv"));
    let grid = fs::read_to_string(dir.join("budget.csv")).expect("read grid");
    assert!(grid.contains("42"), "the cell grid kept the value: {grid}");

    let presentation = office.extract(&slides, &dir.join("deck.txt"), false).expect("extract deck");
    assert_eq!(presentation["kind"], json!("presentation"));
    assert_eq!(presentation["format"], json!("text"));
    let outline = fs::read_to_string(dir.join("deck.txt")).expect("read deck text");
    assert!(outline.contains("Intro") && outline.contains("Plan"), "the slide titles survived: {outline}");

    let document = office.extract(&pdf, &dir.join("source.md"), false).expect("extract pdf");
    assert_eq!(document["kind"], json!("pdf"));
    assert_eq!(document["format"], json!("markdown"));
    assert!(document["bytes"].as_u64().expect("bytes") > 0);
    assert!(document["losses"].as_array().expect("losses").is_empty());
    let markdown_text = fs::read_to_string(dir.join("source.md")).expect("read pdf text");
    assert!(markdown_text.contains("Alpha page") && markdown_text.contains("Beta page"), "both pages were extracted: {markdown_text}");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn extract_protects_targets_and_refuses_bad_routes() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-extract-guard-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let markdown = dir.join("notes.md");
    fs::write(&markdown, b"# Title\n\nBody.\n").expect("write markdown");
    let out = dir.join("notes.txt");
    fs::write(&out, b"keep me").expect("write target");

    let refused = office.extract(&markdown, &out, false).expect_err("existing target");
    assert_eq!(refused.code(), "target-exists");
    assert_eq!(fs::read_to_string(&out).expect("read target"), "keep me");

    let replaced = office.extract(&markdown, &out, true).expect("overwrite target");
    assert_eq!(replaced["out"], json!(out.display().to_string()));
    assert_ne!(fs::read_to_string(&out).expect("read target"), "keep me");

    let missing = office.extract(&dir.join("gone.docx"), &dir.join("gone.txt"), false).expect_err("missing source");
    assert_eq!(missing.code(), "not-found");
    let unsupported = office.extract(&dir.join("notes.zip"), &dir.join("notes.txt"), false).expect_err("unsupported source");
    assert_eq!(unsupported.code(), "unsupported-extension");
    let mismatched = dir.join("notes.pdf");
    fs::write(&mismatched, b"just words").expect("write mismatched");
    let mismatch = office.extract(&mismatched, &dir.join("mismatch.txt"), false).expect_err("mismatched container");
    assert_eq!(mismatch.code(), "container-mismatch");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn extract_many_writes_one_text_file_per_document() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-extract-many-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let markdown = dir.join("notes.md");
    fs::write(&markdown, b"# Title\n\nBody.\n").expect("write markdown");
    let pdf = two_page_pdf(&dir);
    let out_dir = dir.join("text");

    let inputs = vec![markdown.clone(), pdf.clone(), PathBuf::from("missing-extract.docx")];
    let report = office.extract_many(&inputs, &out_dir, false).expect("extract batch");
    assert_eq!(report["extracted"], json!(2));
    assert_eq!(report["failed"], json!(1));
    let results = report["results"].as_array().expect("results array");
    assert_eq!(results.len(), 3);
    assert_eq!(results[0]["source"], json!(markdown.display().to_string()));
    assert_eq!(results[0]["target"], json!(out_dir.join("notes.txt").display().to_string()));
    assert_eq!(results[0]["ok"], json!(true));
    assert_eq!(results[0]["kind"], json!("word"));
    assert_eq!(results[0]["format"], json!("txt"));
    assert!(out_dir.join("notes.txt").is_file());
    assert_eq!(results[1]["target"], json!(out_dir.join("source.md").display().to_string()));
    assert_eq!(results[1]["kind"], json!("pdf"));
    assert_eq!(results[1]["format"], json!("markdown"));
    assert!(out_dir.join("source.md").is_file());
    assert_eq!(results[2]["source"], json!("missing-extract.docx"));
    assert_eq!(results[2]["ok"], json!(false));
    assert_eq!(results[2]["code"], json!("not-found"));

    assert!(office.extract_many(&[], &out_dir, false).expect_err("empty inputs").is_usage());
    assert!(office.extract_many(&inputs, &PathBuf::from(""), false).expect_err("empty out dir").is_usage());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn routed_engine_failures_keep_the_engine_stable_code_and_params() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-engine-error-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");

    // An empty ZIP package routes to the Word engine and fails inside it,
    // so both the single-file error and the batch entry must carry the
    // engine's own stable code and typed parameters, never `runtime`.
    let empty_package = dir.join("empty.docx");
    fs::write(&empty_package, b"PK\x05\x06\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00")
        .expect("write empty zip");
    let engine_failure = open_word_document::open(&empty_package).expect_err("the engine rejects the empty package");
    let expected_params = engine_failure.params().to_json();

    let single = office.inspect(&empty_package, None).expect_err("routing keeps the engine failure");
    assert_eq!(single.code(), engine_failure.code());
    assert_ne!(single.code(), "runtime");
    assert_eq!(single.message(), engine_failure.message());
    assert_eq!(single.params(), Some(&expected_params));

    let report = office
        .replace_many(std::slice::from_ref(&empty_package), "Alpha", "Omega", &ReplaceOptions::default())
        .expect("replace batch");
    let results = report["results"].as_array().expect("results array");
    assert_eq!(results[0]["ok"], json!(false));
    assert_eq!(results[0]["code"], json!(engine_failure.code()));
    assert_eq!(results[0]["params"], expected_params);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn replaced_engine_display_text_keeps_host_branch_and_params() {
    // The host consumes the engine's stable code and frozen parameters only;
    // the display text is not part of the branch.
    let code = "session/argument-invalid";
    let params = json!({ "reason": "page 9 is outside the document", "pages": "9" });
    let original = open_office::project_engine_failure(code, "page 9 is outside the document", Some(params.clone()));
    let replaced = open_office::project_engine_failure(code, "被完全替换的本地化文案", Some(params.clone()));

    assert_eq!(original.code(), code);
    assert_eq!(replaced.code(), original.code(), "rewording the engine message must keep the machine branch");
    assert_eq!(replaced.params(), original.params(), "rewording the engine message must keep the frozen params");
    assert_ne!(original.message(), replaced.message(), "the test must actually replace the display text");

    let original_envelope = json!({ "code": original.code(), "params": original.params() });
    let replaced_envelope = json!({ "code": replaced.code(), "params": replaced.params() });
    assert_eq!(original_envelope, replaced_envelope);
}

#[test]
fn a_non_object_engine_payload_is_dropped() {
    for payload in [Value::Null, json!("scalar"), json!([1, 2, 3])] {
        let projected = open_office::project_engine_failure("io/read", "display text", Some(payload));
        assert_eq!(projected.code(), "io/read");
        assert_eq!(projected.params(), None, "only a frozen object payload survives projection");
    }
}

#[test]
fn publish_refuses_existing_target_and_leaves_it_untouched() {
    let dir = std::env::temp_dir().join(format!("open-office-publish-refuse-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let target = dir.join("page-1.svg");
    fs::write(&target, b"<svg>old</svg>").expect("seed target");

    let error = publish_output(&target, b"<svg>new</svg>", false, "unwritable").expect_err("a same-name target is refused");
    assert_eq!(error.code(), "target-exists");
    assert_eq!(fs::read(&target).expect("read back"), b"<svg>old</svg>", "the refused publish must not touch the target");
    let entries: Vec<PathBuf> = fs::read_dir(&dir).expect("read dir").map(|entry| entry.expect("entry").path()).collect();
    assert_eq!(entries, vec![target.clone()], "no staging file survives a refused publish");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn publish_replaces_existing_target_when_overwrite_is_allowed() {
    let dir = std::env::temp_dir().join(format!("open-office-publish-replace-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let target = dir.join("notes.txt");
    fs::write(&target, b"old text").expect("seed target");

    publish_output(&target, b"new text", true, "output-unwritable").expect("overwrite publish");
    assert_eq!(fs::read(&target).expect("read back"), b"new text");
    let entries: Vec<PathBuf> = fs::read_dir(&dir).expect("read dir").map(|entry| entry.expect("entry").path()).collect();
    assert_eq!(entries, vec![target.clone()], "the rename leaves no staging file behind");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn abandoned_publish_leaves_no_partial_file() {
    let dir = std::env::temp_dir().join(format!("open-office-publish-abandon-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let target = dir.join("page-2.svg");
    {
        let mut publisher = boost_fs::AtomicPublisher::create(&target, boost_fs::PublishOptions::default()).expect("stage the file");
        {
            use std::io::Write;
            publisher.write_all(b"<svg>half written").expect("stage bytes");
        }
        assert!(publisher.staging_path().exists(), "the staging file lives while publishing");
    } // dropped without committing

    assert!(!target.exists(), "an uncommitted publish never reaches the destination");
    assert_eq!(fs::read_dir(&dir).expect("read dir").count(), 0, "the staging file is removed on failure");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn concurrent_publish_contends_on_one_fresh_target() {
    let dir = std::env::temp_dir().join(format!("open-office-publish-contend-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let first = dir.join("shared.md");
    let second = first.clone();

    let winner = std::thread::spawn(move || publish_output(&first, b"first-payload", false, "unwritable"));
    let loser = std::thread::spawn(move || publish_output(&second, b"second-payload", false, "unwritable"));
    let outcomes = [winner.join().expect("thread"), loser.join().expect("thread")];

    assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1, "exactly one publisher claims a fresh same-name target");
    let refused = outcomes.iter().find(|outcome| outcome.is_err()).expect("one publish refused");
    assert_eq!(refused.as_ref().err().expect("error").code(), "target-exists");
    let bytes = fs::read(&dir.join("shared.md")).expect("winner bytes");
    assert!(bytes == b"first-payload" || bytes == b"second-payload", "the winner file is complete, never mixed");
    assert_eq!(fs::read_dir(&dir).expect("read dir").count(), 1, "the loser leaves no staging file");

    let _ = fs::remove_dir_all(&dir);
}

/// S4 suite acceptance: one `Office` instance (the single binary) opens, edits,
/// saves and re-verifies a real DOCX, XLSX, PPTX and PDF inside one process.
///
/// Each fixture is built with the owning engine's minimal construction and
/// exported under its exchange extension, then every later step goes through
/// the suite facade only. PDFs have no in-place find-and-replace route in this
/// build, so the deepest verified path for that family is asserted explicitly:
/// the refusal, an engine-level bookmark edit read back through the suite, and
/// a text round-trip (PDF → Markdown → edit → PDF) verified after reopening.
#[test]
fn single_binary_open_edit_save_reopen_across_four_families() {
    let office = Office::new();
    let dir = std::env::temp_dir().join(format!("open-office-single-binary-four-families-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");

    // --- DOCX: build once through the word engine, then live entirely in the suite.
    let created = open_word_document::create(&json!({ "title": "Quarterly" })).expect("create word");
    let handle = handle_of(&created).expect("word handle");
    open_word_document::edit(
        handle,
        &json!({ "operations": [
            { "type": "heading.append", "level": 1, "text": "Overview" },
            { "type": "paragraph.append", "text": "Acme Corp delivered record revenue." },
        ] }),
    )
    .expect("seed word");
    let docx = dir.join("quarterly.docx");
    open_word_document::export(handle, "docx", &docx, true).expect("export docx");
    let _ = open_word_document::close(handle);

    let route = office.open(&docx, true).expect("route docx");
    assert_eq!(route["kind"], json!("word"));
    let probe = office.probe(&docx).expect("probe docx");
    assert_eq!(probe["format"], json!("docx"), "the package part identifies the Word package");
    assert_eq!(probe["container"], json!("zip"));
    assert_eq!(probe["confidence"], json!("verified"));
    let opened = office.validate(&docx).expect("validate docx");
    assert_eq!(opened["readable"], json!(true));
    assert_eq!(office.search(&docx, "Acme", None).expect("search before")["hits"].as_array().expect("hits").len(), 1);

    let report = office.replace(&docx, "Acme", "Globex", &ReplaceOptions::default()).expect("replace docx");
    assert_eq!(report["kind"], json!("word"));
    assert_eq!(report["replaced"], json!(1));
    assert_eq!(report["saved"]["format"], json!("docx"), "the edit is written back under the same extension");

    assert_eq!(office.search(&docx, "Globex", None).expect("search after")["hits"].as_array().expect("hits").len(), 1);
    assert!(office.search(&docx, "Acme", None).expect("search stale")["hits"].as_array().expect("hits").is_empty());
    let outline = office.outline(&docx, false, None).expect("outline docx");
    assert_eq!(outline["outline"]["items"][0]["title"], json!("Overview"));
    let stats = office.stats(&docx).expect("stats docx");
    assert_eq!(stats["kind"], json!("word"));

    // --- XLSX: the workbook engine seeds cells, the suite edits the package.
    let created = open_excel_workbook::create(&json!({ "title": "Budget" })).expect("create workbook");
    let handle = handle_of(&created).expect("workbook handle");
    open_excel_workbook::edit(
        handle,
        &json!({ "operations": [
            { "op": "cell.set", "sheet": "Sheet1", "reference": "A1", "value": "Item" },
            { "op": "cell.set", "sheet": "Sheet1", "reference": "B1", "value": "Widget cost" },
            { "op": "cell.set", "sheet": "Sheet1", "reference": "B2", "value": "Two Widgets" },
        ] }),
    )
    .expect("seed workbook");
    let xlsx = dir.join("budget.xlsx");
    open_excel_workbook::export(handle, "xlsx", &xlsx, true).expect("export xlsx");
    let _ = open_excel_workbook::close(handle);

    let route = office.open(&xlsx, true).expect("route xlsx");
    assert_eq!(route["kind"], json!("excel"));
    let probe = office.probe(&xlsx).expect("probe xlsx");
    assert_eq!(probe["format"], json!("xlsx"));
    assert_eq!(probe["container"], json!("zip"));
    assert_eq!(probe["confidence"], json!("verified"));
    let opened = office.validate(&xlsx).expect("validate xlsx");
    assert_eq!(opened["readable"], json!(true));
    assert_eq!(office.search(&xlsx, "Widget", None).expect("search before")["hits"].as_array().expect("hits").len(), 2);

    let report = office.replace(&xlsx, "Widget", "Gadget", &ReplaceOptions::default()).expect("replace xlsx");
    assert_eq!(report["kind"], json!("excel"));
    assert_eq!(report["replaced"], json!(2));
    assert_eq!(report["saved"]["format"], json!("xlsx"));

    assert_eq!(office.search(&xlsx, "Gadget", None).expect("search after")["hits"].as_array().expect("hits").len(), 2);
    assert!(office.search(&xlsx, "Widget", None).expect("search stale")["hits"].as_array().expect("hits").is_empty());
    let outline = office.outline(&xlsx, false, None).expect("outline xlsx");
    assert_eq!(outline["outline"]["sheets"][0]["name"], json!("Sheet1"));

    // --- PPTX: the deck engine authors slides, the suite edits the package.
    let deck = open_ppt_deck::create(open_ppt_deck::CreateRequest {
        title: Some("Roadmap".to_owned()),
        size: None,
        theme: None,
        slides: vec!["Roadmap".to_owned(), "Roadmap review".to_owned()],
    })
    .expect("create deck");
    let pptx = dir.join("roadmap.pptx");
    open_ppt_deck::export(&deck, &json!({ "format": "pptx", "target": pptx.display().to_string(), "overwrite": true }))
        .expect("export pptx");

    let route = office.open(&pptx, true).expect("route pptx");
    assert_eq!(route["kind"], json!("presentation"));
    let probe = office.probe(&pptx).expect("probe pptx");
    assert_eq!(probe["format"], json!("pptx"));
    assert_eq!(probe["container"], json!("zip"));
    assert_eq!(probe["confidence"], json!("verified"));
    let opened = office.validate(&pptx).expect("validate pptx");
    assert_eq!(opened["readable"], json!(true));
    assert_eq!(office.search(&pptx, "Roadmap", None).expect("search before")["hits"].as_array().expect("hits").len(), 2);

    let report = office.replace(&pptx, "Roadmap", "Plan", &ReplaceOptions::default()).expect("replace pptx");
    assert_eq!(report["kind"], json!("presentation"));
    assert_eq!(report["replaced"], json!(2));
    assert_eq!(report["saved"]["format"], json!("pptx"));

    assert_eq!(office.search(&pptx, "Plan", None).expect("search after")["hits"].as_array().expect("hits").len(), 2);
    assert!(office.search(&pptx, "Roadmap", None).expect("search stale")["hits"].as_array().expect("hits").is_empty());
    let outline = office.outline(&pptx, true, None).expect("outline pptx");
    assert_eq!(outline["outline"]["slides"][0]["title"], json!("Plan"));
    assert_eq!(outline["outline"]["slides"][1]["title"], json!("Plan review"));

    // --- PDF: no in-place find-and-replace route, so the deepest verified path is asserted.
    let paper = dir.join("paper.pdf");
    let request = open_pdf_core::CreateRequest {
        out: paper.clone(),
        title: Some("Quarterly".to_owned()),
        pages: vec!["Alpha page".to_string(), "Beta page".to_string()],
        overwrite: true,
        ..Default::default()
    };
    open_pdf_core::create(&request).expect("create pdf");

    let route = office.open(&paper, true).expect("route pdf");
    assert_eq!(route["kind"], json!("pdf"));
    let probe = office.probe(&paper).expect("probe pdf");
    assert_eq!(probe["format"], json!("pdf"));
    assert_eq!(probe["container"], json!("pdf"));
    assert_eq!(probe["confidence"], json!("verified"));
    let opened = office.validate(&paper).expect("validate pdf");
    assert_eq!(opened["readable"], json!(true));
    assert_eq!(opened["summary"]["pages"], json!(2));
    assert_eq!(office.search(&paper, "Beta page", None).expect("search pdf")["hits"].as_array().expect("hits").len(), 1);
    let metadata = office.metadata(&paper).expect("metadata pdf");
    assert_eq!(metadata["metadata"]["title"], json!("Quarterly"));

    let refused = office.replace(&paper, "Alpha", "First", &ReplaceOptions::default()).expect_err("pdfs have no replace route");
    assert_eq!(refused.code(), "unsupported-target");
    assert!(refused.is_usage());

    let marked = dir.join("marked.pdf");
    open_pdf_core::set_bookmarks(
        &paper,
        &marked,
        &[
            open_pdf_core::Bookmark { title: "Chapter One".to_owned(), page: 1, depth: Some(0) },
            open_pdf_core::Bookmark { title: "Chapter Two".to_owned(), page: 2, depth: Some(1) },
        ],
    )
    .expect("bookmark edit");
    let outline = office.outline(&marked, false, None).expect("outline marked pdf");
    assert_eq!(outline["outline"]["pages"], json!(2));
    assert_eq!(outline["outline"]["bookmarks"][0]["title"], json!("Chapter One"));
    assert_eq!(outline["outline"]["bookmarks"][1]["title"], json!("Chapter Two"));
    assert_eq!(outline["outline"]["bookmarks"][1]["depth"], json!(2));
    assert_eq!(office.search(&marked, "Alpha page", None).expect("search marked pdf")["hits"].as_array().expect("hits").len(), 1);

    let extracted = dir.join("paper.md");
    office.convert(&paper, "md", &extracted, false).expect("pdf to markdown");
    let markdown = fs::read_to_string(&extracted).expect("read markdown");
    assert!(markdown.contains("Alpha page") && markdown.contains("Beta page"), "both pages were extracted: {markdown}");
    let report = office.replace(&extracted, "Alpha", "First", &ReplaceOptions::default()).expect("edit the projection");
    assert_eq!(report["kind"], json!("word"));
    assert_eq!(report["replaced"], json!(1));
    let rebuilt = dir.join("rebuilt.pdf");
    office.convert(&extracted, "pdf", &rebuilt, false).expect("markdown back to pdf");
    assert_eq!(office.search(&rebuilt, "First page", None).expect("search rebuilt")["hits"].as_array().expect("hits").len(), 1);
    assert!(office.search(&rebuilt, "Alpha page", None).expect("search rebuilt stale")["hits"].as_array().expect("hits").is_empty());
    let stats = office.stats(&rebuilt).expect("stats rebuilt");
    assert!(stats["stats"]["pages"].as_u64().expect("pages") >= 1, "the rebuilt PDF is a real paged document: {stats}");

    let _ = fs::remove_dir_all(&dir);
}
