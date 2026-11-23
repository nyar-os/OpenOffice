//! Command-line entry point for the four document engines.
//!
//! Parsing, help text and exit codes live here; routing, inspection and
//! conversion live in the `open_office` library.

use std::{
    path::{Path, PathBuf},
    process::ExitCode,
};

use open_office::{ConvertOptions, Office, OfficeError, RenderOptions, ReplaceOptions, VERSION};
use serde_json::Value;

fn main() -> ExitCode {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    match run(&arguments) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) if failure.is_usage() => {
            eprintln!("open-office: {}\n", failure.message());
            usage();
            ExitCode::from(2)
        }
        Err(failure) => {
            eprintln!("open-office: {}", failure.message());
            ExitCode::from(1)
        }
    }
}

fn run(arguments: &[String]) -> Result<(), OfficeError> {
    let office = Office::new();
    let Some(command) = arguments.first().map(String::as_str)
    else {
        usage();
        return Err(OfficeError::usage("a command is required"));
    };
    let rest = &arguments[1..];
    match command {
        "version" | "--version" | "-V" => {
            println!("open-office {VERSION}");
            Ok(())
        }
        "capabilities" => print_json(&office.capabilities()),
        "formats" => print_json(&office.formats()),
        "probe" | "inspect-file" | "inspect_file" => probe(&office, rest),
        "inspect" => inspect(&office, rest),
        "stats" => stats(&office, rest),
        "totals" => totals(&office, rest),
        "metadata" => metadata(&office, rest),
        "outline" => outline(&office, rest),
        "compare" => compare(&office, rest),
        "search" => search(&office, rest),
        "replace" => replace(&office, rest),
        "render" => render(&office, rest),
        "convert" => convert(&office, rest),
        "open" => open(&office, rest),
        "help" | "--help" | "-h" => {
            usage();
            Ok(())
        }
        other => Err(OfficeError::usage(format!("unknown command `{other}`"))),
    }
}

/// Parses `probe` paths and reports one file or a batch.
///
/// One path keeps the single-document envelope; more than one path returns a
/// per-input batch report and exits non-zero when any input failed.
fn probe(office: &Office, arguments: &[String]) -> Result<(), OfficeError> {
    let paths = document_paths(arguments, "a file path is required")?;
    if paths.len() == 1 {
        return print_json(&office.probe(&paths[0])?);
    }
    let report = office.probe_many(&paths)?;
    print_json(&report)?;
    let failed = report.get("failed").and_then(Value::as_u64).unwrap_or_default();
    if failed > 0 {
        return Err(OfficeError::runtime(format!("{failed} of {} files failed", paths.len())));
    }
    Ok(())
}

/// Parses `stats` paths and hands one or many requests to the library.
///
/// One path keeps the single-document envelope; more than one path returns a
/// per-input batch report and exits non-zero when any input failed.
fn stats(office: &Office, arguments: &[String]) -> Result<(), OfficeError> {
    let paths = document_paths(arguments, "a document path is required")?;
    if paths.len() == 1 {
        return print_json(&office.stats(&paths[0])?);
    }
    let report = office.stats_many(&paths)?;
    print_json(&report)?;
    let failed = report.get("failed").and_then(Value::as_u64).unwrap_or_default();
    if failed > 0 {
        return Err(OfficeError::runtime(format!("{failed} of {} documents failed", paths.len())));
    }
    Ok(())
}

/// Parses `totals` paths and reports one aggregated suite-level summary.
///
/// Every document contributes the numbers its owning engine reports; a failing
/// document is counted in `failed` and the command exits non-zero after
/// printing the report.
fn totals(office: &Office, arguments: &[String]) -> Result<(), OfficeError> {
    let paths = document_paths(arguments, "a document path is required")?;
    let report = office.totals(&paths)?;
    print_json(&report)?;
    let failed = report.get("failed").and_then(Value::as_u64).unwrap_or_default();
    if failed > 0 {
        return Err(OfficeError::runtime(format!("{failed} of {} documents failed", paths.len())));
    }
    Ok(())
}

/// Parses `inspect` paths and reports one document or a batch.
///
/// One path keeps the single-document envelope; more than one path returns a
/// per-input batch report and exits non-zero when any input failed.
fn inspect(office: &Office, arguments: &[String]) -> Result<(), OfficeError> {
    let mut checks: Option<Vec<String>> = None;
    let mut paths = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--checks" => {
                let value = arguments
                    .get(index + 1)
                    .ok_or_else(|| OfficeError::usage("`--checks` needs a comma-separated list of check names"))?;
                let names = value
                    .split(',')
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned)
                    .collect::<Vec<_>>();
                if names.is_empty() {
                    return Err(OfficeError::usage("`--checks` needs at least one check name"));
                }
                checks = Some(names);
                index += 2;
            }
            option if option.starts_with('-') => {
                return Err(OfficeError::usage(format!("unknown option `{option}`")));
            }
            path => {
                paths.push(PathBuf::from(path));
                index += 1;
            }
        }
    }
    if paths.is_empty() {
        return Err(OfficeError::usage("a document path is required"));
    }
    let checks = checks.as_deref();
    if paths.len() == 1 {
        return print_json(&office.inspect(&paths[0], checks)?);
    }
    let report = office.inspect_many(&paths, checks)?;
    print_json(&report)?;
    let failed = report.get("failed").and_then(Value::as_u64).unwrap_or_default();
    if failed > 0 {
        return Err(OfficeError::runtime(format!("{failed} of {} documents failed", paths.len())));
    }
    Ok(())
}

/// Collects positional document paths, rejecting options and an empty list.
fn document_paths(arguments: &[String], missing: &str) -> Result<Vec<PathBuf>, OfficeError> {
    let mut paths = Vec::new();
    for argument in arguments {
        if argument.starts_with('-') {
            return Err(OfficeError::usage(format!("unknown option `{argument}`")));
        }
        paths.push(PathBuf::from(argument));
    }
    if paths.is_empty() {
        return Err(OfficeError::usage(missing));
    }
    Ok(paths)
}

/// Parses `outline` paths and options, reporting one document or a batch.
///
/// One path keeps the single-document envelope; more than one path returns a
/// per-input batch report and exits non-zero when any input failed.
fn outline(office: &Office, arguments: &[String]) -> Result<(), OfficeError> {
    let mut paths = Vec::new();
    let mut points = false;
    let mut depth = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--points" => points = true,
            "--depth" => {
                let value = value_of(arguments, &mut index, "--depth")?;
                let parsed = value
                    .parse::<u8>()
                    .ok()
                    .filter(|depth| *depth > 0)
                    .ok_or_else(|| OfficeError::usage("`--depth` needs a positive integer"))?;
                depth = Some(parsed);
            }
            value if value.starts_with('-') => {
                return Err(OfficeError::usage(format!("unknown option `{value}`")));
            }
            value => paths.push(PathBuf::from(value)),
        }
        index += 1;
    }
    if paths.is_empty() {
        return Err(OfficeError::usage("a document path is required"));
    }
    if paths.len() == 1 {
        return print_json(&office.outline(&paths[0], points, depth)?);
    }
    let report = office.outline_many(&paths, points, depth)?;
    print_json(&report)?;
    let failed = report.get("failed").and_then(Value::as_u64).unwrap_or_default();
    if failed > 0 {
        return Err(OfficeError::runtime(format!("{failed} of {} documents failed", paths.len())));
    }
    Ok(())
}

/// Parses `compare` paths and reports one outline-level diff.
///
/// Exactly two documents of the same family are required; the diff lists the
/// sections added, removed and renamed between the left and the right outline.
fn compare(office: &Office, arguments: &[String]) -> Result<(), OfficeError> {
    let mut paths = Vec::new();
    let mut ignore_case = false;
    for argument in arguments {
        match argument.as_str() {
            "--ignore-case" | "-i" => ignore_case = true,
            flag if flag.starts_with('-') => {
                return Err(OfficeError::usage(format!("unknown option `{flag}`")));
            }
            path => paths.push(PathBuf::from(path)),
        }
    }
    if paths.len() != 2 {
        return Err(OfficeError::usage("`compare` needs exactly two document paths"));
    }
    print_json(&office.compare_with(&paths[0], &paths[1], ignore_case)?)
}

/// Parses `search` paths and options, reporting one document or a batch.
///
/// One path keeps the single-document envelope; more than one path returns a
/// per-input batch report and exits non-zero when any input failed.
fn search(office: &Office, arguments: &[String]) -> Result<(), OfficeError> {
    let mut paths = Vec::new();
    let mut text = None;
    let mut limit = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--text" | "-t" => {
                text = Some(value_of(arguments, &mut index, "--text")?);
            }
            "--limit" => {
                let value = value_of(arguments, &mut index, "--limit")?;
                limit = Some(value.parse::<u32>().map_err(|_| OfficeError::usage("`--limit` needs a positive integer"))?);
            }
            value if value.starts_with('-') => {
                return Err(OfficeError::usage(format!("unknown option `{value}`")));
            }
            value => paths.push(PathBuf::from(value)),
        }
        index += 1;
    }
    if paths.is_empty() {
        return Err(OfficeError::usage("a document path is required"));
    }
    let text = text.ok_or_else(|| OfficeError::usage("`--text <query>` is required"))?;
    if paths.len() == 1 {
        return print_json(&office.search(&paths[0], &text, limit)?);
    }
    let report = office.search_many(&paths, &text, limit)?;
    print_json(&report)?;
    let failed = report.get("failed").and_then(Value::as_u64).unwrap_or_default();
    if failed > 0 {
        return Err(OfficeError::runtime(format!("{failed} of {} documents failed", paths.len())));
    }
    Ok(())
}

/// Parses `replace` paths and options, reporting one document or a batch.
///
/// One path keeps the single-document envelope; more than one path returns a
/// per-input batch report and exits non-zero when any input failed. Every
/// document that changes is saved in place in its own format.
fn replace(office: &Office, arguments: &[String]) -> Result<(), OfficeError> {
    let mut paths = Vec::new();
    let mut find = None;
    let mut with = None;
    let mut ignore_case = false;
    let mut dry_run = false;
    let mut limit = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--find" | "-f" => {
                find = Some(value_of(arguments, &mut index, "--find")?);
            }
            "--with" | "-w" => {
                with = Some(value_of(arguments, &mut index, "--with")?);
            }
            "--ignore-case" | "-i" => ignore_case = true,
            "--dry-run" => dry_run = true,
            "--limit" => {
                let value = value_of(arguments, &mut index, "--limit")?;
                limit = Some(
                    value
                        .parse::<u32>()
                        .ok()
                        .filter(|limit| *limit > 0)
                        .ok_or_else(|| OfficeError::usage("`--limit` needs a positive integer"))?,
                );
            }
            value if value.starts_with('-') => {
                return Err(OfficeError::usage(format!("unknown option `{value}`")));
            }
            value => paths.push(PathBuf::from(value)),
        }
        index += 1;
    }
    if paths.is_empty() {
        return Err(OfficeError::usage("a document path is required"));
    }
    let find = find.ok_or_else(|| OfficeError::usage("`--find <query>` is required"))?;
    let with = with.ok_or_else(|| OfficeError::usage("`--with <replacement>` is required"))?;
    let options = ReplaceOptions { ignore_case, limit, dry_run };
    if paths.len() == 1 {
        return print_json(&office.replace(&paths[0], &find, &with, &options)?);
    }
    let report = office.replace_many(&paths, &find, &with, &options)?;
    print_json(&report)?;
    let failed = report.get("failed").and_then(Value::as_u64).unwrap_or_default();
    if failed > 0 {
        return Err(OfficeError::runtime(format!("{failed} of {} documents failed", paths.len())));
    }
    Ok(())
}

/// Parses `render` options and hands one request to the library.
fn render(office: &Office, arguments: &[String]) -> Result<(), OfficeError> {
    let mut paths: Vec<String> = Vec::new();
    let mut sheet = None;
    let mut page = None;
    let mut out = None;
    let mut out_dir = None;
    let mut overwrite = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--sheet" | "-s" => sheet = Some(value_of(arguments, &mut index, "--sheet")?),
            "--page" | "-p" => {
                let value = value_of(arguments, &mut index, "--page")?;
                page = Some(value.parse::<u32>().map_err(|_| OfficeError::usage("`--page` needs a positive integer"))?);
            }
            "--out" | "-o" => out = Some(PathBuf::from(value_of(arguments, &mut index, "--out")?)),
            "--out-dir" => out_dir = Some(PathBuf::from(value_of(arguments, &mut index, "--out-dir")?)),
            "--overwrite" => overwrite = true,
            value if value.starts_with('-') => {
                return Err(OfficeError::usage(format!("unknown option `{value}`")));
            }
            value => paths.push(value.to_string()),
        }
        index += 1;
    }
    if paths.is_empty() {
        return Err(OfficeError::usage("a document path is required"));
    }
    let options = RenderOptions { sheet, page, out, out_dir, overwrite };
    if paths.len() == 1 {
        return print_json(&office.render(Path::new(&paths[0]), &options)?);
    }
    let inputs = paths.iter().map(PathBuf::from).collect::<Vec<_>>();
    let report = office.render_many(&inputs, &options)?;
    print_json(&report)?;
    let failed = report.get("failed").and_then(Value::as_u64).unwrap_or_default();
    if failed > 0 {
        return Err(OfficeError::runtime(format!("{failed} of {} documents failed", inputs.len())));
    }
    Ok(())
}

/// Parses `open` options and opens one file with the platform viewer.
///
/// `--print` reports the routed family and opener without launching anything,
/// which is the only form that is meaningful on headless hosts.
fn open(office: &Office, arguments: &[String]) -> Result<(), OfficeError> {
    let mut path = None;
    let mut print_only = false;
    for argument in arguments {
        match argument.as_str() {
            "--print" => print_only = true,
            value if value.starts_with('-') => {
                return Err(OfficeError::usage(format!("unknown option `{value}`")));
            }
            value if path.is_none() => path = Some(value.to_string()),
            _ => return Err(OfficeError::usage("open takes exactly one document path")),
        }
    }
    let path = path.ok_or_else(|| OfficeError::usage("a document path is required"))?;
    print_json(&office.open(Path::new(&path), print_only)?)
}

/// Parses `convert` options and hands one request to the library.
fn convert(office: &Office, arguments: &[String]) -> Result<(), OfficeError> {
    let mut inputs: Vec<String> = Vec::new();
    let mut format = None;
    let mut out = None;
    let mut out_dir = None;
    let mut pages = None;
    let mut overwrite = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--format" | "-f" => {
                format = Some(value_of(arguments, &mut index, "--format")?);
            }
            "--out" | "-o" => {
                out = Some(value_of(arguments, &mut index, "--out")?);
            }
            "--out-dir" => {
                out_dir = Some(value_of(arguments, &mut index, "--out-dir")?);
            }
            "--pages" => {
                pages = Some(value_of(arguments, &mut index, "--pages")?);
            }
            "--overwrite" => overwrite = true,
            value if value.starts_with('-') => {
                return Err(OfficeError::usage(format!("unknown option `{value}`")));
            }
            value => inputs.push(value.to_string()),
        }
        index += 1;
    }
    if inputs.is_empty() {
        return Err(OfficeError::usage("a document path is required"));
    }
    let format = format.ok_or_else(|| OfficeError::usage("`--format <format>` is required"))?;
    if inputs.len() == 1 {
        if out_dir.is_some() {
            return Err(OfficeError::usage(
                "`--out-dir` needs more than one input; use `--out <file>` for a single file",
            ));
        }
        let out = out.ok_or_else(|| OfficeError::usage("`--out <file>` is required"))?;
        let options = ConvertOptions { pages };
        return print_json(&office.convert_with(Path::new(&inputs[0]), &format, Path::new(&out), overwrite, &options)?);
    }
    if pages.is_some() {
        return Err(OfficeError::usage("`--pages` cannot be combined with multiple inputs"));
    }
    if out.is_some() {
        return Err(OfficeError::usage("`--out` cannot be used with multiple inputs; pass `--out-dir <dir>`"));
    }
    let out_dir = out_dir.ok_or_else(|| {
        OfficeError::usage("`--out-dir <dir>` is required when converting more than one input")
    })?;
    let paths = inputs.into_iter().map(PathBuf::from).collect::<Vec<_>>();
    let report = office.convert_many(&paths, &format, Path::new(&out_dir), overwrite)?;
    let failed = report.get("failed").and_then(Value::as_u64).unwrap_or_default();
    print_json(&report)?;
    if failed > 0 {
        return Err(OfficeError::runtime(format!("{failed} of {} conversions failed", paths.len())));
    }
    Ok(())
}

fn value_of(arguments: &[String], index: &mut usize, name: &str) -> Result<String, OfficeError> {
    let next = *index + 1;
    let value = arguments.get(next).ok_or_else(|| OfficeError::usage(format!("`{name}` needs a value")))?;
    *index = next;
    Ok(value.clone())
}

fn print_json(value: &Value) -> Result<(), OfficeError> {
    let text = serde_json::to_string_pretty(value).map_err(|error| OfficeError::runtime(error.to_string()))?;
    println!("{text}");
    Ok(())
}

/// Parses `metadata` and reports the family metadata of one or more documents.
fn metadata(office: &Office, arguments: &[String]) -> Result<(), OfficeError> {
    let paths = document_paths(arguments, "a document path is required")?;
    if paths.len() == 1 {
        return print_json(&office.metadata(&paths[0])?);
    }
    print_json(&office.metadata_many(&paths)?)
}

fn usage() {
    println!(
        "open-office {VERSION}\n\
         One inspector and converter for docx/rtf/md/html/txt, xlsx/csv,\n\
         pptx and PDF files.\n\
         \n\
         USAGE:\n\
         \x20 open-office probe <file>...\n\
         \x20 open-office inspect <file>... [--checks <list>]\n\
         \x20 open-office stats <file>...\n\
         \x20 open-office totals <file>...\n\
         \x20 open-office metadata <file>...\n\
         \x20 open-office outline <file>... [--points] [--depth <n>]\n\
         \x20 open-office compare <left> <right> [--ignore-case]\n\
         \x20 open-office search <file>... --text <query> [--limit <n>]\n\
         \x20 open-office replace <file>... --find <query> --with <replacement> [--ignore-case] [--limit <n>] [--dry-run]\n\
         \x20 open-office render <file> [--sheet <name>] [--page <n>] [--out <file> | --out-dir <dir>] [--overwrite]\n\
         \x20 open-office render <file>... --out-dir <dir> [--sheet <name> | --page <n>] [--overwrite]\n\
         \x20 open-office formats\n\
         \x20 open-office convert <file> --format <format> --out <file> [--overwrite] [--pages <ranges>]\n\
         \x20 open-office convert <file>... --format <format> --out-dir <dir> [--overwrite]\n\
         \x20 open-office capabilities\n\
         \x20 open-office version\n\
         \n\
         EXAMPLES:\n\
         \x20 open-office probe download.bin\n\
         \x20 open-office probe a.bin b.docx c.pdf\n\
         \x20 open-office inspect report.docx\n\
         \x20 open-office open deck.pptx\n\
         \x20 open-office open report.docx --print\n\
         \x20 open-office inspect a.docx b.pdf c.xlsx\n\
         \x20 open-office stats report.docx\n\
         \x20 open-office stats a.docx b.pdf c.xlsx\n\
         \x20 open-office totals a.docx b.pdf c.xlsx deck.pptx\n\
         \x20 open-office metadata report.docx\n\
         \x20 open-office outline report.docx\n\
         \x20 open-office outline deck.pptx --points\n\
         \x20 open-office outline report.docx --depth 2\n\
         \x20 open-office compare draft.docx final.docx\n\
         \x20 open-office outline a.docx b.pdf c.pptx\n\
         \x20 open-office search report.docx --text invoice\n\
         \x20 open-office search a.docx b.pdf c.pptx --text total --limit 10\n\
         \x20 open-office replace notes.md --find Acme --with Globex\n\
         \x20 open-office replace a.docx b.xlsx --find invoice --with statement --ignore-case\n\
         \x20 open-office render budget.xlsx --out sheet.svg\n\
         \x20 open-office render deck.pptx --page 2 --out slide-2.svg\n\
         \x20 open-office render deck.pptx --out-dir preview/\n\
         \x20 open-office render budget.xlsx deck.pptx --out-dir preview/\n\
         \x20 open-office formats\n\
         \x20 open-office convert budget.xlsx --format pdf --out budget.pdf\n\
         \x20 open-office convert deck.pptx --format pdf --out deck.pdf\n\
         \x20 open-office convert notes.pdf --format md --out notes.md\n\
         \x20 open-office convert paper.pdf --format md --out paper.md --pages 1-3,5\n\
         \x20 open-office convert a.docx b.xlsx --format pdf --out-dir out"
    );
}
