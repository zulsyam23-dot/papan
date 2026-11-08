//! PAPAN command line interface.
//!
//! The CLI is a thin, deterministic wrapper around the public API. All commands
//! accept `--json` for machine-readable output and use stable exit codes:
//!
//! * `0` success
//! * `1` design or validation errors
//! * `2` usage errors
//! * `3` I/O, import or internal errors
//!
//! Human-readable diagnostics always include the stable diagnostic code.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use papan_api::{ApiError, ExportFormat, PapanProject};
use papan_backend::BackendInput;
use papan_types::Diagnostics;

const CLI_VERSION: &str = env!("CARGO_PKG_VERSION");
const HELP: &str = "\
PAPAN PCB toolchain

USAGE:
    papan <COMMAND> [OPTIONS] [PATH]

COMMANDS:
    new <name>                  Create a project directory
    check <path>                Compile and verify a project
    build <path>                Build manufacturing or source outputs
    export <path>               Export a project in a selected format
    import <file>               Import an external board file
    validate <path>             Run ERC, DRC and manufacturing validation
    inspect <path>              Print a design summary
    clean <path>                Remove output and cache directories

OPTIONS:
    -o, --output <dir>          Output directory
    -t, --target <name>         Build target (repeatable or comma separated)
    -f, --format <name>         Export/import format
        --profile <name>        Build profile: release (default) or debug
        --json                  Machine-readable JSON output
        --force                 Overwrite or build despite validation errors
    -v, --verbose               Verbose human output
    -q, --quiet                 Suppress non-error human output
    -h, --help                  Show this help
    -V, --version               Show the version

FORMATS:
    source, graph, document-json, ir-json, gerber, drill, bom,
    pick-and-place, manufacturing, kicad

BUILD TARGETS:
    manufacturing (default), gerber, drill, bom, pick-and-place,
    kicad, source, graph, all
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(args) {
        Ok(code) => code,
        Err(error) => {
            error.report();
            ExitCode::from(error.exit)
        }
    }
}

fn run(args: Vec<String>) -> Result<ExitCode, CliError> {
    let parsed = Options::parse(&args)?;
    if parsed.version {
        println!("papan {CLI_VERSION} (api {})", papan_api::API_VERSION);
        return Ok(ExitCode::SUCCESS);
    }
    if parsed.help || parsed.command.is_none() {
        print!("{HELP}");
        return Ok(ExitCode::SUCCESS);
    }

    let command = parsed.command.clone().unwrap();
    let path = parsed.positional(0);
    let mut reporter = Reporter {
        json: parsed.json,
        verbose: parsed.verbose,
        quiet: parsed.quiet,
        command: command.clone(),
    };

    let result = match command.as_str() {
        "new" => cmd_new(&parsed, &mut reporter),
        "check" => cmd_check(&parsed, path, &mut reporter),
        "build" => cmd_build(&parsed, path, &mut reporter),
        "export" => cmd_export(&parsed, path, &mut reporter),
        "import" => cmd_import(&parsed, path, &mut reporter),
        "validate" => cmd_validate(&parsed, path, &mut reporter),
        "inspect" => cmd_inspect(&parsed, path, &mut reporter),
        "clean" => cmd_clean(&parsed, path, &mut reporter),
        other => Err(CliError::usage(format!("unknown command '{other}'"))),
    };
    result.map_err(|error| error.with_command(&command))
}

// ---------------------------------------------------------------------------
// Option parsing
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
struct Options {
    command: Option<String>,
    positionals: Vec<String>,
    output: Option<PathBuf>,
    targets: Vec<String>,
    format: Option<String>,
    profile: String,
    json: bool,
    verbose: bool,
    quiet: bool,
    force: bool,
    help: bool,
    version: bool,
}

impl Options {
    fn parse(args: &[String]) -> Result<Self, CliError> {
        let mut options = Options {
            profile: "release".to_string(),
            ..Options::default()
        };
        let mut index = 0usize;
        while index < args.len() {
            let argument = &args[index];
            if let Some(value) = argument.strip_prefix("--") {
                let (name, inline) = match value.split_once('=') {
                    Some((name, value)) => (name, Some(value.to_string())),
                    None => (value, None),
                };
                match name {
                    "json" => options.json = true,
                    "verbose" => options.verbose = true,
                    "quiet" => options.quiet = true,
                    "force" => options.force = true,
                    "help" => options.help = true,
                    "version" => options.version = true,
                    "output" => {
                        let value = take_value(args, &mut index, inline, "--output")?;
                        options.output = Some(PathBuf::from(value));
                    }
                    "target" => {
                        let value = take_value(args, &mut index, inline, "--target")?;
                        for target in value.split(',') {
                            let target = target.trim();
                            if !target.is_empty() {
                                options.targets.push(target.to_string());
                            }
                        }
                    }
                    "format" => {
                        options.format =
                            Some(take_value(args, &mut index, inline, "--format")?);
                    }
                    "profile" => {
                        let value = take_value(args, &mut index, inline, "--profile")?;
                        if value != "release" && value != "debug" {
                            return Err(CliError::usage(format!(
                                "unknown build profile '{value}' (expected release or debug)"
                            )));
                        }
                        options.profile = value;
                    }
                    other => {
                        return Err(CliError::usage(format!("unknown option '--{other}'")));
                    }
                }
            } else if argument == "-h" {
                options.help = true;
            } else if argument == "-V" {
                options.version = true;
            } else if argument == "-v" {
                options.verbose = true;
            } else if argument == "-q" {
                options.quiet = true;
            } else if argument == "-o" {
                let value = take_value(args, &mut index, None, "-o")?;
                options.output = Some(PathBuf::from(value));
            } else if argument == "-t" {
                let value = take_value(args, &mut index, None, "-t")?;
                for target in value.split(',') {
                    options.targets.push(target.trim().to_string());
                }
            } else if argument == "-f" {
                options.format = Some(take_value(args, &mut index, None, "-f")?);
            } else if argument.starts_with('-') {
                return Err(CliError::usage(format!("unknown option '{argument}'")));
            } else if options.command.is_none() {
                options.command = Some(argument.clone());
            } else {
                options.positionals.push(argument.clone());
            }
            index += 1;
        }
        Ok(options)
    }

    fn positional(&self, index: usize) -> Option<&str> {
        self.positionals.get(index).map(String::as_str)
    }

    fn required_path(&self) -> Result<&str, CliError> {
        self.positional(0)
            .ok_or_else(|| CliError::usage("a project path is required"))
    }
}

fn take_value(
    args: &[String],
    index: &mut usize,
    inline: Option<String>,
    option: &str,
) -> Result<String, CliError> {
    if let Some(value) = inline {
        if value.is_empty() {
            return Err(CliError::usage(format!("{option} requires a value")));
        }
        return Ok(value);
    }
    *index += 1;
    args.get(*index)
        .cloned()
        .ok_or_else(|| CliError::usage(format!("{option} requires a value")))
}

// ---------------------------------------------------------------------------
// Errors and reporting
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct CliError {
    code: String,
    message: String,
    diagnostics: Diagnostics,
    exit: u8,
    command: Option<String>,
}

impl CliError {
    fn usage(message: impl Into<String>) -> Self {
        Self {
            code: "CLI_USAGE".to_string(),
            message: message.into(),
            diagnostics: Diagnostics::new(),
            exit: 2,
            command: None,
        }
    }

    fn io(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            diagnostics: Diagnostics::new(),
            exit: 3,
            command: None,
        }
    }

    fn design(
        code: impl Into<String>,
        message: impl Into<String>,
        diagnostics: Diagnostics,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            diagnostics,
            exit: 1,
            command: None,
        }
    }

    fn with_command(mut self, command: &str) -> Self {
        self.command = Some(command.to_string());
        self
    }

    fn report(&self) {
        let json = std::env::args().any(|argument| argument == "--json");
        if json {
            let value = serde_json::json!({
                "ok": false,
                "command": self.command,
                "error": {
                    "code": self.code,
                    "message": self.message,
                    "diagnostics": self.diagnostics,
                }
            });
            println!("{}", serde_json::to_string_pretty(&value).unwrap_or_default());
        } else {
            eprintln!("error {}: {}", self.code, self.message);
            print_diagnostics(&self.diagnostics, true);
        }
    }
}

impl From<ApiError> for CliError {
    fn from(error: ApiError) -> Self {
        let exit = if matches!(
            error.code.as_str(),
            "API_IO" | "DOC_IO" | "DOC_MANIFEST" | "API_IMPORT"
        ) {
            3
        } else if error.code == "API_TRANSACTION" || error.code == "API_COMPILE" {
            1
        } else {
            1
        };
        Self {
            code: error.code,
            message: error.message,
            diagnostics: error.diagnostics,
            exit,
            command: None,
        }
    }
}

impl From<std::io::Error> for CliError {
    fn from(error: std::io::Error) -> Self {
        Self::io("CLI_IO", error.to_string())
    }
}

struct Reporter {
    json: bool,
    verbose: bool,
    quiet: bool,
    command: String,
}

impl Reporter {
    fn note(&self, message: &str) {
        if !self.json && !self.quiet {
            println!("{message}");
        }
    }

    fn success(&self, path: Option<&Path>, diagnostics: &Diagnostics, data: serde_json::Value) {
        if self.json {
            let value = serde_json::json!({
                "ok": true,
                "command": self.command,
                "api_version": papan_api::API_VERSION,
                "path": path.map(|path| path.display().to_string()),
                "production_ready": !diagnostics.has_errors(),
                "counts": counts(diagnostics),
                "diagnostics": diagnostics,
                "data": data,
            });
            println!("{}", serde_json::to_string_pretty(&value).unwrap_or_default());
        } else if !self.quiet {
            print_diagnostics(diagnostics, self.verbose);
            print_summary(diagnostics);
        }
    }
}

fn counts(diagnostics: &Diagnostics) -> serde_json::Value {
    serde_json::json!({
        "errors": diagnostics.errors(),
        "warnings": diagnostics.warnings(),
        "infos": diagnostics.infos(),
        "waived": diagnostics.waived(),
    })
}

fn print_diagnostics(diagnostics: &Diagnostics, verbose: bool) {
    for diagnostic in diagnostics {
        let location = diagnostic
            .location
            .map(|point| {
                format!(
                    " at ({:.3}mm, {:.3}mm)",
                    point.x.as_mm(),
                    point.y.as_mm()
                )
            })
            .unwrap_or_default();
        let object = diagnostic
            .object
            .as_ref()
            .map(|object| format!(" [{object}]"))
            .unwrap_or_default();
        eprintln!(
            "{:<7} {}: {}{}{}",
            diagnostic.severity.label(),
            diagnostic.code,
            diagnostic.message,
            object,
            location
        );
        if verbose {
            if let Some(span) = diagnostic.span {
                eprintln!("        at bytes {}..{}", span.start, span.end);
            }
            if let Some(suggestion) = &diagnostic.suggestion {
                eprintln!("        suggestion: {suggestion}");
            }
        }
    }
}

fn print_summary(diagnostics: &Diagnostics) {
    if !diagnostics.is_empty() {
        println!(
            "{} error(s), {} warning(s), {} info, {} waived",
            diagnostics.errors(),
            diagnostics.warnings(),
            diagnostics.infos(),
            diagnostics.waived()
        );
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

fn cmd_new(options: &Options, reporter: &mut Reporter) -> Result<ExitCode, CliError> {
    let name = options
        .positional(0)
        .ok_or_else(|| CliError::usage("a project name is required"))?;
    let requested = PathBuf::from(name);
    let project_dir = if requested
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.eq_ignore_ascii_case("papan"))
        .unwrap_or(false)
    {
        requested.clone()
    } else {
        PathBuf::from(format!("{name}.papan"))
    };
    let title = project_dir
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(name);
    if project_dir.join("project.papan").exists() && !options.force {
        return Err(CliError::usage(format!(
            "'{}' already exists (use --force to overwrite)",
            project_dir.display()
        )));
    }
    let project = PapanProject::create(&project_dir, title)?;
    let source = project
        .document()
        .source
        .lines()
        .next()
        .unwrap_or("")
        .to_string();
    reporter.note(&format!(
        "created {} (board \"{}\")",
        project_dir.display(),
        title
    ));
    reporter.success(
        Some(&project_dir),
        project.diagnostics(),
        serde_json::json!({
            "project": project_dir.display().to_string(),
            "name": title,
            "board": source,
            "profile": options.profile,
        }),
    );
    Ok(ExitCode::SUCCESS)
}

fn cmd_check(options: &Options, path: Option<&str>, reporter: &mut Reporter) -> Result<ExitCode, CliError> {
    let path = match path {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(options.required_path()?),
    };
    let project = PapanProject::open(&path)?;
    let report = project.document().structural_validation();
    reporter.note(&format!("checked {}", path.display()));
    if options.json {
        reporter.success(Some(&path), &report.diagnostics, summary_data(&project));
    } else {
        print_diagnostics(&report.diagnostics, options.verbose);
        print_summary(&report.diagnostics);
        if options.verbose {
            println!(
                "{} component(s), {} net(s), {} trace(s), {} via(s)",
                project.ir().components.len(),
                project.ir().nets.len(),
                project.ir().traces.len(),
                project.ir().vias.len()
            );
        }
    }
    Ok(if report.has_errors() {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

fn cmd_validate(
    options: &Options,
    path: Option<&str>,
    reporter: &mut Reporter,
) -> Result<ExitCode, CliError> {
    let path = match path {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(options.required_path()?),
    };
    let project = PapanProject::open(&path)?;
    let report = project.validate();
    reporter.note(&format!("validated {}", path.display()));
    if options.json {
        reporter.success(Some(&path), &report.diagnostics, summary_data(&project));
    } else {
        print_diagnostics(&report.diagnostics, options.verbose);
        print_summary(&report.diagnostics);
        println!(
            "{}",
            if report.production_ready {
                "production ready"
            } else {
                "not production ready"
            }
        );
    }
    Ok(if report.production_ready {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

fn cmd_build(
    options: &Options,
    path: Option<&str>,
    reporter: &mut Reporter,
) -> Result<ExitCode, CliError> {
    let path = match path {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(options.required_path()?),
    };
    let mut project = PapanProject::open(&path)?;
    if project.has_errors() {
        return Err(CliError::design(
            "BUILD_COMPILE",
            format!("'{}' does not compile", path.display()),
            project.diagnostics().clone(),
        ));
    }

    let targets = parse_build_targets(&options.targets)?;
    let physical = targets.iter().any(|target| target.is_physical());
    if physical && project.ir().traces.is_empty() && !project.ir().nets.is_empty() {
        reporter.note("autorouting unrouted nets");
        let route = project.board().autoroute()?;
        if options.verbose {
            for diagnostic in &route.diagnostics {
                eprintln!("{} {}: {}", diagnostic.severity.label(), diagnostic.code, diagnostic.message);
            }
        }
    }

    let report = project.validate();
    if physical && report.has_errors() && !options.force {
        return Err(CliError::design(
            "BUILD_VALIDATION",
            "design has validation errors; no manufacturing files were written (use --force to override)",
            report.diagnostics,
        ));
    }

    let output_root = options.output.clone().unwrap_or_else(|| {
        let root = project_root(&path);
        if options.profile == "debug" {
            root.join("output").join("debug")
        } else {
            root.join("output")
        }
    });
    std::fs::create_dir_all(&output_root)?;

    let mut written: BTreeMap<String, String> = BTreeMap::new();
    for target in &targets {
        let formats = target.formats();
        for format in formats {
            let bundle = project.export(format)?;
            for file in bundle.files {
                let destination = output_root.join(&file.name);
                if let Some(parent) = destination.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&destination, file.content)?;
                written.insert(file.name, destination.display().to_string());
            }
        }
    }

    let files: Vec<serde_json::Value> = written
        .iter()
        .map(|(name, path)| serde_json::json!({ "name": name, "path": path }))
        .collect();
    reporter.note(&format!(
        "built {} file(s) in {} (profile {})",
        written.len(),
        output_root.display(),
        options.profile
    ));
    reporter.success(
        Some(&output_root),
        &report.diagnostics,
        serde_json::json!({
            "output": output_root.display().to_string(),
            "profile": options.profile,
            "targets": targets.iter().map(|target| target.label()).collect::<Vec<_>>(),
            "files": files,
            "production_ready": report.production_ready,
        }),
    );
    Ok(if report.has_errors() {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

fn cmd_export(
    options: &Options,
    path: Option<&str>,
    reporter: &mut Reporter,
) -> Result<ExitCode, CliError> {
    let path = match path {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(options.required_path()?),
    };
    let format_name = options.format.as_deref().unwrap_or("manufacturing");
    let format = ExportFormat::parse(format_name).ok_or_else(|| {
        CliError::usage(format!(
            "unknown export format '{format_name}' (see `papan --help`)"
        ))
    })?;
    let project = PapanProject::open(&path)?;
    let bundle = project.export(format)?;
    let output_root = options
        .output
        .clone()
        .unwrap_or_else(|| project_root(&path).join("output"));
    std::fs::create_dir_all(&output_root)?;
    let mut files = Vec::new();
    for file in &bundle.files {
        let destination = output_root.join(&file.name);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&destination, &file.content)?;
        files.push(serde_json::json!({
            "name": file.name,
            "path": destination.display().to_string(),
            "bytes": file.content.len(),
        }));
    }
    reporter.note(&format!(
        "exported {} file(s) as {} to {}",
        files.len(),
        format.label(),
        output_root.display()
    ));
    reporter.success(
        Some(&output_root),
        &bundle.diagnostics,
        serde_json::json!({
            "format": format.label(),
            "output": output_root.display().to_string(),
            "files": files,
        }),
    );
    Ok(ExitCode::SUCCESS)
}

fn cmd_import(
    options: &Options,
    path: Option<&str>,
    reporter: &mut Reporter,
) -> Result<ExitCode, CliError> {
    let path = match path {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(options.required_path()?),
    };
    let adapter = options
        .format
        .clone()
        .unwrap_or_else(|| infer_import_format(&path));
    let input = BackendInput::from_path(&path)?;
    let mut project = PapanProject::new();
    project.import(&adapter, input)?;
    if project.has_errors() {
        return Err(CliError::design(
            "IMPORT_VALIDATION",
            "imported design contains errors",
            project.diagnostics().clone(),
        ));
    }

    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("imported");
    let output = match &options.output {
        Some(output) if output.is_dir() => output.join(format!("{stem}.pn")),
        Some(output) if output.extension().is_some() => output.clone(),
        Some(output) => output.join(format!("{stem}.pn")),
        None => path.with_file_name(format!("{stem}.pn")),
    };
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    project.save_source(&output)?;
    let graph_path = output.with_extension("graph");
    project.save_graph(&graph_path)?;

    reporter.note(&format!(
        "imported {} as {} ({})",
        path.display(),
        adapter,
        output.display()
    ));
    reporter.success(
        Some(&output),
        project.diagnostics(),
        serde_json::json!({
            "adapter": adapter,
            "source": output.display().to_string(),
            "graph": graph_path.display().to_string(),
            "components": project.ir().components.len(),
            "nets": project.ir().nets.len(),
            "traces": project.ir().traces.len(),
            "vias": project.ir().vias.len(),
        }),
    );
    Ok(ExitCode::SUCCESS)
}

fn cmd_inspect(
    options: &Options,
    path: Option<&str>,
    reporter: &mut Reporter,
) -> Result<ExitCode, CliError> {
    let path = match path {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(options.required_path()?),
    };
    let project = PapanProject::open(&path)?;
    let report = project.document().structural_validation();
    if options.json {
        reporter.success(Some(&path), &report.diagnostics, inspect_data(&project));
    } else {
        if !options.quiet {
            print_inspection(&project);
        }
        print_diagnostics(&report.diagnostics, options.verbose);
        print_summary(&report.diagnostics);
    }
    Ok(if report.has_errors() {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

fn cmd_clean(
    options: &Options,
    path: Option<&str>,
    reporter: &mut Reporter,
) -> Result<ExitCode, CliError> {
    let path = match path {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(options.required_path()?),
    };
    let root = project_root(&path);
    if !root.exists() {
        return Err(CliError::io(
            "CLEAN_IO",
            format!("'{}' does not exist", root.display()),
        ));
    }
    let mut removed = Vec::new();
    for name in ["output", "cache"] {
        let directory = root.join(name);
        if directory.exists() {
            std::fs::remove_dir_all(&directory)?;
            removed.push(directory.display().to_string());
        }
    }
    reporter.note(&format!(
        "cleaned {} director{} under {}",
        removed.len(),
        if removed.len() == 1 { "y" } else { "ies" },
        root.display()
    ));
    reporter.success(
        Some(&root),
        &Diagnostics::new(),
        serde_json::json!({ "removed": removed }),
    );
    Ok(ExitCode::SUCCESS)
}

// ---------------------------------------------------------------------------
// Build targets
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BuildTarget {
    Manufacturing,
    Gerber,
    Drill,
    Bom,
    PickAndPlace,
    KiCad,
    Source,
    Graph,
    All,
}

impl BuildTarget {
    fn parse(value: &str) -> Result<Self, CliError> {
        Ok(match value.to_ascii_lowercase().as_str() {
            "manufacturing" | "mfg" | "fab" => Self::Manufacturing,
            "gerber" | "gbr" => Self::Gerber,
            "drill" | "excellon" => Self::Drill,
            "bom" => Self::Bom,
            "pick-and-place" | "pick_and_place" | "pnp" | "pos" => Self::PickAndPlace,
            "kicad" => Self::KiCad,
            "source" | "pn" => Self::Source,
            "graph" | "nodes" => Self::Graph,
            "all" => Self::All,
            other => {
                return Err(CliError::usage(format!(
                    "unknown build target '{other}' (see `papan --help`)"
                )))
            }
        })
    }

    fn formats(self) -> Vec<ExportFormat> {
        match self {
            Self::Manufacturing => vec![ExportFormat::Manufacturing],
            Self::Gerber => vec![ExportFormat::Gerber],
            Self::Drill => vec![ExportFormat::Drill],
            Self::Bom => vec![ExportFormat::Bom],
            Self::PickAndPlace => vec![ExportFormat::PickAndPlace],
            Self::KiCad => vec![ExportFormat::KiCad],
            Self::Source => vec![ExportFormat::Source],
            Self::Graph => vec![ExportFormat::NodeGraph],
            Self::All => vec![
                ExportFormat::Manufacturing,
                ExportFormat::Source,
                ExportFormat::NodeGraph,
                ExportFormat::KiCad,
            ],
        }
    }

    fn is_physical(self) -> bool {
        matches!(
            self,
            Self::Manufacturing
                | Self::Gerber
                | Self::Drill
                | Self::Bom
                | Self::PickAndPlace
                | Self::All
        )
    }

    fn label(self) -> &'static str {
        match self {
            Self::Manufacturing => "manufacturing",
            Self::Gerber => "gerber",
            Self::Drill => "drill",
            Self::Bom => "bom",
            Self::PickAndPlace => "pick-and-place",
            Self::KiCad => "kicad",
            Self::Source => "source",
            Self::Graph => "graph",
            Self::All => "all",
        }
    }
}

fn parse_build_targets(targets: &[String]) -> Result<Vec<BuildTarget>, CliError> {
    if targets.is_empty() {
        return Ok(vec![BuildTarget::Manufacturing]);
    }
    let mut parsed = Vec::new();
    for target in targets {
        let target = BuildTarget::parse(target)?;
        if !parsed.contains(&target) {
            parsed.push(target);
        }
    }
    Ok(parsed)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn infer_import_format(path: &Path) -> String {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "kicad_pcb" | "kicad" | "pcbnew" => "kicad".to_string(),
        _ => "kicad".to_string(),
    }
}

fn project_root(path: &Path) -> PathBuf {
    if path.is_dir() {
        return path.to_path_buf();
    }
    path.parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn summary_data(project: &PapanProject) -> serde_json::Value {
    let ir = project.ir();
    serde_json::json!({
        "name": project.name(),
        "board": ir.board.name,
        "width_mm": ir.board.width.as_mm(),
        "height_mm": ir.board.height.as_mm(),
        "thickness_mm": ir.board.thickness.as_mm(),
        "layers": ir.board.layer_count,
        "components": ir.components.len(),
        "nets": ir.nets.len(),
        "connections": ir.connections.len(),
        "traces": ir.traces.len(),
        "vias": ir.vias.len(),
    })
}

fn inspect_data(project: &PapanProject) -> serde_json::Value {
    let ir = project.ir();
    let components: Vec<serde_json::Value> = ir
        .components
        .iter()
        .map(|component| {
            let part = ir
                .definition_for(component)
                .map(|definition| definition.name.clone())
                .unwrap_or_else(|| "generic".to_string());
            serde_json::json!({
                "id": component.id,
                "designator": component.designator,
                "part": part,
                "value": component.value.map(|value| value.to_source()),
                "x_mm": component.position.map(|position| position.x.as_mm()),
                "y_mm": component.position.map(|position| position.y.as_mm()),
                "rotation_deg": component.rotation.as_deg(),
                "side": component.side,
                "locked": component.locked,
            })
        })
        .collect();
    let nets: Vec<serde_json::Value> = ir
        .nets
        .iter()
        .map(|net| {
            let members: Vec<String> = net
                .members
                .iter()
                .map(|member| {
                    let designator = ir
                        .find_component_by_id(&member.component)
                        .map(|component| component.designator.as_str())
                        .unwrap_or_else(|| member.component.as_str());
                    format!("{designator}.{}", member.pin)
                })
                .collect();
            serde_json::json!({
                "id": net.id,
                "name": net.name,
                "kind": format!("{:?}", net.kind).to_ascii_lowercase(),
                "voltage": net.voltage.map(|voltage| voltage.to_source()),
                "members": members,
            })
        })
        .collect();
    serde_json::json!({
        "name": project.name(),
        "version": project.version(),
        "board": {
            "id": ir.board.id,
            "name": ir.board.name,
            "width_mm": ir.board.width.as_mm(),
            "height_mm": ir.board.height.as_mm(),
            "thickness_mm": ir.board.thickness.as_mm(),
            "layers": ir.board.layer_count,
            "outline_area_mm2": ir.board.outline.area_mm2(),
        },
        "layers": ir.stackup.layers.iter().map(|layer| {
            serde_json::json!({
                "id": layer.id,
                "name": layer.name,
                "kind": format!("{:?}", layer.kind).to_ascii_lowercase(),
                "index": layer.index,
            })
        }).collect::<Vec<_>>(),
        "components": components,
        "nets": nets,
        "connections": ir.connections.len(),
        "traces": ir.traces.len(),
        "vias": ir.vias.len(),
        "commands": project.commands(),
        "constraints": {
            "min_trace_width_mm": ir.constraints.min_trace_width.as_mm(),
            "clearance_mm": ir.constraints.clearance.as_mm(),
            "min_via_drill_mm": ir.constraints.min_via_drill.as_mm(),
            "min_via_diameter_mm": ir.constraints.min_via_diameter.as_mm(),
            "copper_edge_clearance_mm": ir.constraints.copper_edge_clearance.as_mm(),
            "min_annular_ring_mm": ir.constraints.min_annular_ring.as_mm(),
            "solder_mask_clearance_mm": ir.constraints.solder_mask_clearance.as_mm(),
            "drill_to_drill_mm": ir.constraints.drill_to_drill.as_mm(),
        },
    })
}

fn print_inspection(project: &PapanProject) {
    let ir = project.ir();
    println!("project: {}", project.name());
    println!("version: {}", project.version());
    println!(
        "board: {} ({} x {} mm, {} copper layer(s), {} mm thick)",
        ir.board.name,
        ir.board.width.as_mm(),
        ir.board.height.as_mm(),
        ir.board.layer_count,
        ir.board.thickness.as_mm()
    );
    println!(
        "objects: {} component(s), {} net(s), {} connection(s), {} trace(s), {} via(s)",
        ir.components.len(),
        ir.nets.len(),
        ir.connections.len(),
        ir.traces.len(),
        ir.vias.len()
    );
    println!("components:");
    for component in &ir.components {
        let part = ir
            .definition_for(component)
            .map(|definition| definition.name.as_str())
            .unwrap_or("generic");
        let position = component
            .position
            .map(|position| {
                format!(
                    "({:.3}mm, {:.3}mm)",
                    position.x.as_mm(),
                    position.y.as_mm()
                )
            })
            .unwrap_or_else(|| "unplaced".to_string());
        let value = component
            .value
            .map(|value| format!(" value={}", value.to_source()))
            .unwrap_or_default();
        println!(
            "  {:<8} {:<20} {:<24} rot={}deg side={:?}{}",
            component.designator,
            part,
            position,
            component.rotation.as_deg(),
            component.side,
            value
        );
    }
    println!("nets:");
    for net in &ir.nets {
        let members: Vec<String> = net
            .members
            .iter()
            .map(|member| {
                let designator = ir
                    .find_component_by_id(&member.component)
                    .map(|component| component.designator.as_str())
                    .unwrap_or_else(|| member.component.as_str());
                format!("{designator}.{}", member.pin)
            })
            .collect();
        println!(
            "  {:<16} {:?}{} members={}",
            net.name,
            net.kind,
            net.voltage
                .map(|voltage| format!(" {}", voltage.to_source()))
                .unwrap_or_default(),
            if members.is_empty() {
                "-".to_string()
            } else {
                members.join(", ")
            }
        );
    }
    println!(
        "constraints: width >= {} mm, clearance >= {} mm",
        ir.constraints.min_trace_width.as_mm(),
        ir.constraints.clearance.as_mm()
    );
}
