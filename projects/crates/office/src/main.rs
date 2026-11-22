//! Command-line entry point for the four document engines.
//!
//! Parsing, help text and exit codes live here; routing, inspection and
//! conversion live in the `open_office` library.

use std::{
    path::{Path, PathBuf},
    process::ExitCode,
};

use open_office::{ConvertOptions, Office, OfficeError, RenderOptions, VERSION};
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
        "probe" | "inspect-file" | "inspect_file" => {
            let path = positional(rest, "a file path is required")?;
            print_json(&office.probe(Path::new(&path))?)
        }
        "inspect" => {
            let path = positional(rest, "a document path is required")?;
            print_json(&office.inspect(Path::new(&path))?)
        }
        "stats" => stats(&office, rest),
        "outline" => outline(&office, rest),
        "search" => search(&office, rest),
        "render" => render(&office, rest),
        "convert" => convert(&office, rest),
        "help" | "--help" | "-h" => {
            usage();
            Ok(())
        }
        other => Err(OfficeError::usage(format!("unknown command `{other}`"))),
    }
}

/// Parses `stats` paths and hands one or many requests to the library.
///
/// One path keeps the single-document envelope; more than one path returns a
/// per-input batch report and exits non-zero when any input failed.
fn stats(office: &Office, arguments: &[String]) -> Result<(), OfficeError> {
    let mut paths = Vec::new();
    for argument in arguments {
        if argument.starts_with('-') {
            return Err(OfficeError::usage(format!("unknown option `{argument}`")));
        }
        paths.push(PathBuf::from(argument));
    }
    if paths.is_empty() {
        return Err(OfficeError::usage("a document path is required"));
    }
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

/// Parses `outline` options and hands one request to the library.
fn outline(office: &Office, arguments: &[String]) -> Result<(), OfficeError> {
    let mut path = None;
    let mut points = false;
    for argument in arguments {
        match argument.as_str() {
            "--points" => points = true,
            value if value.starts_with('-') => {
                return Err(OfficeError::usage(format!("unknown option `{value}`")));
            }
            value if path.is_none() => path = Some(value.to_string()),
            _ => return Err(OfficeError::usage("outline takes exactly one document path")),
        }
    }
    let path = path.ok_or_else(|| OfficeError::usage("a document path is required"))?;
    print_json(&office.outline(Path::new(&path), points)?)
}

/// Parses `search` options and hands one request to the library.
fn search(office: &Office, arguments: &[String]) -> Result<(), OfficeError> {
    let mut path = None;
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
            value if path.is_none() => path = Some(value.to_string()),
            _ => return Err(OfficeError::usage("search takes exactly one document path")),
        }
        index += 1;
    }
    let path = path.ok_or_else(|| OfficeError::usage("a document path is required"))?;
    let text = text.ok_or_else(|| OfficeError::usage("`--text <query>` is required"))?;
    print_json(&office.search(Path::new(&path), &text, limit)?)
}

/// Parses `render` options and hands one request to the library.
fn render(office: &Office, arguments: &[String]) -> Result<(), OfficeError> {
    let mut path = None;
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
            value if path.is_none() => path = Some(value.to_string()),
            _ => return Err(OfficeError::usage("render takes exactly one document path")),
        }
        index += 1;
    }
    let path = path.ok_or_else(|| OfficeError::usage("a document path is required"))?;
    let options = RenderOptions { sheet, page, out, out_dir, overwrite };
    print_json(&office.render(Path::new(&path), &options)?)
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

fn positional(arguments: &[String], message: &str) -> Result<String, OfficeError> {
    arguments.iter().find(|argument| !argument.starts_with('-')).cloned().ok_or_else(|| OfficeError::usage(message))
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

fn usage() {
    println!(
        "open-office {VERSION}\n\
         One inspector and converter for docx/rtf/md/html/txt, xlsx/csv,\n\
         pptx and PDF files.\n\
         \n\
         USAGE:\n\
         \x20 open-office probe <file>\n\
         \x20 open-office inspect <file>\n\
         \x20 open-office stats <file>...\n\
         \x20 open-office outline <file> [--points]\n\
         \x20 open-office search <file> --text <query> [--limit <n>]\n\
         \x20 open-office render <file> [--sheet <name>] [--page <n>] [--out <file> | --out-dir <dir>] [--overwrite]\n\
         \x20 open-office formats\n\
         \x20 open-office convert <file> --format <format> --out <file> [--overwrite] [--pages <ranges>]\n\
         \x20 open-office convert <file>... --format <format> --out-dir <dir> [--overwrite]\n\
         \x20 open-office capabilities\n\
         \x20 open-office version\n\
         \n\
         EXAMPLES:\n\
         \x20 open-office probe download.bin\n\
         \x20 open-office inspect report.docx\n\
         \x20 open-office stats report.docx\n\
         \x20 open-office stats a.docx b.pdf c.xlsx\n\
         \x20 open-office outline report.docx\n\
         \x20 open-office outline deck.pptx --points\n\
         \x20 open-office search report.docx --text invoice\n\
         \x20 open-office render budget.xlsx --out sheet.svg\n\
         \x20 open-office render deck.pptx --page 2 --out slide-2.svg\n\
         \x20 open-office render deck.pptx --out-dir preview/\n\
         \x20 open-office formats\n\
         \x20 open-office convert budget.xlsx --format pdf --out budget.pdf\n\
         \x20 open-office convert deck.pptx --format pdf --out deck.pdf\n\
         \x20 open-office convert notes.pdf --format md --out notes.md\n\
         \x20 open-office convert paper.pdf --format md --out paper.md --pages 1-3,5\n\
         \x20 open-office convert a.docx b.xlsx --format pdf --out-dir out"
    );
}
