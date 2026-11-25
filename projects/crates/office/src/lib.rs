//! Library API of `open-office`.
//!
//! The Office crate does not grow a fifth document model. It detects the file
//! family, opens it through that engine's own public API, and dispatches
//! inspection and conversion. Every report comes from the engine that did the
//! work, so capability claims stay identical to the individual products.
//!
//! The command line in `main.rs` is a thin parser over [`Office`].

use std::{
    collections::{HashMap, HashSet},
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
///
/// A failure raised inside one of the four engines is forwarded with the
/// engine's own stable code and typed parameters, so a routed command reports
/// the same code the individual product would. Suite-level guardrails keep
/// their own codes such as `not-found` or `unsupported-target`.
#[derive(Debug)]
pub struct OfficeError {
    message: String,
    usage: bool,
    code: String,
    params: Option<Value>,
}

impl OfficeError {
    /// A caller mistake: unknown option, missing argument or unsupported target.
    pub fn usage(message: impl Into<String>) -> Self {
        Self { message: message.into(), usage: true, code: "usage".to_owned(), params: None }
    }

    /// A failure while reading, converting or writing a document.
    pub fn runtime(message: impl Into<String>) -> Self {
        Self { message: message.into(), usage: false, code: "runtime".to_owned(), params: None }
    }

    /// Refines the failure with a stable machine-readable code.
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = code.into();
        self
    }

    /// Attaches the typed parameters that belong to the stable code.
    pub fn with_params(mut self, params: Value) -> Self {
        self.params = (!params.is_null()).then_some(params);
        self
    }

    /// Human-readable detail, without the `open-office:` prefix.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Stable machine-readable code, safe for callers to branch on.
    ///
    /// Unrefined failures report `usage` or `runtime`; the concrete cause is
    /// named by sites such as `not-found`, `unsupported-target` or the engine
    /// codes (`io/read`, `session/argument-invalid`, ...) that forwarded
    /// engine failures carry unchanged.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Typed per-code parameters, or `None` when the code carries none.
    pub fn params(&self) -> Option<&Value> {
        self.params.as_ref()
    }

    /// Whether the failure is a usage error (exit code 2) or a runtime error (1).
    pub fn is_usage(&self) -> bool {
        self.usage
    }
}

/// The stable machine-readable identity an engine failure carries into suite
/// reports, so routing never hides the owning engine's code or parameters.
trait EngineFailure {
    fn stable_code(&self) -> &str;
    fn stable_message(&self) -> String;
    fn stable_params(&self) -> Option<Value>;
}

impl EngineFailure for open_word_document::ServiceError {
    fn stable_code(&self) -> &str {
        self.code()
    }

    fn stable_message(&self) -> String {
        self.message().to_owned()
    }

    /// The word engine stores the shared `document_ir` frozen payload, so its
    /// typed parameters serialize directly instead of being derived from text.
    fn stable_params(&self) -> Option<Value> {
        frozen_params(&self.params().to_json())
    }
}

impl EngineFailure for open_excel_workbook::ServiceError {
    fn stable_code(&self) -> &str {
        self.code()
    }

    fn stable_message(&self) -> String {
        self.message().to_owned()
    }

    /// The workbook engine stores the shared formula frozen payload, so its
    /// typed parameters serialize directly instead of being derived from text.
    fn stable_params(&self) -> Option<Value> {
        frozen_params(&self.params().to_json())
    }
}

impl EngineFailure for open_ppt_deck::ServiceError {
    fn stable_code(&self) -> &str {
        self.code()
    }

    fn stable_message(&self) -> String {
        self.message().to_owned()
    }

    fn stable_params(&self) -> Option<Value> {
        frozen_params(self.params())
    }
}

impl EngineFailure for open_pdf_core::ServiceError {
    fn stable_code(&self) -> &str {
        self.code()
    }

    fn stable_message(&self) -> String {
        self.message().to_owned()
    }

    /// The PDF product surface now carries typed parameters beside the code,
    /// so routing forwards their frozen object unchanged.
    fn stable_params(&self) -> Option<Value> {
        frozen_params(self.params())
    }
}

/// Accepts only the frozen object payload an engine attaches to its code.
///
/// Every engine publishes parameters as a JSON object whose keys are fixed per
/// code; `null`, arrays and scalars are not a frozen payload, so the suite drops
/// them instead of forwarding a half-shaped value into a report.
fn frozen_params(params: &Value) -> Option<Value> {
    params.as_object().map(|_| params.clone())
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

/// Options that narrow one `render` request.
///
/// Defaults render every page the owning engine produces as SVG.
#[derive(Debug, Clone, Default)]
pub struct RenderOptions {
    /// Workbook sheet name or id; the primary sheet is used when omitted.
    pub sheet: Option<String>,
    /// One-based presentation page; renders every page when omitted.
    pub page: Option<u32>,
    /// One-based presentation pages such as `1,3-5`; renders every page when omitted.
    pub pages: Option<String>,
    /// Writes the selected SVG here instead of only reporting it.
    pub out: Option<PathBuf>,
    /// Writes every selected page as `page-<n>.svg` into this directory.
    pub out_dir: Option<PathBuf>,
    /// Replaces existing `out` or `out_dir` files.
    pub overwrite: bool,
}

/// Options that narrow one `replace` request.
///
/// Defaults replace case-sensitively and apply every match the document holds.
#[derive(Debug, Clone, Default)]
pub struct ReplaceOptions {
    /// Matches `find` regardless of case; the suite matches case by default.
    pub ignore_case: bool,
    /// Caps the replacements: per document for Word, per sheet for workbooks.
    pub limit: Option<u32>,
    /// Counts matches without writing the changed document back to disk.
    pub dry_run: bool,
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
            "commands": ["probe", "inspect", "validate", "extract", "convert", "open", "formats", "stats", "totals", "metadata", "outline", "compare", "search", "replace", "render", "capabilities", "version"],
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

    /// Opens one document in the platform's default viewer.
    ///
    /// The file is routed by extension first, so the report always names the
    /// family that owns it. `print_only` returns the same report without
    /// launching anything, which keeps route checks usable on headless hosts.
    /// The suite ships no viewer of its own: on Windows it starts the file with
    /// `cmd /C start`, on macOS it uses `open` and elsewhere `xdg-open`.
    pub fn open(&self, path: &Path, print_only: bool) -> Result<Value, OfficeError> {
        let kind = Kind::of_path(path)?;
        if !path.exists() {
            return Err(OfficeError::runtime(format!("{} does not exist", path.display())).with_code("not-found"));
        }
        let opener = default_opener();
        let mut report = json!({
            "kind": kind.label(),
            "path": path.display().to_string(),
            "opener": opener,
            "opened": false,
        });
        if print_only {
            return Ok(report);
        }
        let status = if cfg!(windows) {
            std::process::Command::new("cmd")
                .arg("/C")
                .arg("start")
                .arg("")
                .arg(path)
                .status()
        }
        else {
            std::process::Command::new(opener).arg(path).status()
        }
        .map_err(|error| {
            OfficeError::runtime(format!("cannot start {opener} for {}: {error}", path.display())).with_code("open-failed")
        })?;
        if !status.success() {
            return Err(OfficeError::runtime(format!(
                "{opener} exited with {} for {}",
                status.code().map_or_else(|| "a signal".to_owned(), |code| code.to_string()),
                path.display()
            ))
            .with_code("open-failed"));
        }
        report["opened"] = Value::Bool(true);
        Ok(report)
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

    /// Probes many files in input order, using bytes first and extensions as hints.
    ///
    /// A failing file does not stop the batch: each entry carries `ok` and
    /// either the detection fields or the stable `code` and `error` message.
    /// `evidence` is present only when the detector quoted some.
    pub fn probe_many(&self, inputs: &[PathBuf]) -> Result<Value, OfficeError> {
        if inputs.is_empty() {
            return Err(OfficeError::usage("at least one document is required"));
        }
        let mut probed = 0;
        let mut failed = 0;
        let mut results = Vec::with_capacity(inputs.len());
        for input in inputs {
            match self.probe(input) {
                Ok(report) => {
                    probed += 1;
                    let mut entry = json!({
                        "source": input.display().to_string(),
                        "ok": true,
                        "format": report.get("format").cloned().unwrap_or(Value::Null),
                        "container": report.get("container").cloned().unwrap_or(Value::Null),
                        "confidence": report.get("confidence").cloned().unwrap_or(Value::Null),
                        "candidates": report.get("candidates").cloned().unwrap_or(Value::Null),
                    });
                    let evidence = report.get("evidence").cloned().unwrap_or(Value::Null);
                    if evidence.as_str().map(|text| !text.is_empty()).unwrap_or(false) {
                        entry["evidence"] = evidence;
                    }
                    results.push(entry);
                }
                Err(error) => {
                    failed += 1;
                    results.push(json!({
                        "source": input.display().to_string(),
                        "ok": false,
                        "code": error.code(),
                        "params": error.params(),
                        "error": error.message(),
                    }));
                }
            }
        }
        Ok(json!({ "probed": probed, "failed": failed, "results": results }))
    }

    /// Reports lightweight per-family statistics for one document.
    pub fn stats(&self, path: &Path) -> Result<Value, OfficeError> {
        let kind = Kind::of_path(path)?;
        verify_container(path, kind)?;
        let stats = match kind {
            Kind::Word => {
                let opened = open_word_document::open(path).map_err(engine)?;
                let handle = handle_of(&opened)?;
                let outcome = open_word_document::stats(handle);
                let _ = open_word_document::close(handle);
                outcome.map_err(engine)?
            }
            Kind::Excel => {
                let opened = open_excel_workbook::open(path).map_err(engine)?;
                let handle = handle_of(&opened)?;
                let outcome = open_excel_workbook::summary(handle);
                let _ = open_excel_workbook::close(handle);
                outcome.map_err(engine)?
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
                let report = open_pdf_core::inspect(path).map_err(pdf_engine)?;
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

    /// Reports the descriptive metadata each family actually stores.
    ///
    /// Word, workbook and presentation engines keep only a document title, so
    /// they answer with `{ title }`; a PDF answers with its whole information
    /// dictionary (`title`, `author`, `subject`, `keywords`, `creator`,
    /// `producer`). Nothing is derived or defaulted, so a missing field is
    /// `null` instead of an invented value.
    pub fn metadata(&self, path: &Path) -> Result<Value, OfficeError> {
        let kind = Kind::of_path(path)?;
        verify_container(path, kind)?;
        let metadata = match kind {
            Kind::Word => {
                let opened = open_word_document::open(path).map_err(engine)?;
                let handle = handle_of(&opened)?;
                let outcome = open_word_document::summary(handle);
                let _ = open_word_document::close(handle);
                let summary = outcome.map_err(engine)?;
                json!({ "title": summary.get("title").cloned().unwrap_or(Value::Null) })
            }
            Kind::Excel => {
                let opened = open_excel_workbook::open(path).map_err(engine)?;
                let handle = handle_of(&opened)?;
                let outcome = open_excel_workbook::summary(handle);
                let _ = open_excel_workbook::close(handle);
                let summary = outcome.map_err(engine)?;
                json!({ "title": summary.get("title").cloned().unwrap_or(Value::Null) })
            }
            Kind::Presentation => {
                let loaded = load_deck(path)?;
                let summary = open_ppt_deck::summary(&loaded.deck);
                json!({ "title": summary.get("title").cloned().unwrap_or(Value::Null) })
            }
            Kind::Pdf => {
                let report = open_pdf_core::inspect(path).map_err(pdf_engine)?;
                serde_json::to_value(&report.metadata).map_err(|error| OfficeError::runtime(error.to_string()))?
            }
        };
        Ok(json!({
            "kind": kind.label(),
            "path": path.display().to_string(),
            "metadata": metadata,
        }))
    }

    /// Reports metadata for many documents on a best-effort basis.
    ///
    /// Every input is attempted in order; a failing document contributes only
    /// its stable `code` and `error` and does not stop the batch. Each success
    /// keeps the family `kind` and the same `metadata` object [`Office::metadata`]
    /// would return for that file.
    pub fn metadata_many(&self, paths: &[PathBuf]) -> Result<Value, OfficeError> {
        if paths.is_empty() {
            return Err(OfficeError::usage("at least one document is required"));
        }
        let mut counted = 0;
        let mut failed = 0;
        let mut results = Vec::with_capacity(paths.len());
        for path in paths {
            match self.metadata(path) {
                Ok(report) => {
                    counted += 1;
                    results.push(json!({
                        "source": path.display().to_string(),
                        "ok": true,
                        "kind": report.get("kind").cloned().unwrap_or(Value::Null),
                        "metadata": report.get("metadata").cloned().unwrap_or(Value::Null),
                    }));
                }
                Err(error) => {
                    failed += 1;
                    results.push(json!({
                        "source": path.display().to_string(),
                        "ok": false,
                        "code": error.code(),
                        "params": error.params(),
                        "error": error.message(),
                    }));
                }
            }
        }
        Ok(json!({ "counted": counted, "failed": failed, "results": results }))
    }

    /// Projects one document into its navigable outline.
    ///
    /// Each family answers with the structure it really owns: a word document
    /// nests its sections by heading level, a workbook lists its sheets, a
    /// presentation lists its slides (and, with `points`, their talking points),
    /// and a PDF reports its outline bookmarks. `points` is refused for families
    /// that have no talking points instead of being silently ignored. `depth`
    /// keeps only word headings and PDF bookmarks at that level or shallower;
    /// families without levels refuse the option.
    pub fn outline(&self, path: &Path, points: bool, depth: Option<u8>) -> Result<Value, OfficeError> {
        let kind = Kind::of_path(path)?;
        if points && kind != Kind::Presentation {
            return Err(OfficeError::usage("`--points` only applies to presentations").with_code("unsupported-option"));
        }
        if depth.is_some() && !matches!(kind, Kind::Word | Kind::Pdf) {
            return Err(OfficeError::usage("`--depth` only applies to word documents and PDFs").with_code("unsupported-option"));
        }
        if depth == Some(0) {
            return Err(OfficeError::usage("`--depth` needs a positive integer").with_code("invalid-depth"));
        }
        verify_container(path, kind)?;
        let outline = match kind {
            Kind::Word => {
                let opened = open_word_document::open(path).map_err(engine)?;
                let handle = handle_of(&opened)?;
                let outcome = open_word_document::outline(handle);
                let _ = open_word_document::close(handle);
                let mut outline = outcome.map_err(engine)?;
                if let Some(depth) = depth
                    && let Some(items) = outline.get_mut("items")
                {
                    trim_outline_items(items, depth);
                }
                outline
            }
            Kind::Excel => {
                let opened = open_excel_workbook::open(path).map_err(engine)?;
                let handle = handle_of(&opened)?;
                let summary = open_excel_workbook::summary(handle);
                let _ = open_excel_workbook::close(handle);
                let summary = summary.map_err(engine)?;
                json!({
                    "revision": summary.get("revision").cloned().unwrap_or(Value::Null),
                    "sheets": summary.get("sheets").cloned().unwrap_or_else(|| json!([])),
                })
            }
            Kind::Presentation => {
                let loaded = load_deck(path)?;
                open_ppt_deck::outline::outline(&loaded.deck, &json!({ "points": points })).map_err(engine)?
            }
            Kind::Pdf => {
                let report = open_pdf_core::inspect(path).map_err(pdf_engine)?;
                let mut bookmarks = serde_json::to_value(&report.bookmarks)
                    .map_err(|error| OfficeError::runtime(error.to_string()))?;
                if let Some(depth) = depth
                    && let Some(entries) = bookmarks.as_array_mut()
                {
                    entries.retain(|entry| entry.get("depth").and_then(Value::as_u64).is_some_and(|value| value <= depth as u64));
                }
                json!({ "pages": report.pages, "bookmarks": bookmarks })
            }
        };
        Ok(json!({
            "kind": kind.label(),
            "path": path.display().to_string(),
            "outline": outline,
        }))
    }

    /// Projects many documents into their outlines in input order.
    ///
    /// A failing document does not stop the batch: each entry carries `ok` and
    /// either the family `kind`/`outline` or the stable `code` and `error`.
    /// `points` applies to presentations only and fails other families the same
    /// way single-document [`Office::outline`] does.
    pub fn outline_many(&self, inputs: &[PathBuf], points: bool, depth: Option<u8>) -> Result<Value, OfficeError> {
        if inputs.is_empty() {
            return Err(OfficeError::usage("at least one document is required"));
        }
        let mut outlined = 0;
        let mut failed = 0;
        let mut results = Vec::with_capacity(inputs.len());
        for input in inputs {
            match self.outline(input, points, depth) {
                Ok(report) => {
                    outlined += 1;
                    results.push(json!({
                        "source": input.display().to_string(),
                        "ok": true,
                        "kind": report.get("kind").cloned().unwrap_or(Value::Null),
                        "outline": report.get("outline").cloned().unwrap_or(Value::Null),
                    }));
                }
                Err(error) => {
                    failed += 1;
                    results.push(json!({
                        "source": input.display().to_string(),
                        "ok": false,
                        "code": error.code(),
                        "params": error.params(),
                        "error": error.message(),
                    }));
                }
            }
        }
        Ok(json!({ "outlined": outlined, "failed": failed, "results": results }))
    }

    /// Diffs the outlines of two documents of the same family.
    ///
    /// Sections come from the owning engine's own outline projection, in
    /// document order: word heading titles (nested children flattened),
    /// workbook sheet names, slide titles and PDF bookmark titles. The report
    /// aligns the two lists with a longest-common-subsequence walk: titles only
    /// in the right document are `added`, titles only in the left are
    /// `removed`, and a removed title paired with an added title inside the
    /// same change block is reported as `renamed`. Two documents of different
    /// families fail with `kind-mismatch`; anything the single-document
    /// [`Office::outline`] rejects (missing file, wrong container) applies here
    /// too.
    pub fn compare(&self, left: &Path, right: &Path) -> Result<Value, OfficeError> {
        self.compare_with(left, right, false)
    }

    /// Compares two same-family outlines, optionally folding case in titles.
    ///
    /// With `ignore_case`, `"Intro"` and `"INTRO"` are the same section, so a
    /// pure case change is not reported as a rename. Added and removed titles
    /// keep their original spelling from the side they came from.
    pub fn compare_with(&self, left: &Path, right: &Path, ignore_case: bool) -> Result<Value, OfficeError> {
        self.compare_at(left, right, ignore_case, None, false, false)
    }

    /// Compares two same-family outlines at an optional heading depth.
    ///
    /// `depth` keeps only word headings and PDF bookmarks at that level or
    /// shallower on both sides before diffing, so a deeper change is invisible
    /// to a shallow comparison. Workbooks and presentations reject a depth the
    /// same way [`Office::outline`] does. With `points`, presentation slides are
    /// expanded into one entry per talking point so body changes show up as
    /// added, removed or renamed entries; other families reject the option.
    /// With `notes`, presentation slides are expanded into one entry per
    /// speaker-note line the same way; `points` and `notes` are mutually
    /// exclusive.
    pub fn compare_at(
        &self,
        left: &Path,
        right: &Path,
        ignore_case: bool,
        depth: Option<u8>,
        points: bool,
        notes: bool,
    ) -> Result<Value, OfficeError> {
        let left_kind = Kind::of_path(left)?;
        let right_kind = Kind::of_path(right)?;
        if left_kind != right_kind {
            return Err(OfficeError::usage(format!(
                "`compare` needs two documents of the same family; the left is {}, the right is {}",
                left_kind.label(),
                right_kind.label()
            ))
            .with_code("kind-mismatch"));
        }
        if points && notes {
            return Err(OfficeError::usage("`--points` and `--notes` are mutually exclusive").with_code("unsupported-option"));
        }
        if points && left_kind != Kind::Presentation {
            return Err(OfficeError::usage("`--points` only applies to presentations").with_code("unsupported-option"));
        }
        if notes && left_kind != Kind::Presentation {
            return Err(OfficeError::usage("`--notes` only applies to presentations").with_code("unsupported-option"));
        }
        let left_sections = self.outline_sections(left, depth, points, notes)?;
        let right_sections = self.outline_sections(right, depth, points, notes)?;
        let (added, removed, renamed, moved) = diff_sections(&left_sections, &right_sections, ignore_case);
        Ok(json!({
            "kind": left_kind.label(),
            "left": { "path": left.display().to_string(), "sections": left_sections.len() },
            "right": { "path": right.display().to_string(), "sections": right_sections.len() },
            "ignoreCase": ignore_case,
            "depth": depth,
            "points": points,
            "notes": notes,
            "identical": added.is_empty() && removed.is_empty() && renamed.is_empty() && moved.is_empty(),
            "added": added,
            "removed": removed,
            "renamed": renamed,
            "moved": moved,
        }))
    }

    /// Collects the outline titles of one document, in document order.
    ///
    /// With `points`, a presentation contributes one `Title · point` entry per
    /// slide point (or the title alone when the slide has no points), so point
    /// changes participate in the diff. With `notes`, each slide contributes
    /// one `Title · line` entry per non-empty speaker-note line (or the title
    /// alone when the slide has no notes), and the two modes never mix.
    fn outline_sections(
        &self,
        path: &Path,
        depth: Option<u8>,
        points: bool,
        notes: bool,
    ) -> Result<Vec<String>, OfficeError> {
        let report = self.outline(path, points, depth)?;
        if points {
            let mut entries = Vec::new();
            if let Some(slides) = report.pointer("/outline/slides").and_then(Value::as_array) {
                for slide in slides {
                    let title = slide.get("title").and_then(Value::as_str).unwrap_or("");
                    let slide_points = slide.get("points").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
                    if slide_points.is_empty() {
                        entries.push(title.to_owned());
                        continue;
                    }
                    for point in slide_points {
                        if let Some(point) = point.as_str() {
                            entries.push(if title.is_empty() {
                                point.to_owned()
                            }
                            else {
                                format!("{title} · {point}")
                            });
                        }
                    }
                }
            }
            if !entries.is_empty() {
                return Ok(entries);
            }
        }
        if notes {
            let mut entries = Vec::new();
            if let Some(slides) = report.pointer("/outline/slides").and_then(Value::as_array) {
                for slide in slides {
                    let title = slide.get("title").and_then(Value::as_str).unwrap_or("");
                    let lines: Vec<&str> = slide
                        .get("notes")
                        .and_then(Value::as_str)
                        .map(|notes| notes.lines().map(str::trim).filter(|line| !line.is_empty()).collect())
                        .unwrap_or_default();
                    if lines.is_empty() {
                        entries.push(title.to_owned());
                        continue;
                    }
                    for line in lines {
                        entries.push(if title.is_empty() {
                            line.to_owned()
                        }
                        else {
                            format!("{title} · {line}")
                        });
                    }
                }
            }
            if !entries.is_empty() {
                return Ok(entries);
            }
        }
        let mut titles = Vec::new();
        collect_titles(&report["outline"], &mut titles);
        Ok(titles)
    }

    /// Searches the text one document owns and reports hits with stable locations.
    ///
    /// Matching is case-sensitive for every family so one query behaves the same
    /// way across Word, Excel, Presentation and PDF. Word and Excel reuse the
    /// engine's own `find`; a presentation walks slide text, table cells and
    /// speaker notes; a PDF scans the extracted Markdown line by line because
    /// page numbers are not tracked per hit.
    pub fn search(&self, path: &Path, query: &str, limit: Option<u32>) -> Result<Value, OfficeError> {
        if query.is_empty() {
            return Err(OfficeError::usage("`--text` needs a non-empty query").with_code("invalid-query"));
        }
        let kind = Kind::of_path(path)?;
        verify_container(path, kind)?;
        let limit = limit.unwrap_or(100).clamp(1, 500) as usize;
        let (hits, truncated, notes) = match kind {
            Kind::Word => {
                let opened = open_word_document::open(path).map_err(engine)?;
                let handle = handle_of(&opened)?;
                let outcome = open_word_document::find(handle, query, Some(limit as u32));
                let _ = open_word_document::close(handle);
                let report = outcome.map_err(engine)?;
                let hits = report.as_array().cloned().unwrap_or_default();
                let truncated = hits.len() >= limit;
                (hits, truncated, Vec::new())
            }
            Kind::Excel => {
                let opened = open_excel_workbook::open(path).map_err(engine)?;
                let handle = handle_of(&opened)?;
                let outcome = open_excel_workbook::find(handle, &json!({ "query": query, "options": { "matchCase": true } }));
                let _ = open_excel_workbook::close(handle);
                let report = outcome.map_err(engine)?;
                let mut hits = report.get("hits").and_then(Value::as_array).cloned().unwrap_or_default();
                let capped = report.get("truncated").and_then(Value::as_bool).unwrap_or(false);
                let truncated = capped || hits.len() > limit;
                hits.truncate(limit);
                (hits, truncated, Vec::new())
            }
            Kind::Presentation => presentation_search(path, query, limit)?,
            Kind::Pdf => pdf_search(path, query, limit)?,
        };
        Ok(json!({
            "kind": kind.label(),
            "path": path.display().to_string(),
            "query": query,
            "limit": limit,
            "hits": hits,
            "truncated": truncated,
            "notes": notes,
        }))
    }

    /// Searches many documents for the same query in input order.
    ///
    /// A failing document does not stop the batch: each entry carries `ok` and
    /// either the family `kind`/`search` report or the stable `code` and `error`
    /// message. `limit` applies to each document, not to the batch.
    pub fn search_many(&self, inputs: &[PathBuf], query: &str, limit: Option<u32>) -> Result<Value, OfficeError> {
        if inputs.is_empty() {
            return Err(OfficeError::usage("at least one document is required"));
        }
        if query.is_empty() {
            return Err(OfficeError::usage("`--text` needs a non-empty query").with_code("invalid-query"));
        }
        let mut searched = 0;
        let mut failed = 0;
        let mut results = Vec::with_capacity(inputs.len());
        for input in inputs {
            match self.search(input, query, limit) {
                Ok(report) => {
                    searched += 1;
                    results.push(json!({
                        "source": input.display().to_string(),
                        "ok": true,
                        "kind": report.get("kind").cloned().unwrap_or(Value::Null),
                        "search": report,
                    }));
                }
                Err(error) => {
                    failed += 1;
                    results.push(json!({
                        "source": input.display().to_string(),
                        "ok": false,
                        "code": error.code(),
                        "params": error.params(),
                        "error": error.message(),
                    }));
                }
            }
        }
        Ok(json!({ "query": query, "searched": searched, "failed": failed, "results": results }))
    }

    /// Replaces one query with new text inside one document, saved in place.
    ///
    /// The request reaches the owning engine's own find-and-replace, so Word
    /// covers section titles, paragraphs, list items and table cells, workbooks
    /// cover the text cells of every sheet that holds content, and presentations
    /// cover slide text, table cells and speaker notes. Matching is
    /// case-sensitive unless `ignore_case` is set, mirroring the suite's
    /// case-sensitive `search`. A document with no match is reported with
    /// `replaced: 0` and its file is left untouched; only a document that
    /// changed is written back in its own format. With `dry_run` the engine
    /// still counts matches, but `saved` stays null and the file on disk keeps
    /// its bytes. PDF has no replacement route in this build.
    ///
    /// `limit` caps per document for word files and per sheet for workbooks;
    /// the presentation engine replaces every match, so a limited request on a
    /// deck is refused with `unsupported-option` instead of being ignored. The
    /// write-back goes to the same path the caller passed, so the caller must
    /// own that file; exchange formats can drop engine-specific content and
    /// each loss is reported under `saved.report`. Structured or scoped
    /// replacement belongs to the owning product's SDK, not to this command.
    pub fn replace(&self, path: &Path, find: &str, with: &str, options: &ReplaceOptions) -> Result<Value, OfficeError> {
        if find.is_empty() {
            return Err(OfficeError::usage("`--find` needs a non-empty query").with_code("invalid-query"));
        }
        let kind = Kind::of_path(path)?;
        verify_container(path, kind)?;
        let extension = path.extension().and_then(|value| value.to_str()).unwrap_or_default().to_ascii_lowercase();
        let match_case = !options.ignore_case;
        let (replaced, revision, saved) = match kind {
            Kind::Word => {
                let opened = open_word_document::open(path).map_err(engine)?;
                let handle = handle_of(&opened)?;
                let _session = WordSession(handle);
                let mut request = json!({ "find": find, "replace": with, "caseSensitive": match_case });
                if let Some(limit) = options.limit {
                    request["limit"] = json!(limit);
                }
                let outcome = open_word_document::replace(handle, &request).map_err(engine)?;
                let replaced = outcome.get("replaced").and_then(Value::as_u64).unwrap_or_default();
                let revision = outcome.get("revision").cloned().unwrap_or(Value::Null);
                let saved = if replaced == 0 || options.dry_run {
                    Value::Null
                }
                else {
                    let (format, report) = if extension == "oword" {
                        ("oword".to_owned(), open_word_document::save(handle, path, true).map_err(engine)?)
                    }
                    else {
                        let format = word_persist_format(&extension).ok_or_else(|| unsupported_extension_route(&extension))?;
                        (format.to_owned(), open_word_document::export(handle, format, path, true).map_err(engine)?)
                    };
                    json!({ "format": format, "report": report })
                };
                (replaced, revision, saved)
            }
            Kind::Excel => {
                let opened = open_excel_workbook::open(path).map_err(engine)?;
                let handle = handle_of(&opened)?;
                let _session = ExcelSession(handle);
                let summary = open_excel_workbook::summary(handle).map_err(engine)?;
                let revision = summary.get("revision").cloned().unwrap_or(Value::Null);
                let mut operations = Vec::new();
                for sheet in summary.get("sheets").and_then(Value::as_array).cloned().unwrap_or_default() {
                    if sheet.get("cells").and_then(Value::as_u64).unwrap_or_default() == 0 {
                        continue;
                    }
                    let mut operation = json!({
                        "op": "cell.replaceText",
                        "sheet": sheet.get("name").cloned().unwrap_or(Value::Null),
                        "find": find,
                        "replace": with,
                        "matchCase": match_case,
                    });
                    if let Some(limit) = options.limit {
                        operation["limit"] = json!(limit);
                    }
                    operations.push(operation);
                }
                let (replaced, revision, saved) = if operations.is_empty() {
                    (0, revision, Value::Null)
                }
                else {
                    let outcome =
                        open_excel_workbook::edit(handle, &json!({ "operations": operations })).map_err(engine)?;
                    let replaced = outcome.get("replacements").and_then(Value::as_u64).unwrap_or_default();
                    let revision = outcome.get("revision").cloned().unwrap_or(revision);
                    let saved = if replaced == 0 || options.dry_run {
                        Value::Null
                    }
                    else {
                        let (format, report) = if extension == "oexcel" {
                            ("oexcel".to_owned(), open_excel_workbook::save(handle, path, true).map_err(engine)?)
                        }
                        else {
                            let format =
                                excel_persist_format(&extension).ok_or_else(|| unsupported_extension_route(&extension))?;
                            (format.to_owned(), open_excel_workbook::export(handle, format, path, true).map_err(engine)?)
                        };
                        json!({ "format": format, "report": report })
                    };
                    (replaced, revision, saved)
                };
                (replaced, revision, saved)
            }
            Kind::Presentation => {
                if options.limit.is_some() {
                    return Err(OfficeError::usage(
                        "`--limit` applies to word documents and workbooks; the presentation engine replaces every match",
                    )
                    .with_code("unsupported-option"));
                }
                let mut loaded = load_deck(path)?;
                let outcome = open_ppt_deck::edit::edit(
                    &mut loaded.deck,
                    &json!({ "operations": [{
                        "op": "deck.replaceText",
                        "find": find,
                        "replace": with,
                        "caseSensitive": match_case,
                    }] }),
                )
                .map_err(engine)?;
                let replaced = outcome
                    .get("results")
                    .and_then(Value::as_array)
                    .and_then(|results| results.first())
                    .and_then(|value| value.as_u64().or_else(|| value.as_str().and_then(|text| text.parse().ok())))
                    .unwrap_or_default();
                let revision = outcome.get("revision").cloned().unwrap_or(Value::Null);
                let saved = if replaced == 0 || options.dry_run {
                    Value::Null
                }
                else if matches!(extension.as_str(), "project" | "oppt" | "opresent") {
                    let bytes = open_ppt_deck::save(&loaded.deck, path, true).map_err(engine)?;
                    json!({ "format": extension, "report": { "bytes": bytes } })
                }
                else {
                    let format =
                        presentation_persist_format(&extension).ok_or_else(|| unsupported_extension_route(&extension))?;
                    let report = open_ppt_deck::export(
                        &loaded.deck,
                        &json!({ "format": format, "target": path.display().to_string(), "overwrite": true }),
                    )
                    .map_err(engine)?;
                    json!({ "format": format, "report": report })
                };
                (replaced, revision, saved)
            }
            Kind::Pdf => {
                return Err(OfficeError::usage(format!(
                    "`replace` supports word documents, workbooks and presentations; {} has no find-and-replace route in this build",
                    kind.label()
                ))
                .with_code("unsupported-target"));
            }
        };
        Ok(json!({
            "kind": kind.label(),
            "path": path.display().to_string(),
            "find": find,
            "replace": with,
            "matchCase": match_case,
            "dryRun": options.dry_run,
            "replaced": replaced,
            "revision": revision,
            "saved": saved,
        }))
    }

    /// Replaces one query across many documents in input order.
    ///
    /// A failing document does not stop the batch: each entry carries `ok` and
    /// either the family `kind` with its `replaced` count and `revision` or the
    /// stable `code` and `error` message. Options apply to every document, and
    /// each document is saved in place exactly as single-document
    /// [`Office::replace`] does.
    pub fn replace_many(
        &self,
        inputs: &[PathBuf],
        find: &str,
        with: &str,
        options: &ReplaceOptions,
    ) -> Result<Value, OfficeError> {
        if inputs.is_empty() {
            return Err(OfficeError::usage("at least one document is required"));
        }
        if find.is_empty() {
            return Err(OfficeError::usage("`--find` needs a non-empty query").with_code("invalid-query"));
        }
        let mut updated = 0;
        let mut failed = 0;
        let mut results = Vec::with_capacity(inputs.len());
        for input in inputs {
            match self.replace(input, find, with, options) {
                Ok(report) => {
                    updated += 1;
                    results.push(json!({
                        "source": input.display().to_string(),
                        "ok": true,
                        "kind": report.get("kind").cloned().unwrap_or(Value::Null),
                        "replaced": report.get("replaced").cloned().unwrap_or(Value::Null),
                        "revision": report.get("revision").cloned().unwrap_or(Value::Null),
                    }));
                }
                Err(error) => {
                    failed += 1;
                    results.push(json!({
                        "source": input.display().to_string(),
                        "ok": false,
                        "code": error.code(),
                        "params": error.params(),
                        "error": error.message(),
                    }));
                }
            }
        }
        Ok(json!({
            "find": find,
            "replace": with,
            "dryRun": options.dry_run,
            "updated": updated,
            "failed": failed,
            "results": results,
        }))
    }

    /// Renders a workbook sheet or presentation slides to SVG.
    ///
    /// The page list always comes from the owning engine: a workbook yields one
    /// page for the selected sheet, a presentation yields one page per slide.
    /// `page` selects a one-based presentation page and is refused for
    /// workbooks; `out` writes the single selected SVG, `out_dir` writes every
    /// selected page as `page-<n>.svg`, and both need `overwrite` when a target
    /// already exists. Word and PDF files are refused because neither engine
    /// exposes an SVG renderer in this build.
    pub fn render(&self, path: &Path, options: &RenderOptions) -> Result<Value, OfficeError> {
        let kind = Kind::of_path(path)?;
        verify_container(path, kind)?;
        let (revision, pages, losses, total) = match kind {
            Kind::Excel => {
                if options.page.is_some() || options.pages.is_some() {
                    return Err(OfficeError::usage("`--page` and `--pages` only apply to presentations")
                        .with_code("unsupported-option"));
                }
                let opened = open_excel_workbook::open(path).map_err(engine)?;
                let handle = handle_of(&opened)?;
                let request = json!({ "format": "svg", "sheet": options.sheet.clone().unwrap_or_default() });
                let outcome = open_excel_workbook::render(handle, &request);
                let _ = open_excel_workbook::close(handle);
                let report = outcome.map_err(engine)?;
                let page = json!({
                    "index": 0,
                    "sheet": report.get("sheet").cloned().unwrap_or(Value::Null),
                    "sheetId": report.get("sheetId").cloned().unwrap_or(Value::Null),
                    "svg": report.get("svg").cloned().unwrap_or(Value::Null),
                });
                (report.get("revision").cloned().unwrap_or(Value::Null), vec![page], json!([]), 1)
            }
            Kind::Presentation => {
                if options.sheet.is_some() {
                    return Err(OfficeError::usage("`--sheet` only applies to workbooks").with_code("unsupported-option"));
                }
                if options.page.is_some() && options.pages.is_some() {
                    return Err(OfficeError::usage("`--page` and `--pages` are mutually exclusive")
                        .with_code("unsupported-option"));
                }
                let loaded = load_deck(path)?;
                let report = open_ppt_deck::render(&loaded.deck).map_err(engine)?;
                let all = report.get("pages").and_then(Value::as_array).cloned().unwrap_or_default();
                let total = all.len();
                let pages = match (&options.page, options.pages.as_deref()) {
                    (Some(number), _) => {
                        let index = number.checked_sub(1).ok_or_else(|| {
                            OfficeError::usage("`--page` starts at 1").with_code("page-out-of-range")
                        })? as usize;
                        let page = all.get(index).cloned().ok_or_else(|| {
                            OfficeError::usage(format!("page {number} is outside this {total}-page presentation"))
                                .with_code("page-out-of-range")
                        })?;
                        vec![page]
                    }
                    (None, Some(ranges)) => parse_page_ranges(ranges, total)?
                        .into_iter()
                        .map(|page| all[page - 1].clone())
                        .collect(),
                    (None, None) => all,
                };
                let losses = report.get("losses").cloned().unwrap_or_else(|| json!([]));
                (report.get("revision").cloned().unwrap_or(Value::Null), pages, losses, total)
            }
            Kind::Word | Kind::Pdf => {
                return Err(OfficeError::usage(format!(
                    "`render` supports workbooks and presentations; {} has no SVG renderer in this build",
                    kind.label()
                ))
                .with_code("unsupported-target"));
            }
        };
        if options.out.is_some() && options.out_dir.is_some() {
            return Err(OfficeError::usage("`--out` and `--out-dir` are mutually exclusive").with_code("unsupported-option"));
        }
        let mut written: Vec<String> = Vec::new();
        if let Some(out) = &options.out {
            if pages.len() != 1 {
                return Err(OfficeError::usage(
                    "`--out` needs exactly one page; pass `--page <n>` to choose one of the presentation pages",
                )
                .with_code("multiple-pages"));
            }
            write_svg(out, &pages[0], options.overwrite)?;
            written.push(out.display().to_string());
        }
        if let Some(dir) = &options.out_dir {
            fs::create_dir_all(dir).map_err(|error| {
                OfficeError::runtime(format!("cannot create {}: {error}", dir.display())).with_code("unwritable")
            })?;
            for (offset, page) in pages.iter().enumerate() {
                let number = page.get("index").and_then(Value::as_u64).unwrap_or(offset as u64) + 1;
                let out = dir.join(format!("page-{number}.svg"));
                write_svg(&out, page, options.overwrite)?;
                written.push(out.display().to_string());
            }
        }
        Ok(json!({
            "kind": kind.label(),
            "path": path.display().to_string(),
            "format": "svg",
            "revision": revision,
            "out": options.out.as_ref().map(|path| path.display().to_string()),
            "outDir": options.out_dir.as_ref().map(|path| path.display().to_string()),
            "written": written,
            "totalPages": total,
            "pages": pages,
            "losses": losses,
        }))
    }

    /// Renders many documents into one subdirectory per source file.
    ///
    /// Every input keeps its own `page-<n>.svg` files under
    /// `<out_dir>/<source stem>/`. A failing document does not stop the batch:
    /// each entry carries `ok` and either `written`/`totalPages` or the stable
    /// `code` and `error` message.
    pub fn render_many(&self, inputs: &[PathBuf], options: &RenderOptions) -> Result<Value, OfficeError> {
        if inputs.is_empty() {
            return Err(OfficeError::usage("at least one document is required"));
        }
        if options.out.is_some() {
            return Err(OfficeError::usage(
                "`--out` writes one page and cannot be combined with several documents; pass `--out-dir <dir>`",
            )
            .with_code("unsupported-option"));
        }
        let out_dir = options
            .out_dir
            .as_ref()
            .ok_or_else(|| OfficeError::usage("`--out-dir <dir>` is required for several documents"))?;
        fs::create_dir_all(out_dir).map_err(|error| {
            OfficeError::runtime(format!("cannot create {}: {error}", out_dir.display())).with_code("unwritable")
        })?;

        let mut rendered = 0;
        let mut failed = 0;
        let mut results = Vec::with_capacity(inputs.len());
        for input in inputs {
            let stem = input.file_stem().map(|value| value.to_string_lossy().into_owned()).unwrap_or_default();
            let target = out_dir.join(&stem);
            let per_file = RenderOptions {
                sheet: options.sheet.clone(),
                page: options.page,
                pages: options.pages.clone(),
                out: None,
                out_dir: Some(target.clone()),
                overwrite: options.overwrite,
            };
            match self.render(input, &per_file) {
                Ok(report) => {
                    rendered += 1;
                    results.push(json!({
                        "source": input.display().to_string(),
                        "outDir": target.display().to_string(),
                        "ok": true,
                        "totalPages": report.get("totalPages").cloned().unwrap_or(Value::Null),
                        "written": report.get("written").cloned().unwrap_or_else(|| json!([])),
                    }));
                }
                Err(error) => {
                    failed += 1;
                    results.push(json!({
                        "source": input.display().to_string(),
                        "outDir": target.display().to_string(),
                        "ok": false,
                        "code": error.code(),
                        "params": error.params(),
                        "error": error.message(),
                    }));
                }
            }
        }
        Ok(json!({
            "outDir": out_dir.display().to_string(),
            "rendered": rendered,
            "failed": failed,
            "results": results,
        }))
    }

    /// Inspects a document by dispatching to the owning engine.
    ///
    /// `checks` limits the rule checks Word, Excel and presentation engines
    /// run; `None` runs every check they support. PDF inspection has no
    /// selectable checks, so a non-empty `checks` list is a usage error there.
    pub fn inspect(&self, path: &Path, checks: Option<&[String]>) -> Result<Value, OfficeError> {
        let kind = Kind::of_path(path)?;
        verify_container(path, kind)?;
        let selected = checks.filter(|names| !names.is_empty());
        if selected.is_some() && kind == Kind::Pdf {
            return Err(OfficeError::usage("PDF files have no inspection rule checks; omit `--checks`")
                .with_code("unsupported-option"));
        }
        match kind {
            Kind::Word => {
                let opened = open_word_document::open(path).map_err(engine)?;
                let handle = handle_of(&opened)?;
                let summary = open_word_document::summary(handle).map_err(engine)?;
                let stats = open_word_document::stats(handle).map_err(engine)?;
                let report = open_word_document::inspect(handle, selected.map(<[String]>::to_vec)).map_err(engine)?;
                let _ = open_word_document::close(handle);
                Ok(json!({
                    "kind": kind.label(),
                    "path": path.display().to_string(),
                    "checks": selected,
                    "summary": summary,
                    "stats": stats,
                    "inspect": report,
                }))
            }
            Kind::Excel => {
                let opened = open_excel_workbook::open(path).map_err(engine)?;
                let handle = handle_of(&opened)?;
                let summary = open_excel_workbook::summary(handle).map_err(engine)?;
                let report = open_excel_workbook::inspect(handle, selected.map(<[String]>::to_vec)).map_err(engine)?;
                let _ = open_excel_workbook::close(handle);
                Ok(json!({
                    "kind": kind.label(),
                    "path": path.display().to_string(),
                    "checks": selected,
                    "summary": summary,
                    "inspect": report,
                }))
            }
            Kind::Presentation => {
                let loaded = load_deck(path)?;
                let summary = open_ppt_deck::summary(&loaded.deck);
                let parsed = match selected {
                    Some(names) => Some(open_ppt_deck::parse_checks(&names.join(",")).map_err(engine)?),
                    None => None,
                };
                let report = open_ppt_deck::inspect(&loaded.deck, parsed);
                Ok(json!({
                    "kind": kind.label(),
                    "path": path.display().to_string(),
                    "checks": selected,
                    "summary": summary,
                    "losses": loaded.losses,
                    "inspect": report,
                }))
            }
            Kind::Pdf => {
                let report = open_pdf_core::inspect(path).map_err(pdf_engine)?;
                let report = serde_json::to_value(report).map_err(|error| OfficeError::runtime(error.to_string()))?;
                Ok(json!({
                    "kind": kind.label(),
                    "path": path.display().to_string(),
                    "checks": selected,
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
                let opened = open_word_document::open(input).map_err(engine)?;
                let handle = handle_of(&opened)?;
                let report = open_word_document::export(handle, &format, out, overwrite).map_err(engine)?;
                let _ = open_word_document::close(handle);
                report
            }
            Kind::Excel => {
                let opened = open_excel_workbook::open(input).map_err(engine)?;
                let handle = handle_of(&opened)?;
                let report = open_excel_workbook::export(handle, &format, out, overwrite).map_err(engine)?;
                let _ = open_excel_workbook::close(handle);
                report
            }
            Kind::Presentation => {
                let loaded = load_deck(input)?;
                open_ppt_deck::export(
                    &loaded.deck,
                    &json!({ "format": format, "target": out.display().to_string(), "overwrite": overwrite }),
                )
                .map_err(engine)?
            }
            Kind::Pdf => extract_pdf_text(input, out, options.pages.as_deref(), overwrite)?,
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
                        "params": error.params(),
                        "error": error.message(),
                    }));
                }
            }
        }
        Ok(json!({ "converted": converted, "failed": failed, "results": results }))
    }

    /// Reports lightweight statistics for many documents in input order.
    ///
    /// A failing document does not stop the batch: each entry carries `ok` and
    /// either `kind`/`stats` or the stable `code` and `error` message.
    pub fn stats_many(&self, inputs: &[PathBuf]) -> Result<Value, OfficeError> {
        if inputs.is_empty() {
            return Err(OfficeError::usage("at least one document is required"));
        }
        let mut inspected = 0;
        let mut failed = 0;
        let mut results = Vec::with_capacity(inputs.len());
        for input in inputs {
            match self.stats(input) {
                Ok(report) => {
                    inspected += 1;
                    results.push(json!({
                        "source": input.display().to_string(),
                        "ok": true,
                        "kind": report.get("kind").cloned().unwrap_or(Value::Null),
                        "stats": report.get("stats").cloned().unwrap_or(Value::Null),
                    }));
                }
                Err(error) => {
                    failed += 1;
                    results.push(json!({
                        "source": input.display().to_string(),
                        "ok": false,
                        "code": error.code(),
                        "params": error.params(),
                        "error": error.message(),
                    }));
                }
            }
        }
        Ok(json!({ "inspected": inspected, "failed": failed, "results": results }))
    }

    /// Aggregates the statistics of many documents into one suite-level report.
    ///
    /// Every number still comes from the owning engine through [`Office::stats`];
    /// this only sums them per family: word documents add up blocks, paragraphs,
    /// headings, tables, words and characters, workbooks add up sheets and
    /// cells, presentations add up slides and PDFs add up pages. A failing
    /// document does not stop the batch: it is counted in `failed` and reported
    /// with its stable `code` and `error`, exactly like the other batch verbs.
    pub fn totals(&self, inputs: &[PathBuf]) -> Result<Value, OfficeError> {
        if inputs.is_empty() {
            return Err(OfficeError::usage("at least one document is required"));
        }
        let mut families = json!({
            "word": { "documents": 0, "blocks": 0, "paragraphs": 0, "headings": 0, "tables": 0, "words": 0, "characters": 0 },
            "excel": { "documents": 0, "sheets": 0, "cells": 0 },
            "presentation": { "documents": 0, "slides": 0 },
            "pdf": { "documents": 0, "pages": 0 },
        });
        let mut counted = 0;
        let mut failed = 0;
        let mut results = Vec::with_capacity(inputs.len());
        for input in inputs {
            match self.stats(input) {
                Ok(report) => {
                    counted += 1;
                    let kind = report.get("kind").and_then(Value::as_str).unwrap_or_default().to_owned();
                    let stats = report.get("stats").cloned().unwrap_or(Value::Null);
                    let totals = match kind.as_str() {
                        "word" => json!({
                            "blocks": number_of(&stats, "blocks"),
                            "paragraphs": number_of(&stats, "paragraphs"),
                            "headings": headings_total(&stats),
                            "tables": number_of(&stats, "tables"),
                            "words": number_of(&stats, "words"),
                            "characters": number_of(&stats, "characters"),
                        }),
                        "excel" => json!({
                            "sheets": stats.get("sheets").and_then(Value::as_array).map_or(0, |sheets| sheets.len() as u64),
                            "cells": number_of(&stats, "cells"),
                        }),
                        "presentation" => json!({
                            "slides": stats.get("slides").and_then(Value::as_array).map_or(0, |slides| slides.len() as u64),
                        }),
                        "pdf" => json!({ "pages": number_of(&stats, "pages") }),
                        _ => Value::Null,
                    };
                    if let Some(family) = families.get_mut(&kind) {
                        accumulate_totals(family, &totals);
                    }
                    results.push(json!({
                        "source": input.display().to_string(),
                        "ok": true,
                        "kind": kind,
                        "totals": totals,
                    }));
                }
                Err(error) => {
                    failed += 1;
                    results.push(json!({
                        "source": input.display().to_string(),
                        "ok": false,
                        "code": error.code(),
                        "params": error.params(),
                        "error": error.message(),
                    }));
                }
            }
        }
        Ok(json!({ "counted": counted, "failed": failed, "families": families, "results": results }))
    }

    /// Inspects many documents in input order.
    ///
    /// A failing document does not stop the batch: each entry carries `ok` and
    /// either the full inspect report or the stable `code` and `error` message.
    pub fn inspect_many(&self, inputs: &[PathBuf], checks: Option<&[String]>) -> Result<Value, OfficeError> {
        if inputs.is_empty() {
            return Err(OfficeError::usage("at least one document is required"));
        }
        let mut inspected = 0;
        let mut failed = 0;
        let mut results = Vec::with_capacity(inputs.len());
        for input in inputs {
            match self.inspect(input, checks) {
                Ok(report) => {
                    inspected += 1;
                    results.push(json!({
                        "source": input.display().to_string(),
                        "ok": true,
                        "kind": report.get("kind").cloned().unwrap_or(Value::Null),
                        "inspect": report,
                    }));
                }
                Err(error) => {
                    failed += 1;
                    results.push(json!({
                        "source": input.display().to_string(),
                        "ok": false,
                        "code": error.code(),
                        "params": error.params(),
                        "error": error.message(),
                    }));
                }
            }
        }
        Ok(json!({ "inspected": inspected, "failed": failed, "results": results }))
    }

    /// Opens one document through its family engine and reports what the engine read.
    ///
    /// Nothing is converted or written: the owning engine opens the document and
    /// the report names the routed family, the container the bytes proved, the
    /// engine's own summary with its revision, and every loss or warning the
    /// engine carried. Word and presentation documents report import `losses`,
    /// PDFs report structure `warnings`; a document the engine cannot open
    /// fails with its stable code instead of being guessed about.
    pub fn validate(&self, path: &Path) -> Result<Value, OfficeError> {
        let kind = Kind::of_path(path)?;
        verify_container(path, kind)?;
        let container = self.probe(path)?.get("container").cloned().unwrap_or(Value::Null);
        let (revision, summary, losses, warnings) = match kind {
            Kind::Word => {
                let opened = open_word_document::open(path).map_err(engine)?;
                let handle = handle_of(&opened)?;
                let _ = open_word_document::close(handle);
                let summary = opened.get("document").cloned().unwrap_or(Value::Null);
                (
                    summary.get("revision").cloned().unwrap_or(Value::Null),
                    summary,
                    opened.get("losses").cloned().unwrap_or_else(|| json!([])),
                    json!([]),
                )
            }
            Kind::Excel => {
                let opened = open_excel_workbook::open(path).map_err(engine)?;
                let handle = handle_of(&opened)?;
                let _ = open_excel_workbook::close(handle);
                let summary = opened.get("workbook").cloned().unwrap_or(Value::Null);
                (summary.get("revision").cloned().unwrap_or(Value::Null), summary, json!([]), json!([]))
            }
            Kind::Presentation => {
                let loaded = load_deck(path)?;
                let summary = open_ppt_deck::summary(&loaded.deck);
                (summary.get("revision").cloned().unwrap_or(Value::Null), summary, json!(loaded.losses), json!([]))
            }
            Kind::Pdf => {
                let report = open_pdf_core::inspect(path).map_err(pdf_engine)?;
                (
                    Value::Null,
                    json!({ "version": report.version, "pages": report.pages, "encrypted": report.encrypted }),
                    json!([]),
                    json!(report.warnings),
                )
            }
        };
        Ok(json!({
            "kind": kind.label(),
            "path": path.display().to_string(),
            "readable": true,
            "container": container,
            "revision": revision,
            "summary": summary,
            "losses": losses,
            "warnings": warnings,
        }))
    }

    /// Validates many documents in input order.
    ///
    /// A failing document does not stop the batch: each entry carries `ok` and
    /// either the same fields the single-document [`Office::validate`] report
    /// holds or the stable `code` and `error` message.
    pub fn validate_many(&self, inputs: &[PathBuf]) -> Result<Value, OfficeError> {
        if inputs.is_empty() {
            return Err(OfficeError::usage("at least one document is required"));
        }
        let mut validated = 0;
        let mut failed = 0;
        let mut results = Vec::with_capacity(inputs.len());
        for input in inputs {
            match self.validate(input) {
                Ok(report) => {
                    validated += 1;
                    results.push(json!({
                        "source": input.display().to_string(),
                        "ok": true,
                        "kind": report.get("kind").cloned().unwrap_or(Value::Null),
                        "container": report.get("container").cloned().unwrap_or(Value::Null),
                        "revision": report.get("revision").cloned().unwrap_or(Value::Null),
                        "summary": report.get("summary").cloned().unwrap_or(Value::Null),
                        "losses": report.get("losses").cloned().unwrap_or_else(|| json!([])),
                        "warnings": report.get("warnings").cloned().unwrap_or_else(|| json!([])),
                    }));
                }
                Err(error) => {
                    failed += 1;
                    results.push(json!({
                        "source": input.display().to_string(),
                        "ok": false,
                        "code": error.code(),
                        "params": error.params(),
                        "error": error.message(),
                    }));
                }
            }
        }
        Ok(json!({ "validated": validated, "failed": failed, "results": results }))
    }

    /// Extracts plain text from one document into one output file.
    ///
    /// The owning engine does the reading and the report names the family that
    /// handled the document: a word document exports its text projection as
    /// `txt`, a workbook exports its primary sheet grid as `csv`, a deck is
    /// projected to slide titles, body text and speaker notes as `text`, and a
    /// PDF is projected to its extracted `markdown`. The report carries the
    /// written byte count and every loss the engine kept. An existing target is
    /// refused without `overwrite`, exactly like `convert`.
    pub fn extract(&self, path: &Path, out: &Path, overwrite: bool) -> Result<Value, OfficeError> {
        let kind = Kind::of_path(path)?;
        verify_container(path, kind)?;
        if out.exists() && !overwrite {
            return Err(OfficeError::runtime(format!(
                "{} already exists; pass --overwrite to replace it",
                out.display()
            ))
            .with_code("target-exists"));
        }
        let (format, losses) = match kind {
            Kind::Word => {
                let opened = open_word_document::open(path).map_err(engine)?;
                let handle = handle_of(&opened)?;
                let report = open_word_document::export(handle, "txt", out, overwrite).map_err(engine)?;
                let _ = open_word_document::close(handle);
                ("txt", report.get("losses").cloned().unwrap_or_else(|| json!([])))
            }
            Kind::Excel => {
                let opened = open_excel_workbook::open(path).map_err(engine)?;
                let handle = handle_of(&opened)?;
                let report = open_excel_workbook::export(handle, "csv", out, overwrite).map_err(engine)?;
                let _ = open_excel_workbook::close(handle);
                ("csv", report.get("losses").cloned().unwrap_or_else(|| json!([])))
            }
            Kind::Presentation => {
                let loaded = load_deck(path)?;
                let text = deck_text(&loaded.deck)?;
                publish_output(out, text.as_bytes(), overwrite, "output-unwritable")?;
                ("text", json!(loaded.losses))
            }
            Kind::Pdf => {
                let report = open_pdf_core::extract_text(path).map_err(pdf_engine)?;
                publish_output(out, report.markdown.as_bytes(), overwrite, "output-unwritable")?;
                ("markdown", json!(report.losses))
            }
        };
        let bytes = fs::metadata(out)
            .map_err(|error| OfficeError::runtime(format!("cannot read back {}: {error}", out.display())).with_code("output-unwritable"))?
            .len();
        Ok(json!({
            "kind": kind.label(),
            "path": path.display().to_string(),
            "out": out.display().to_string(),
            "format": format,
            "bytes": bytes,
            "losses": losses,
        }))
    }

    /// Extracts many documents into one text file per source, named after its stem.
    ///
    /// Each family keeps its own natural text extension in the output directory:
    /// word documents and decks become `.txt`, workbooks `.csv` and PDFs `.md`.
    /// A failing document does not stop the batch, exactly like the other batch
    /// verbs: each entry carries `ok` and either the written `target`, family,
    /// `format` and `bytes` or the stable `code` and `error` message.
    pub fn extract_many(&self, inputs: &[PathBuf], out_dir: &Path, overwrite: bool) -> Result<Value, OfficeError> {
        if inputs.is_empty() {
            return Err(OfficeError::usage("at least one document is required"));
        }
        if out_dir.as_os_str().is_empty() {
            return Err(OfficeError::usage("`--out-dir <dir>` is required"));
        }
        fs::create_dir_all(out_dir)
            .map_err(|error| {
                OfficeError::runtime(format!("cannot create {}: {error}", out_dir.display())).with_code("output-unwritable")
            })?;
        let mut extracted = 0;
        let mut failed = 0;
        let mut results = Vec::with_capacity(inputs.len());
        for input in inputs {
            let stem = input.file_stem().map(|value| value.to_string_lossy().into_owned()).unwrap_or_default();
            let extension = match Kind::of_path(input) {
                Ok(Kind::Excel) => "csv",
                Ok(Kind::Pdf) => "md",
                _ => "txt",
            };
            let target = out_dir.join(format!("{stem}.{extension}"));
            match self.extract(input, &target, overwrite) {
                Ok(report) => {
                    extracted += 1;
                    results.push(json!({
                        "source": input.display().to_string(),
                        "target": target.display().to_string(),
                        "ok": true,
                        "kind": report.get("kind").cloned().unwrap_or(Value::Null),
                        "format": report.get("format").cloned().unwrap_or(Value::Null),
                        "bytes": report.get("bytes").cloned().unwrap_or(Value::Null),
                    }));
                }
                Err(error) => {
                    failed += 1;
                    results.push(json!({
                        "source": input.display().to_string(),
                        "target": target.display().to_string(),
                        "ok": false,
                        "code": error.code(),
                        "params": error.params(),
                        "error": error.message(),
                    }));
                }
            }
        }
        Ok(json!({ "extracted": extracted, "failed": failed, "results": results }))
    }
}

/// Name of the platform command that opens a file in its default application.
fn default_opener() -> &'static str {
    if cfg!(windows) {
        "cmd /C start"
    }
    else if cfg!(target_os = "macos") {
        "open"
    }
    else {
        "xdg-open"
    }
}

/// Drops outline items deeper than `depth` and prunes their descendants.
///
/// Word outlines nest by heading level, so a shallow depth keeps the top of
/// the tree and removes every deeper heading together with its children.
fn trim_outline_items(items: &mut Value, depth: u8) {
    let Some(entries) = items.as_array_mut()
    else {
        return;
    };
    entries.retain(|entry| entry.get("level").and_then(Value::as_u64).is_none_or(|level| level <= depth as u64));
    for entry in entries.iter_mut() {
        if let Some(children) = entry.get_mut("children") {
            trim_outline_items(children, depth);
        }
    }
}

/// Cross-checks the extension route against what the bytes really are.
///
/// The probe reads the whole file, not just the header: inside a ZIP container a
/// Word, Spreadsheet and Presentation package all look alike until the parts are
/// read, so a header-only check could not tell XLSX from PPTX. A resolved family
/// that contradicts the route is refused with the probe's own evidence.
#[doc(hidden)]
pub fn verify_container(path: &Path, kind: Kind) -> Result<(), OfficeError> {
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

/// Finds one query in a presentation's slide text, tables and speaker notes.
///
/// The deck is serialized once and walked generically because the shared model
/// keeps text in nested paragraph, run and cell nodes. Only string leaves under
/// a `text` key are searched, so ids, colors and geometry never produce hits.
fn presentation_search(path: &Path, query: &str, limit: usize) -> Result<(Vec<Value>, bool, Vec<Value>), OfficeError> {
    let loaded = load_deck(path)?;
    let document = serde_json::to_value(loaded.deck.document()).map_err(|error| OfficeError::runtime(error.to_string()))?;
    Ok(presentation_hits(&document, query, limit))
}

/// Searches one serialized deck document and reports hit locations.
#[doc(hidden)]
pub fn presentation_hits(document: &Value, query: &str, limit: usize) -> (Vec<Value>, bool, Vec<Value>) {
    let slides = document.get("slides").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut hits = Vec::new();
    'slides: for (index, slide) in slides.iter().enumerate() {
        let slide_id = slide.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
        if let Some(notes) = slide.get("notes").and_then(Value::as_str)
            && !notes.is_empty()
        {
            for (start, end, matched) in match_spans(notes, query, limit - hits.len()) {
                hits.push(json!({
                    "slideId": slide_id,
                    "slideIndex": index,
                    "elementId": Value::Null,
                    "source": "notes",
                    "start": start,
                    "end": end,
                    "text": matched,
                    "context": notes,
                }));
                if hits.len() >= limit {
                    break 'slides;
                }
            }
        }
        let elements = slide.get("elements").and_then(Value::as_array).cloned().unwrap_or_default();
        for element in &elements {
            let element_id = element.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
            let content = element.get("content");
            let source = if content.and_then(|content| content.get("rows")).is_some() { "table" } else { "text" };
            let mut texts = Vec::new();
            collect_json_text(content, &mut texts);
            for text in texts {
                for (start, end, matched) in match_spans(&text, query, limit - hits.len()) {
                    hits.push(json!({
                        "slideId": slide_id,
                        "slideIndex": index,
                        "elementId": element_id,
                        "source": source,
                        "start": start,
                        "end": end,
                        "text": matched,
                        "context": text,
                    }));
                    if hits.len() >= limit {
                        break 'slides;
                    }
                }
            }
        }
    }
    let truncated = hits.len() >= limit;
    let notes = vec![json!("presentations include slide text, table cells and speaker notes")];
    (hits, truncated, notes)
}

/// Finds one query in a PDF's extracted Markdown, one hit per matching line.
fn pdf_search(path: &Path, query: &str, limit: usize) -> Result<(Vec<Value>, bool, Vec<Value>), OfficeError> {
    let report = open_pdf_core::extract_text(path).map_err(pdf_engine)?;
    let mut hits = Vec::new();
    for (index, line) in report.markdown.lines().enumerate() {
        if hits.len() >= limit {
            break;
        }
        for (start, end, matched) in match_spans(line, query, limit - hits.len()) {
            hits.push(json!({
                "line": index + 1,
                "start": start,
                "end": end,
                "text": matched,
                "context": line,
            }));
        }
    }
    let truncated = hits.len() >= limit;
    let mut notes = vec![json!("PDF page numbers are not tracked; `line` is a 1-based line in the extracted Markdown")];
    if !report.losses.is_empty() {
        notes.push(json!(format!(
            "{} page(s) reported extraction losses, so hits only cover extractable text",
            report.losses.len()
        )));
    }
    Ok((hits, truncated, notes))
}

/// Collects every string held under a `text` key inside one content subtree.
#[doc(hidden)]
pub fn collect_json_text(value: Option<&Value>, out: &mut Vec<String>) {
    match value {
        Some(Value::Object(map)) => {
            for (key, value) in map {
                if key == "text" {
                    if let Some(text) = value.as_str() {
                        if !text.is_empty() {
                            out.push(text.to_string());
                        }
                    }
                    else {
                        // Table cells wrap their paragraphs in a nested `text`
                        // object, so recurse instead of treating it as a leaf.
                        collect_json_text(Some(value), out);
                    }
                }
                else {
                    collect_json_text(Some(value), out);
                }
            }
        }
        Some(Value::Array(values)) => {
            for value in values {
                collect_json_text(Some(value), out);
            }
        }
        _ => {}
    }
}

/// Returns character offsets and matched text for up to `limit` occurrences.
#[doc(hidden)]
pub fn match_spans(text: &str, query: &str, limit: usize) -> Vec<(usize, usize, String)> {
    if query.is_empty() || limit == 0 {
        return Vec::new();
    }
    let mut spans = Vec::new();
    for (byte_start, matched) in text.match_indices(query) {
        let start = text[..byte_start].chars().count();
        spans.push((start, start + matched.chars().count(), matched.to_string()));
        if spans.len() >= limit {
            break;
        }
    }
    spans
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
        let deck = open_ppt_deck::open(path).map_err(engine)?;
        Ok(LoadedDeck { deck, losses: Vec::new() })
    }
    else {
        let outcome = open_ppt_deck::import_pptx(path).map_err(engine)?;
        Ok(LoadedDeck { deck: outcome.deck, losses: outcome.losses })
    }
}

/// Projects one deck into plain text: one block per slide with its title, the
/// text leaves of its elements and its speaker notes.
///
/// The deck engine keeps text in nested paragraph, run and table nodes, so each
/// slide is serialized and walked generically, exactly like presentation
/// search does.
fn deck_text(deck: &open_ppt_deck::EditableDeck) -> Result<String, OfficeError> {
    let document = serde_json::to_value(deck.document())
        .map_err(|error| OfficeError::runtime(error.to_string()))?;
    let slides = document.get("slides").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut blocks = Vec::with_capacity(slides.len());
    for (index, slide) in slides.iter().enumerate() {
        let title = deck.slides().get(index).map(|slide| slide.title()).unwrap_or_default();
        let heading = if title.is_empty() {
            format!("## Slide {}", index + 1)
        }
        else {
            format!("## Slide {}: {title}", index + 1)
        };
        let mut lines = vec![heading];
        collect_json_text(slide.get("elements"), &mut lines);
        if let Some(notes) = slide.get("notes").and_then(Value::as_str).map(str::trim).filter(|notes| !notes.is_empty()) {
            lines.push(format!("Notes: {notes}"));
        }
        blocks.push(lines.join("\n"));
    }
    Ok(blocks.join("\n\n"))
}

/// Writes the extracted Markdown projection of a PDF to a text target.
fn extract_pdf_text(input: &Path, out: &Path, pages: Option<&str>, overwrite: bool) -> Result<Value, OfficeError> {
    let (report, selection) = match pages {
        Some(ranges) => {
            let (report, selected) = extract_pdf_pages(input, ranges)?;
            (report, Some((ranges, selected)))
        }
        None => (open_pdf_core::extract_text(input).map_err(pdf_engine)?, None),
    };
    publish_output(out, report.markdown.as_bytes(), overwrite, "output-unwritable")?;
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
    let info = open_pdf_core::inspect(input).map_err(pdf_engine)?;
    let selected = parse_page_ranges(ranges, info.pages)?;
    if selected.len() == info.pages && selected.iter().copied().eq(1..=info.pages) {
        return Ok((open_pdf_core::extract_text(input).map_err(pdf_engine)?, selected));
    }
    let remove = (1..=info.pages).filter(|page| !selected.contains(page)).collect::<Vec<_>>();
    let temporary = temporary_pdf_path();
    let request = open_pdf_core::DocumentArrange { order: Some(selected.clone()), rotate: None, remove: Some(remove) };
    open_pdf_core::arrange(input, &temporary, &request).map_err(pdf_engine)?;
    let extracted = open_pdf_core::extract_text(&temporary).map_err(pdf_engine);
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

/// A word-document handle that closes itself when the replace path leaves scope.
struct WordSession(u64);

impl Drop for WordSession {
    fn drop(&mut self) {
        let _ = open_word_document::close(self.0);
    }
}

/// A workbook handle that closes itself when the replace path leaves scope.
struct ExcelSession(u64);

impl Drop for ExcelSession {
    fn drop(&mut self) {
        let _ = open_excel_workbook::close(self.0);
    }
}

#[doc(hidden)]
pub fn handle_of(opened: &Value) -> Result<u64, OfficeError> {
    opened
        .get("handle")
        .and_then(Value::as_u64)
        .ok_or_else(|| OfficeError::runtime("the engine did not return a document handle"))
}

fn write_svg(path: &Path, page: &Value, overwrite: bool) -> Result<(), OfficeError> {
    if path.exists() && !overwrite {
        return Err(OfficeError::runtime(format!("{} already exists; pass --overwrite to replace it", path.display()))
            .with_code("target-exists"));
    }
    let svg = page
        .get("svg")
        .and_then(Value::as_str)
        .ok_or_else(|| OfficeError::runtime("the engine returned no SVG for this page"))?;
    publish_output(path, svg.as_bytes(), overwrite, "unwritable")
}

/// Publishes output bytes atomically through `boost-fs`.
///
/// Bytes are staged in a sibling temporary file and linked or renamed into place, so a reader sees
/// either the previous file or the complete new one, never a partial write. Without `overwrite` an
/// existing target is refused even if it appeared after the caller's own existence check; a failure
/// before the destination is published removes the staging file.
#[doc(hidden)]
pub fn publish_output(path: &Path, bytes: &[u8], overwrite: bool, code: &str) -> Result<(), OfficeError> {
    let options = boost_fs::PublishOptions {
        overwrite: if overwrite { boost_fs::OverwritePolicy::Replace } else { boost_fs::OverwritePolicy::RefuseExisting },
        ..Default::default()
    };
    boost_fs::publish_bytes_with_options(path, bytes, options).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            OfficeError::runtime(format!("{} already exists; pass --overwrite to replace it", path.display())).with_code("target-exists")
        } else {
            OfficeError::runtime(format!("cannot write {}: {error}", path.display())).with_code(code)
        }
    })
}

/// Projects an engine failure onto the suite envelope.
///
/// The suite owns the envelope and the localized copy of its own guardrails;
/// the engine's stable code and frozen parameters pass through unchanged. The
/// message is display-only, so replacing it never changes the machine branch or
/// the parameters a caller receives.
#[doc(hidden)]
pub fn project_engine_failure(code: &str, message: impl Into<String>, params: Option<Value>) -> OfficeError {
    OfficeError {
        message: message.into(),
        usage: false,
        code: code.to_owned(),
        params: params.as_ref().and_then(frozen_params),
    }
}

/// Wraps an engine failure, keeping its stable code and frozen parameters.
fn engine(error: impl EngineFailure) -> OfficeError {
    project_engine_failure(error.stable_code(), error.stable_message(), error.stable_params())
}

/// Wraps a PDF failure, lifting the upstream error into the product-surface type.
///
/// The shared `document-pdf` entry points still report the upstream string
/// error, while the product surface owns the typed `ServiceError`; the product
/// type converts from the upstream one losslessly, so both shapes reach the
/// suite through the same bridge.
fn pdf_engine(error: impl Into<open_pdf_core::ServiceError>) -> OfficeError {
    engine(error.into())
}

/// The export route that writes a word document back under its own extension.
fn word_persist_format(extension: &str) -> Option<&'static str> {
    match extension {
        "docx" | "docm" => Some("docx"),
        "rtf" => Some("rtf"),
        "txt" => Some("txt"),
        "md" | "markdown" => Some("md"),
        "html" | "htm" => Some("html"),
        _ => None,
    }
}

/// The export route that writes a workbook back under its own extension.
fn excel_persist_format(extension: &str) -> Option<&'static str> {
    match extension {
        "xlsx" | "xlsm" => Some("xlsx"),
        "csv" => Some("csv"),
        "tsv" => Some("tsv"),
        _ => None,
    }
}

/// The export route that writes a presentation back under its own extension.
///
/// Macro-enabled packages have no export route, so they keep their macro-aware
/// refusal instead of being rewritten as a plain package.
fn presentation_persist_format(extension: &str) -> Option<&'static str> {
    match extension {
        "pptx" => Some("pptx"),
        "ppsx" => Some("ppsx"),
        "potx" => Some("potx"),
        _ => None,
    }
}

/// A routed extension that has no in-place replacement write-back route.
fn unsupported_extension_route(extension: &str) -> OfficeError {
    OfficeError::usage(format!("`.{extension}` has no in-place replacement route in this build")).with_code("unsupported-target")
}

/// Reads one numeric field from an engine stats object, defaulting to zero.
fn number_of(stats: &Value, key: &str) -> u64 {
    stats.get(key).and_then(Value::as_u64).unwrap_or_default()
}

/// Sums the per-level heading counts a word document reports.
fn headings_total(stats: &Value) -> u64 {
    stats.get("headings").and_then(Value::as_object).map(|levels| levels.values().filter_map(Value::as_u64).sum()).unwrap_or_default()
}

/// Adds one document's family totals into the aggregate and counts the document.
fn accumulate_totals(family: &mut Value, totals: &Value) {
    let (Some(target), Some(source)) = (family.as_object_mut(), totals.as_object()) else {
        return;
    };
    for (key, value) in source {
        if let Some(current) = target.get(key).and_then(Value::as_u64) {
            target.insert(key.clone(), json!(current + value.as_u64().unwrap_or_default()));
        }
    }
    let documents = target.get("documents").and_then(Value::as_u64).unwrap_or_default() + 1;
    target.insert("documents".to_string(), json!(documents));
}

/// Walks one engine outline projection and collects its section titles in
/// document order: word heading `title`s (with `children` flattened), workbook
/// sheet `name`s, slide `title`s and PDF bookmark `title`s.
fn collect_titles(value: &Value, titles: &mut Vec<String>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_titles(item, titles);
            }
        }
        Value::Object(map) => {
            if let Some(title) = map.get("title").and_then(Value::as_str) {
                titles.push(title.to_owned());
            }
            else if let Some(name) = map.get("name").and_then(Value::as_str) {
                titles.push(name.to_owned());
            }
            for key in ["items", "children", "sheets", "slides", "bookmarks"] {
                if let Some(list) = map.get(key) {
                    collect_titles(list, titles);
                }
            }
        }
        _ => {}
    }
}

/// Aligns two ordered section lists with a longest-common-subsequence walk.
///
/// A title that appears on both sides outside the common subsequence kept its
/// content but changed position, so it is reported under `moved` instead of
/// being counted as an add and a remove. Inside each remaining change block the
/// removed titles are paired positionally with the added titles and reported as
/// renames; unpaired entries stay in the `added` and `removed` lists.
fn diff_sections(
    left: &[String],
    right: &[String],
    ignore_case: bool,
) -> (Vec<String>, Vec<String>, Vec<Value>, Vec<Value>) {
    let same = |left: &str, right: &str| {
        if ignore_case {
            left.to_lowercase() == right.to_lowercase()
        }
        else {
            left == right
        }
    };
    let key = |value: &str| if ignore_case { value.to_lowercase() } else { value.to_owned() };
    let (n, m) = (left.len(), right.len());
    let mut table = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[i][j] = if same(&left[i], &right[j]) {
                table[i + 1][j + 1] + 1
            }
            else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }
    let mut script = Vec::with_capacity(n + m);
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if same(&left[i], &right[j]) {
            script.push(Edit::Equal(i, j));
            i += 1;
            j += 1;
        }
        else if table[i + 1][j] >= table[i][j + 1] {
            script.push(Edit::Removed(i));
            i += 1;
        }
        else {
            script.push(Edit::Added(j));
            j += 1;
        }
    }
    script.extend((i..n).map(Edit::Removed));
    script.extend((j..m).map(Edit::Added));

    // Titles outside the common subsequence that still exist on both sides only
    // changed position, so they pair up here before renames are considered.
    let mut matched_left = vec![false; n];
    let mut matched_right = vec![false; m];
    for step in &script {
        if let Edit::Equal(left_at, right_at) = step {
            matched_left[*left_at] = true;
            matched_right[*right_at] = true;
        }
    }
    let mut right_by_key: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, title) in right.iter().enumerate().filter(|(index, _)| !matched_right[*index]) {
        right_by_key.entry(key(title)).or_default().push(index);
    }
    let mut moved = Vec::new();
    for (index, title) in left.iter().enumerate().filter(|(index, _)| !matched_left[*index]) {
        let Some(candidates) = right_by_key.get_mut(&key(title))
        else {
            continue;
        };
        let Some(position) = candidates.first().copied()
        else {
            continue;
        };
        candidates.remove(0);
        moved.push(json!({ "title": title, "from": index, "to": position }));
    }
    let moved_titles = moved.iter().filter_map(|entry| entry["title"].as_str()).map(key).collect::<HashSet<_>>();

    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut renamed = Vec::new();
    let mut index = 0;
    while index < script.len() {
        if matches!(script[index], Edit::Equal(..)) {
            index += 1;
            continue;
        }
        let mut block_removed = Vec::new();
        let mut block_added = Vec::new();
        while index < script.len() && !matches!(script[index], Edit::Equal(..)) {
            match script[index] {
                Edit::Removed(at) => {
                    if !moved_titles.contains(&key(&left[at])) {
                        block_removed.push(left[at].clone());
                    }
                }
                Edit::Added(at) => {
                    if !moved_titles.contains(&key(&right[at])) {
                        block_added.push(right[at].clone());
                    }
                }
                Edit::Equal(..) => {}
            }
            index += 1;
        }
        let paired = block_removed.len().min(block_added.len());
        for (from, to) in block_removed.iter().take(paired).zip(block_added.iter().take(paired)) {
            renamed.push(json!({ "from": from, "to": to }));
        }
        removed.extend(block_removed.into_iter().skip(paired));
        added.extend(block_added.into_iter().skip(paired));
    }
    (added, removed, renamed, moved)
}

/// One step of an outline alignment between two documents.
enum Edit {
    Equal(usize, usize),
    Removed(usize),
    Added(usize),
}

