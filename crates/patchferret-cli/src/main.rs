//! PatchFerret command line tool.
//!
//! The local option: same core as the browser build, no upload, no network.
//! Argument parsing is hand-rolled to keep the dependency tree at zero beyond
//! the workspace crates — this binary should build anywhere Rust does.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use patchferret_formats::{adapters, detect, ShowFile, ShowInput};
use patchferret_model::xml;
use patchferret_report::build_all;

const USAGE: &str = "\
PatchFerret — console show file documentation

USAGE:
    patchferret info    <show>            Identify the format and summarise
    patchferret xml     <show> [-o FILE]  Convert to PFX interchange XML
    patchferret report  <show> [-o DIR]   Write patch list, spec and topology PDFs
    patchferret formats                   List supported formats

ARGS:
    <show>    A show file, or a directory holding one

OPTIONS:
    -o, --out    Output file (xml) or directory (report). Default: alongside input
    -h, --help   Show this message
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("patchferret: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let Some(cmd) = args.first() else {
        print!("{USAGE}");
        return Ok(());
    };

    if cmd == "-h" || cmd == "--help" || cmd == "help" {
        print!("{USAGE}");
        return Ok(());
    }

    if cmd == "formats" {
        println!("{:<8}  {:<40}  EXTENSIONS", "ID", "FORMAT");
        for a in adapters() {
            println!("{:<8}  {:<40}  {}", a.id(), a.display_name(), a.extensions().join(", "));
        }
        return Ok(());
    }

    let path = args.get(1).ok_or_else(|| format!("'{cmd}' needs a show file\n\n{USAGE}"))?;
    let out = flag(args, "-o", "--out");

    let input = load(Path::new(path))?;
    let (adapter, confidence) =
        detect(&input).ok_or_else(|| format!("no adapter recognised {path}"))?;
    let show = adapter.parse(&input).map_err(|e| e.to_string())?;

    match cmd.as_str() {
        "info" => {
            println!("File       {path}");
            println!("Format     {} ({:?} confidence)", adapter.display_name(), confidence);
            println!("Show       {}", show.meta.name);
            println!("Console    {}", show.meta.console);
            if let Some(v) = &show.meta.format_version {
                println!("Version    {v}");
            }
            println!();
            println!("Strips     {}", show.strips.len());
            println!("Devices    {}", show.devices.len());
            println!(
                "Input      {} slots, {} with a connector, {} reaching a channel",
                show.patch.inputs.len(),
                show.patch.inputs.iter().filter(|p| p.socket.is_some()).count(),
                show.patch.inputs.iter().filter(|p| p.strip.is_some()).count()
            );
            println!("Outputs    {}", show.patch.outputs.len());
            println!("Head amps  {}", show.head_amps.len());

            if show.diagnostics.is_empty() {
                println!("\nFidelity   everything recognised was modelled");
            } else {
                println!(
                    "\nFidelity   {} item(s) would not survive conversion:",
                    show.diagnostics.len()
                );
                for d in &show.diagnostics {
                    println!("  [{}] {}", d.severity.as_str(), d.locus);
                    println!("      {}", d.message);
                }
            }
        }

        "xml" => {
            let text = xml::to_xml(&show).map_err(|e| e.to_string())?;
            match out {
                Some(o) => {
                    std::fs::write(&o, text).map_err(|e| format!("writing {o}: {e}"))?;
                    eprintln!("wrote {o}");
                }
                None => println!("{text}"),
            }
        }

        "report" => {
            let dir = out.map(PathBuf::from).unwrap_or_else(|| {
                Path::new(path).parent().unwrap_or(Path::new(".")).to_path_buf()
            });
            std::fs::create_dir_all(&dir)
                .map_err(|e| format!("creating {}: {e}", dir.display()))?;
            for r in build_all(&show) {
                let target = dir.join(&r.file_name);
                std::fs::write(&target, &r.bytes)
                    .map_err(|e| format!("writing {}: {e}", target.display()))?;
                println!("{:<28} {}", r.title, target.display());
            }
        }

        other => return Err(format!("unknown command '{other}'\n\n{USAGE}")),
    }

    Ok(())
}

fn flag(args: &[String], short: &str, long: &str) -> Option<String> {
    args.iter().position(|a| a == short || a == long).and_then(|i| args.get(i + 1)).cloned()
}

/// Read a show file, or every file in a directory for folder-based formats.
fn load(path: &Path) -> Result<ShowInput, String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());

    if path.is_dir() {
        let mut files = Vec::new();
        let entries =
            std::fs::read_dir(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() {
                let bytes =
                    std::fs::read(&p).map_err(|e| format!("reading {}: {e}", p.display()))?;
                let rel = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
                files.push(ShowFile::new(rel, bytes));
            }
        }
        if files.is_empty() {
            return Err(format!("{} is empty", path.display()));
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(ShowInput::bundle(name, files))
    } else {
        let bytes =
            std::fs::read(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
        Ok(ShowInput::single(name, bytes))
    }
}
