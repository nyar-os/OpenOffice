//! Library API of `open-office`.
//!
//! The Office crate does not grow a fifth document model. It detects the file
//! family, opens it through that engine's own public API, and dispatches
//! inspection and conversion. Every report comes from the engine that did the
//! work, so capability claims stay identical to the individual products.
//!
//! The command line in `main.rs` is a thin parser over [`Office`].

use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

use document_formats::detect::{ContainerKind, DetectionConfidence, detect_bytes};
use serde_json::{Value, json};

/// Crate version reported by `capabilities` and `version`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Document families this build can route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Word,
    Excel,
    Presentation,
    Pdf,
}

impl Kind {
    /// Stable name used in reports.
    pub fn label(self) -> &'static str {
        match self {
            Self::Word => "word",
            Self::Excel => "excel",
            Self::Presentation => "presentation",
            Self::Pdf => "pdf",
        }
    }

    /// Import extensions each family accepts, matching [`Kind::of_path`].
    pub fn imports(self) -> &'static [&'static str] {
        match self {
            Self::Word => &["docx", "docm", "rtf", "txt", "md", "markdown", "html", "htm", "oword"],
            Self::Excel => &["xlsx", "xlsm", "csv", "tsv", "oexcel"],
            Self::Presentation => &["pptx", "pptm", "ppsx", "ppsm", "potx", "potm", "project", "oppt", "opresent"],
            Self::Pdf => &["pdf"],
        }
    }

    /// Export formats each family actually wires.
    pub fn exports(self) -> &'static [&'static str] {
        match self {
            Self::Word => &["docx", "pdf", "html", "md", "txt", "rtf"],
            Self::Excel => &["xlsx", "csv", "tsv", "json", "html", "pdf"],
            Self::Presentation => &["pptx", "ppsx", "potx", "pdf"],
            Self::Pdf => &["md", "txt", "text"],
        }
    }

    /// Resolves the family from the file extension.
    pub fn of_path(path: &Path) -> Result<Self, OfficeError> {
        let extension = path.extension().and_then(|value| value.to_str()).unwrap_or_default().to_ascii_lowercase();
        if extension.is_empty() {
            return Err(OfficeError::runtime(format!("{} has no file extension", path.display())));
        }
        [Self::Word, Self::Excel, Self::Presentation, Self::Pdf]
            .into_iter()
            .find(|kind| kind.imports().contains(&extension.as_str()))
            .ok_or_else(|| {
                OfficeError::runtime(format!(
                    "`{extension}` is not a word, workbook, presentation or PDF format in this build"
                ))
            })
    }
}

/// Library failure with a stable distinction between usage and runtime errors.
#[derive(Debug)]
pub struct OfficeError {
    message: String,
    usage: bool,
}

impl OfficeError {
    /// A caller mistake: unknown option, missing argument or unsupported target.
    pub fn usage(message: impl Into<String>) -> Self {
        Self { message: message.into(), usage: true }
    }

    /// A failure while reading, converting or writing a document.
    pub fn runtime(message: impl Into<String>) -> Self {
        Self { message: message.into(), usage: false }
    }

    /// Human-readable detail, without the `open-office:` prefix.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Whether the failure is a usage error (exit code 2) or a runtime error (1).
    pub fn is_usage(&self) -> bool {
        self.usage
    }
}

impl fmt::Display for OfficeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for OfficeError {}

/// The four-engine inspector and converter.
#[derive(Debug, Clone, Copy, Default)]
pub struct Office;

impl Office {
    /// Creates a stateless office session.
    pub fn new() -> Self {
        Self
    }

    /// Version, routed commands, module capabilities and conversion targets.
    pub fn capabilities(&self) -> Value {
        json!({
            "engine": { "package": "open-office", "version": VERSION },
            "commands": ["probe", "inspect", "convert", "formats", "stats", "capabilities", "version"],
            "modules": {
                "word": open_word_document::capabilities(),
                "excel": open_excel_workbook::capabilities(),
                "presentation": open_ppt_deck::capabilities(),
                "pdf": open_pdf_core::capabilities(),
            },
            "convert": {
                "word": Kind::Word.exports(),
                "excel": Kind::Excel.exports(),
                "presentation": Kind::Presentation.exports(),
                "pdf": Kind::Pdf.exports(),
            },
        })
    }

    /// Import extensions and export targets for every document family.
    pub fn formats(&self) -> Value {
        json!({
            "word": { "import": Kind::Word.imports(), "export": Kind::Word.exports() },
            "excel": { "import": Kind::Excel.imports(), "export": Kind::Excel.exports() },
            "presentation": { "import": Kind::Presentation.imports(), "export": Kind::Presentation.exports() },
            "pdf": { "import": Kind::Pdf.imports(), "export": Kind::Pdf.exports() },
        })
    }

    /// Probes what a file's bytes prove about its format, using the extension as a hint only.
    ///
    /// Unlike [`Office::inspect`] this needs no routed extension and never opens
    /// the document. The report names the container and confidence, keeps every
    /// candidate still possible and quotes the evidence, so an ambiguous ZIP
    /// package is described rather than guessed.
    pub fn probe(&self, path: &Path) -> Result<Value, OfficeError> {
        let bytes = fs::read(path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                OfficeError::runtime(format!("{} does not exist", path.display()))
            }
            else {
                OfficeError::runtime(format!("cannot read {}: {error}", path.display()))
            }
        })?;
        let extension = path.extension().and_then(|value| value.to_str()).map(str::to_ascii_lowercase);
        let detection = detect_bytes(&bytes, extension.as_deref());
        Ok(json!({
            "path": path.display().to_string(),
            "bytes": bytes.len(),
            "extension": extension,
            "format": detection.format,
            "container": container_label(detection.container),
            "confidence": confidence_label(detection.confidence),
            "candidates": detection.candidates,
            "evidence": detection.evidence,
        }))
    }

    /// Reports lightweight per-family statistics for one document.
    pub fn stats(&self, path: &Path) -> Result<Value, OfficeError> {
        let kind = Kind::of_path(path)?;
        verify_container(path, kind)?;
        let stats = match kind {
            Kind::Word => {
                let opened = open_word_document::open(path).map_err(runtime)?;
                let handle = handle_of(&opened)?;
                let outcome = open_word_document::stats(handle);
                let _ = open_word_document::close(handle);
                outcome.map_err(runtime)?
            }
            Kind::Excel => {
                let opened = open_excel_workbook::open(path).map_err(runtime)?;
                let handle = handle_of(&opened)?;
                let outcome = open_excel_workbook::summary(handle);
                let _ = open_excel_workbook::close(handle);
                outcome.map_err(runtime)?
            }
            Kind::Presentation => {
                let loaded = load_deck(path)?;
                let mut stats = open_ppt_deck::summary(&loaded.deck);
                if let Some(report) = stats.as_object_mut() {
                    report.insert("losses".to_string(), json!(loaded.losses));
                }
                stats
            }
            Kind::Pdf => {
                let report = open_pdf_core::inspect(path).map_err(runtime)?;
                json!({
                    "pages": report.pages,
                    "version": report.version,
                    "encrypted": report.encrypted,
                })
            }
        };
        Ok(json!({
            "kind": kind.label(),
            "path": path.display().to_string(),
            "stats": stats,
        }))
    }

    /// Inspects a document by dispatching to the owning engine.
    pub fn inspect(&self, path: &Path) -> Result<Value, OfficeError> {
        let kind = Kind::of_path(path)?;
        verify_container(path, kind)?;
        match kind {
            Kind::Word => {
                let opened = open_word_document::open(path).map_err(runtime)?;
                let handle = handle_of(&opened)?;
                let summary = open_word_document::summary(handle).map_err(runtime)?;
                let stats = open_word_document::stats(handle).map_err(runtime)?;
                let report = open_word_document::inspect(handle, None).map_err(runtime)?;
                let _ = open_word_document::close(handle);
                Ok(json!({
                    "kind": kind.label(),
                    "path": path.display().to_string(),
                    "summary": summary,
                    "stats": stats,
                    "inspect": report,
                }))
            }
            Kind::Excel => {
                let opened = open_excel_workbook::open(path).map_err(runtime)?;
                let handle = handle_of(&opened)?;
                let summary = open_excel_workbook::summary(handle).map_err(runtime)?;
                let report = open_excel_workbook::inspect(handle, None).map_err(runtime)?;
                let _ = open_excel_workbook::close(handle);
                Ok(json!({
                    "kind": kind.label(),
                    "path": path.display().to_string(),
                    "summary": summary,
                    "inspect": report,
                }))
            }
            Kind::Presentation => {
                let loaded = load_deck(path)?;
                let summary = open_ppt_deck::summary(&loaded.deck);
                Ok(json!({
                    "kind": kind.label(),
                    "path": path.display().to_string(),
                    "summary": summary,
                    "losses": loaded.losses,
                }))
            }
            Kind::Pdf => {
                let report = open_pdf_core::inspect(path).map_err(runtime)?;
                let report = serde_json::to_value(report).map_err(|error| OfficeError::runtime(error.to_string()))?;
                Ok(json!({
                    "kind": kind.label(),
                    "path": path.display().to_string(),
                    "inspect": report,
                }))
            }
        }
    }

    /// Converts one document into a format the owning engine writes.
    pub fn convert(
        &self,
        input: &Path,
        format: &str,
        out: &Path,
        overwrite: bool,
    ) -> Result<Value, OfficeError> {
        let format = format.to_ascii_lowercase();
        let kind = Kind::of_path(input)?;
        if !kind.exports().contains(&format.as_str()) {
            return Err(OfficeError::usage(format!(
                "{} cannot be converted to `{format}`; supported targets: {}",
                kind.label(),
                kind.exports().join(", ")
            )));
        }
        verify_container(input, kind)?;
        if out.exists() && !overwrite {
            return Err(OfficeError::runtime(format!(
                "{} already exists; pass --overwrite to replace it",
                out.display()
            )));
        }

        let report = match kind {
            Kind::Word => {
                let opened = open_word_document::open(input).map_err(runtime)?;
                let handle = handle_of(&opened)?;
                let report = open_word_document::export(handle, &format, out, overwrite).map_err(runtime)?;
                let _ = open_word_document::close(handle);
                report
            }
            Kind::Excel => {
                let opened = open_excel_workbook::open(input).map_err(runtime)?;
                let handle = handle_of(&opened)?;
                let report = open_excel_workbook::export(handle, &format, out, overwrite).map_err(runtime)?;
                let _ = open_excel_workbook::close(handle);
                report
            }
            Kind::Presentation => {
                let loaded = load_deck(input)?;
                open_ppt_deck::export(
                    &loaded.deck,
                    &json!({ "format": format, "target": out.display().to_string(), "overwrite": overwrite }),
                )
                .map_err(runtime)?
            }
            Kind::Pdf => extract_pdf_text(input, out)?,
        };
        Ok(json!({
            "source": input.display().to_string(),
            "kind": kind.label(),
            "format": format,
            "target": out.display().to_string(),
            "report": report,
        }))
    }

    /// Converts many documents into `out_dir`, naming each target after its source stem.
    ///
    /// A failing document does not stop the batch: every result is reported in
    /// input order with `ok`, and `report` or `error` alongside.
    pub fn convert_many(
        &self,
        inputs: &[PathBuf],
        format: &str,
        out_dir: &Path,
        overwrite: bool,
    ) -> Result<Value, OfficeError> {
        if inputs.is_empty() {
            return Err(OfficeError::usage("at least one input document is required"));
        }
        let format = format.trim().to_ascii_lowercase();
        if format.is_empty() {
            return Err(OfficeError::usage("`--format <format>` is required"));
        }
        if out_dir.as_os_str().is_empty() {
            return Err(OfficeError::usage("`--out-dir <dir>` is required"));
        }
        fs::create_dir_all(out_dir).map_err(|error| {
            OfficeError::runtime(format!("cannot create {}: {error}", out_dir.display()))
        })?;

        let mut converted = 0;
        let mut failed = 0;
        let mut results = Vec::with_capacity(inputs.len());
        for input in inputs {
            let stem = input.file_stem().map(|value| value.to_string_lossy().into_owned()).unwrap_or_default();
            let target = out_dir.join(format!("{stem}.{format}"));
            match self.convert(input, &format, &target, overwrite) {
                Ok(report) => {
                    converted += 1;
                    results.push(json!({
                        "source": input.display().to_string(),
                        "target": target.display().to_string(),
                        "ok": true,
                        "report": report.get("report").cloned().unwrap_or(report),
                    }));
                }
                Err(error) => {
                    failed += 1;
                    results.push(json!({
                        "source": input.display().to_string(),
                        "target": target.display().to_string(),
                        "ok": false,
                        "error": error.message(),
                    }));
                }
            }
        }
        Ok(json!({ "converted": converted, "failed": failed, "results": results }))
    }
}

/// Cross-checks the extension route against what the bytes really are.
///
/// The probe reads the whole file, not just the header: inside a ZIP container a
/// Word, Spreadsheet and Presentation package all look alike until the parts are
/// read, so a header-only check could not tell XLSX from PPTX. A resolved family
/// that contradicts the route is refused with the probe's own evidence.
fn verify_container(path: &Path, kind: Kind) -> Result<(), OfficeError> {
    if !path.exists() {
        return Err(OfficeError::runtime(format!("{} does not exist", path.display())));
    }
    let bytes = fs::read(path).map_err(|error| OfficeError::runtime(format!("cannot read {}: {error}", path.display())))?;
    let extension = path.extension().and_then(|value| value.to_str()).unwrap_or_default();
    let detection = detect_bytes(&bytes, Some(extension));
    let mismatch = match (kind, detection.container) {
        (Kind::Pdf, ContainerKind::Pdf) => false,
        (Kind::Pdf, _) => true,
        (_, ContainerKind::Pdf) => true,
        _ => match detection.format.and_then(family_of) {
            Some(family) => family != kind,
            None => false,
        },
    };
    if !mismatch {
        return Ok(());
    }
    Err(OfficeError::runtime(format!(
        "{} is routed as {} by its extension, but {}",
        path.display(),
        kind.label(),
        detection.evidence
    )))
}

/// Stable container name used in probe reports.
fn container_label(container: ContainerKind) -> &'static str {
    match container {
        ContainerKind::Plain => "plain",
        ContainerKind::Zip => "zip",
        ContainerKind::OleCompound => "ole-compound",
        ContainerKind::Pdf => "pdf",
    }
}

/// Stable confidence name used in probe reports.
fn confidence_label(confidence: DetectionConfidence) -> &'static str {
    match confidence {
        DetectionConfidence::Verified => "verified",
        DetectionConfidence::Hinted => "hinted",
        DetectionConfidence::Ambiguous => "ambiguous",
        DetectionConfidence::Unknown => "unknown",
    }
}

/// Family a detected format id names, when it names one of the routed families.
fn family_of(format: &str) -> Option<Kind> {
    match format {
        "docx" | "doc" | "rtf" => Some(Kind::Word),
        "xlsx" => Some(Kind::Excel),
        "pptx" => Some(Kind::Presentation),
        "pdf" => Some(Kind::Pdf),
        _ => None,
    }
}

/// Loads a native presentation project or imports a PresentationML package.
struct LoadedDeck {
    deck: open_ppt_deck::EditableDeck,
    losses: Vec<Value>,
}

fn load_deck(path: &Path) -> Result<LoadedDeck, OfficeError> {
    let extension = path.extension().and_then(|value| value.to_str()).unwrap_or_default().to_ascii_lowercase();
    if matches!(extension.as_str(), "project" | "oppt" | "opresent") {
        let deck = open_ppt_deck::open(path).map_err(runtime)?;
        Ok(LoadedDeck { deck, losses: Vec::new() })
    }
    else {
        let outcome = open_ppt_deck::import_pptx(path).map_err(runtime)?;
        Ok(LoadedDeck { deck: outcome.deck, losses: outcome.losses })
    }
}

/// Writes the extracted Markdown projection of a PDF to a text target.
fn extract_pdf_text(input: &Path, out: &Path) -> Result<Value, OfficeError> {
    let report = open_pdf_core::extract_text(input).map_err(runtime)?;
    fs::write(out, report.markdown.as_bytes())
        .map_err(|error| OfficeError::runtime(format!("cannot write {}: {error}", out.display())))?;
    Ok(json!({
        "bytes": report.markdown.len(),
        "losses": report.losses,
    }))
}

fn handle_of(opened: &Value) -> Result<u64, OfficeError> {
    opened
        .get("handle")
        .and_then(Value::as_u64)
        .ok_or_else(|| OfficeError::runtime("the engine did not return a document handle"))
}

fn runtime(error: impl fmt::Display) -> OfficeError {
    OfficeError::runtime(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convert_many_requires_inputs_format_and_out_dir() {
        let office = Office::new();
        let dir = Path::new("out");
        assert!(office.convert_many(&[], "pdf", dir, false).expect_err("empty inputs").is_usage());
        assert!(
            office.convert_many(&[PathBuf::from("a.docx")], "  ", dir, false).expect_err("empty format").is_usage()
        );
        assert!(
            office.convert_many(&[PathBuf::from("a.docx")], "pdf", Path::new(""), false)
                .expect_err("empty out dir")
                .is_usage()
        );
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
        assert!(results[0]["error"].is_string());
        assert_eq!(results[1]["ok"], json!(false));
        let _ = fs::remove_dir_all(&out_dir);
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
        assert!(error.message().contains("plain text has no container header"), "the probe evidence is quoted: {}", error.message());
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
}
