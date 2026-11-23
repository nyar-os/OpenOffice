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
