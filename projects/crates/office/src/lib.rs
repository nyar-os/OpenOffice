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
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
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
            return Err(OfficeError::runtime(format!("{} has no file extension", path.display())).with_code("missing-extension"));
        }
        [Self::Word, Self::Excel, Self::Presentation, Self::Pdf]
            .into_iter()
            .find(|kind| kind.imports().contains(&extension.as_str()))
            .ok_or_else(|| {
                OfficeError::runtime(format!(
                    "`{extension}` is not a word, workbook, presentation or PDF format in this build"
                ))
                .with_code("unsupported-extension")
            })
    }
}

/// Library failure with a stable distinction between usage and runtime errors.
#[derive(Debug)]
pub struct OfficeError {
    message: String,
    usage: bool,
    code: &'static str,
}

impl OfficeError {
    /// A caller mistake: unknown option, missing argument or unsupported target.
    pub fn usage(message: impl Into<String>) -> Self {
        Self { message: message.into(), usage: true, code: "usage" }
    }

    /// A failure while reading, converting or writing a document.
    pub fn runtime(message: impl Into<String>) -> Self {
        Self { message: message.into(), usage: false, code: "runtime" }
    }

    /// Refines the failure with a stable machine-readable code.
    pub fn with_code(mut self, code: &'static str) -> Self {
        self.code = code;
        self
    }

    /// Human-readable detail, without the `open-office:` prefix.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Stable machine-readable code, safe for callers to branch on.
    ///
    /// Unrefined failures report `usage` or `runtime`; the concrete cause is
    /// named by sites such as `not-found`, `unsupported-target` or
    /// `conversion-failed`.
    pub fn code(&self) -> &'static str {
        self.code
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

/// Options that refine one conversion request.
///
/// Defaults preserve the behaviour of [`Office::convert`].
#[derive(Debug, Clone, Default)]
pub struct ConvertOptions {
    /// One-based page ranges such as `1-3,5`, for PDF text targets only.
    pub pages: Option<String>,
}

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
            "commands": ["probe", "inspect", "convert", "formats", "stats", "outline", "capabilities", "version"],
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
                OfficeError::runtime(format!("{} does not exist", path.display())).with_code("not-found")
            }
            else {
                OfficeError::runtime(format!("cannot read {}: {error}", path.display())).with_code("unreadable")
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

    /// Projects one document into its navigable outline.
    ///
    /// Each family answers with the structure it really owns: a word document
    /// nests its sections by heading level, a workbook lists its sheets, a
    /// presentation lists its slides (and, with `points`, their talking points),
    /// and a PDF reports its outline bookmarks. `points` is refused for families
    /// that have no talking points instead of being silently ignored.
    pub fn outline(&self, path: &Path, points: bool) -> Result<Value, OfficeError> {
        let kind = Kind::of_path(path)?;
        if points && kind != Kind::Presentation {
            return Err(OfficeError::usage("`--points` only applies to presentations").with_code("unsupported-option"));
        }
        verify_container(path, kind)?;
        let outline = match kind {
            Kind::Word => {
                let opened = open_word_document::open(path).map_err(runtime)?;
                let handle = handle_of(&opened)?;
                let outcome = open_word_document::outline(handle);
                let _ = open_word_document::close(handle);
                outcome.map_err(runtime)?
            }
            Kind::Excel => {
                let opened = open_excel_workbook::open(path).map_err(runtime)?;
                let handle = handle_of(&opened)?;
                let summary = open_excel_workbook::summary(handle);
                let _ = open_excel_workbook::close(handle);
                let summary = summary.map_err(runtime)?;
                json!({
                    "revision": summary.get("revision").cloned().unwrap_or(Value::Null),
                    "sheets": summary.get("sheets").cloned().unwrap_or_else(|| json!([])),
                })
            }
            Kind::Presentation => {
                let loaded = load_deck(path)?;
                open_ppt_deck::outline::outline(&loaded.deck, &json!({ "points": points })).map_err(runtime)?
            }
            Kind::Pdf => {
                let report = open_pdf_core::inspect(path).map_err(runtime)?;
                json!({
                    "pages": report.pages,
                    "bookmarks": serde_json::to_value(&report.bookmarks)
                        .map_err(|error| OfficeError::runtime(error.to_string()))?,
                })
            }
        };
        Ok(json!({
            "kind": kind.label(),
            "path": path.display().to_string(),
            "outline": outline,
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
    pub fn convert(&self, input: &Path, format: &str, out: &Path, overwrite: bool) -> Result<Value, OfficeError> {
        self.convert_with(input, format, out, overwrite, &ConvertOptions::default())
    }

    /// Converts one document with explicit options.
    ///
    /// [`ConvertOptions::pages`] limits a PDF text conversion to one-based page
    /// ranges such as `1-3,5`; every other conversion ignores it because no
    /// engine accepts a partial export.
    pub fn convert_with(
        &self,
        input: &Path,
        format: &str,
        out: &Path,
        overwrite: bool,
        options: &ConvertOptions,
    ) -> Result<Value, OfficeError> {
        let format = format.to_ascii_lowercase();
        let kind = Kind::of_path(input)?;
        if let Some(pages) = &options.pages {
            if kind != Kind::Pdf {
                return Err(OfficeError::usage("`--pages` only applies to PDF text conversions").with_code("unsupported-option"));
            }
            if pages.trim().is_empty() {
                return Err(OfficeError::usage("`--pages` needs at least one page range").with_code("invalid-pages"));
            }
        }
        if !kind.exports().contains(&format.as_str()) {
            return Err(OfficeError::usage(format!(
                "{} cannot be converted to `{format}`; supported targets: {}",
                kind.label(),
                kind.exports().join(", ")
            ))
            .with_code("unsupported-target"));
        }
        verify_container(input, kind)?;
        if out.exists() && !overwrite {
            return Err(OfficeError::runtime(format!(
                "{} already exists; pass --overwrite to replace it",
                out.display()
            ))
            .with_code("target-exists"));
        }

        let report = match kind {
            Kind::Word => {
                let opened = open_word_document::open(input).map_err(conversion)?;
                let handle = handle_of(&opened)?;
                let report = open_word_document::export(handle, &format, out, overwrite).map_err(conversion)?;
                let _ = open_word_document::close(handle);
                report
            }
            Kind::Excel => {
                let opened = open_excel_workbook::open(input).map_err(conversion)?;
                let handle = handle_of(&opened)?;
                let report = open_excel_workbook::export(handle, &format, out, overwrite).map_err(conversion)?;
                let _ = open_excel_workbook::close(handle);
                report
            }
            Kind::Presentation => {
                let loaded = load_deck(input).map_err(|error| error.with_code("conversion-failed"))?;
                open_ppt_deck::export(
                    &loaded.deck,
                    &json!({ "format": format, "target": out.display().to_string(), "overwrite": overwrite }),
                )
                .map_err(conversion)?
            }
            Kind::Pdf => extract_pdf_text(input, out, options.pages.as_deref())?,
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
        fs::create_dir_all(out_dir)
            .map_err(|error| {
                OfficeError::runtime(format!("cannot create {}: {error}", out_dir.display())).with_code("output-unwritable")
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
                        "code": error.code(),
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
        return Err(OfficeError::runtime(format!("{} does not exist", path.display())).with_code("not-found"));
    }
    let bytes = fs::read(path)
        .map_err(|error| OfficeError::runtime(format!("cannot read {}: {error}", path.display())).with_code("unreadable"))?;
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
    ))
    .with_code("container-mismatch"))
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
fn extract_pdf_text(input: &Path, out: &Path, pages: Option<&str>) -> Result<Value, OfficeError> {
    let (report, selection) = match pages {
        Some(ranges) => {
            let (report, selected) = extract_pdf_pages(input, ranges)?;
            (report, Some((ranges, selected)))
        }
        None => (open_pdf_core::extract_text(input).map_err(conversion)?, None),
    };
    fs::write(out, report.markdown.as_bytes())
        .map_err(|error| OfficeError::runtime(format!("cannot write {}: {error}", out.display())).with_code("output-unwritable"))?;
    let mut summary = json!({
        "bytes": report.markdown.len(),
        "losses": report.losses,
    });
    if let Some((requested, selected)) = selection {
        summary["pages"] = json!({
            "requested": requested,
            "extracted": selected.len(),
            "numbers": selected,
        });
    }
    Ok(summary)
}

/// Extracts only the selected pages by arranging the source into a temporary PDF first.
fn extract_pdf_pages(input: &Path, ranges: &str) -> Result<(open_pdf_core::ExtractReport, Vec<usize>), OfficeError> {
    let info = open_pdf_core::inspect(input).map_err(conversion)?;
    let selected = parse_page_ranges(ranges, info.pages)?;
    if selected.len() == info.pages && selected.iter().copied().eq(1..=info.pages) {
        return Ok((open_pdf_core::extract_text(input).map_err(conversion)?, selected));
    }
    let remove = (1..=info.pages).filter(|page| !selected.contains(page)).collect::<Vec<_>>();
    let temporary = temporary_pdf_path();
    let request = open_pdf_core::DocumentArrange { order: Some(selected.clone()), rotate: None, remove: Some(remove) };
    open_pdf_core::arrange(input, &temporary, &request).map_err(conversion)?;
    let extracted = open_pdf_core::extract_text(&temporary).map_err(conversion);
    let _ = fs::remove_file(&temporary);
    Ok((extracted?, selected))
}

/// Expands one-based ranges such as `1-3,5`, refusing pages the PDF cannot satisfy.
fn parse_page_ranges(ranges: &str, page_count: usize) -> Result<Vec<usize>, OfficeError> {
    let mut selected = Vec::new();
    for raw in ranges.split(',') {
        let token = raw.trim();
        if token.is_empty() {
            return Err(invalid_pages("page ranges contain an empty entry"));
        }
        let (start, end) = match token.split_once('-') {
            Some((start, end)) => (start.trim(), end.trim()),
            None => (token, token),
        };
        let start = start.parse::<usize>().map_err(|_| invalid_pages(format!("invalid page range `{token}`")))?;
        let end = end.parse::<usize>().map_err(|_| invalid_pages(format!("invalid page range `{token}`")))?;
        if start == 0 || end == 0 {
            return Err(invalid_pages("page numbers start at 1"));
        }
        if end < start {
            return Err(invalid_pages(format!("page range `{token}` is reversed")));
        }
        if end > page_count {
            return Err(invalid_pages(format!("page range `{token}` exceeds the {page_count} available pages")));
        }
        for page in start..=end {
            if selected.contains(&page) {
                return Err(invalid_pages(format!("page {page} appears twice in `{ranges}`")));
            }
            selected.push(page);
        }
    }
    if selected.is_empty() {
        return Err(invalid_pages("at least one page range is required"));
    }
    Ok(selected)
}

/// A unique path for the intermediate PDF that carries only the selected pages.
fn temporary_pdf_path() -> PathBuf {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_nanos()).unwrap_or_default();
    std::env::temp_dir().join(format!("open-office-pages-{}-{sequence}-{nanos}.pdf", std::process::id()))
}

fn invalid_pages(message: impl Into<String>) -> OfficeError {
    OfficeError::usage(message).with_code("invalid-pages")
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

/// A failure raised by an engine while converting, with its stable batch code.
fn conversion(error: impl fmt::Display) -> OfficeError {
    OfficeError::runtime(error.to_string()).with_code("conversion-failed")
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

        let report = Office::new().outline(&path, false).expect("outline");
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
        let report = office.outline(&path, false).expect("outline");
        assert_eq!(report["kind"], json!("excel"));
        assert_eq!(report["outline"]["sheets"][0]["name"], json!("Sheet1"));

        let error = office.outline(&path, true).expect_err("points are presentations only");
        assert_eq!(error.code(), "unsupported-option");
        assert!(error.is_usage());
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

        let report = Office::new().outline(&marked, false).expect("outline");
        assert_eq!(report["kind"], json!("pdf"));
        assert_eq!(report["outline"]["pages"], json!(2));
        assert_eq!(report["outline"]["bookmarks"][0]["title"], json!("Top"));
        assert_eq!(report["outline"]["bookmarks"][0]["depth"], json!(1));
        assert_eq!(report["outline"]["bookmarks"][1]["depth"], json!(2));
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

        let report = Office::new().outline(&path, true).expect("outline");
        assert_eq!(report["kind"], json!("presentation"));
        assert_eq!(report["outline"]["slides"][0]["title"], json!("Intro"));
        assert_eq!(report["outline"]["slides"][1]["title"], json!("Plan"));
        assert_eq!(report["outline"]["slides"][0]["points"], json!([]));
        let _ = fs::remove_dir_all(&dir);
    }
}
