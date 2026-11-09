//! Development tasks of the workspace, run as `cargo xtask <task>` (alias in
//! `.cargo/config.toml`).
//!
//! `cargo xtask check` is the one command of the checks: `scripts\check.bat`
//! runs it on the developer's computer and the CI runs it on Windows and
//! Ubuntu, so the two cannot drift apart. It runs format, the build and the
//! tests in release and in debug, Clippy with warnings as errors and
//! cargo-deny, shows everything while it runs and writes `target/check.log`
//! (the whole output) and `target/check-summary.txt` (for each step its
//! status, time and the lines that matter).
//!
//!   cargo xtask check                     every step
//!   cargo xtask check --step test         one step: fmt, build, test, clippy, deny
//!   cargo xtask check --notes FILE        lines of FILE added to the environment
//!                                         section (what only the caller knows)
//!
//! Exit status: 0 all green, 1 to 255 the number of failed steps, 2 for a
//! wrong command line.

mod lines;
mod plan;

use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, LineWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

use plan::{Platform, Profile, Step, Task};

/// Lines of a step kept in the summary; the rest stays in the log.
const MAX_SUMMARY_LINES: usize = 80;

const USAGE: &str = "usage: cargo xtask check [--step fmt|build|test|clippy|deny] [--notes FILE]";

struct CheckArgs {
    only: Option<Step>,
    notes: Option<PathBuf>,
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<CheckArgs, String> {
    match args.next().as_deref() {
        Some("check") => {}
        Some(other) => return Err(format!("unknown task '{other}'")),
        None => return Err("no task given".to_owned()),
    }
    let mut parsed = CheckArgs { only: None, notes: None };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--step" => {
                let name = args.next().ok_or("--step needs a value")?;
                parsed.only =
                    Some(Step::parse(&name).ok_or_else(|| format!("unknown step '{name}' (steps: {})", Step::NAMES))?);
            }
            "--notes" => parsed.notes = Some(PathBuf::from(args.next().ok_or("--notes needs a file")?)),
            other => return Err(format!("unknown option '{other}'")),
        }
    }
    Ok(parsed)
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(why) => {
            eprintln!("xtask: {why}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match check(&args) {
        Ok(0) => ExitCode::SUCCESS,
        Ok(failures) => ExitCode::from(u8::try_from(failures).unwrap_or(u8::MAX)),
        Err(why) => {
            eprintln!("xtask: check: {why}");
            ExitCode::FAILURE
        }
    }
}

/// Where the checks run and write.
struct Paths {
    root: PathBuf,
    /// Cargo's target directory (`CARGO_TARGET_DIR` when set).
    target: PathBuf,
    /// Log and summary: always the repository's `target/`, where the scripts
    /// and the instructions look for them.
    reports: PathBuf,
}

impl Paths {
    fn find() -> io::Result<Self> {
        // set by cargo for the program it runs: the xtask crate's folder
        let manifest = std::env::var_os("CARGO_MANIFEST_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| io::Error::other("CARGO_MANIFEST_DIR is not set: run it as `cargo xtask`"))?;
        let root = manifest
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| io::Error::other("the xtask crate has no parent folder"))?;
        let target = match std::env::var_os("CARGO_TARGET_DIR") {
            Some(dir) => root.join(dir),
            None => root.join("target"),
        };
        let reports = root.join("target");
        Ok(Self { root, target, reports })
    }
}

/// Screen and `check.log` at once, plus the lines for the summary.
struct Report {
    log: LineWriter<File>,
    summary: Vec<String>,
}

impl Report {
    fn say(&mut self, text: &str) {
        println!("{text}");
        // a log that cannot be written must not stop the checks
        let _ = writeln!(self.log, "{text}");
    }

    fn note(&mut self, text: impl Into<String>) {
        self.summary.push(text.into());
    }
}

fn check(args: &CheckArgs) -> io::Result<usize> {
    let paths = Paths::find()?;
    fs::create_dir_all(&paths.reports)?;
    let mut report =
        Report { log: LineWriter::new(File::create(paths.reports.join("check.log"))?), summary: Vec::new() };
    let started = Instant::now();
    let platform = Platform::current();

    report.say("check started");
    report.note("check started");
    report.note("");
    report.note("== environment");
    let environment = environment(&paths, args.notes.as_deref());
    for line in &environment {
        report.say(line);
        report.note(line.clone());
    }

    let mut failures = 0;
    // profiles whose build succeeded: only their tests run
    let mut built: Vec<Profile> = Vec::new();
    for task in plan::plan(args.only, platform) {
        match task {
            Task::CopyDlls(profile) => {
                let lines = copy_dlls(&paths.target.join(profile.dir_name()));
                report.note("");
                report.note(format!("== {}", task.label()));
                for line in lines {
                    report.say(&line);
                    report.note(format!("   {line}"));
                }
            }
            Task::Test(profile) if !built.contains(&profile) => {
                report.note("");
                report.note(format!("== {} : skipped (the build failed)", task.label()));
            }
            _ => {
                let Some(cargo_args) = task.cargo_args(platform) else { continue };
                let ok = run_step(&mut report, &paths.root, &task.label(), &cargo_args);
                if !ok {
                    failures += 1;
                    if task == Task::Deny {
                        report.note("   (cargo-deny missing? install it: cargo install --locked cargo-deny)");
                    }
                }
                if let Task::Build(profile) = task {
                    if ok {
                        built.push(profile);
                    }
                }
            }
        }
    }

    let minutes = started.elapsed().as_secs_f64() / 60.0;
    let verdict = if failures == 0 { "ALL GREEN".to_owned() } else { format!("{failures} step(s) FAILED") };
    report.note("");
    report.note(format!("check finished: {verdict} in {minutes:.1} min"));
    let summary_path = paths.reports.join("check-summary.txt");
    let mut text = report.summary.join("\n");
    text.push('\n');
    fs::write(&summary_path, &text)?;

    println!();
    println!("================ summary ================");
    print!("{text}");
    println!();
    println!("Full output: {}", paths.reports.join("check.log").display());
    println!("Summary:     {}", summary_path.display());
    Ok(failures)
}

/// Runs `cargo <args>` in `root`, showing and logging its output; true when
/// it succeeded.
fn run_step(report: &mut Report, root: &Path, label: &str, args: &[&str]) -> bool {
    let shown = format!("cargo {}", args.join(" "));
    report.say("");
    report.say(&format!("===== {label} : {shown} ====="));
    let started = Instant::now();
    let mut picked = Vec::new();
    let mut warnings = 0usize;
    // the cargo that runs xtask, the toolchain of rust-toolchain.toml
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    // panics in the tests show where they happened
    command.args(args).current_dir(root).env("RUST_BACKTRACE", "1");
    let status = run_streaming(&mut command, |line| {
        report.say(line);
        if lines::is_warning(line) {
            warnings += 1;
        }
        if lines::is_important(line) {
            picked.push(line.to_owned());
        }
    });
    let secs = started.elapsed().as_secs();
    let (ok, status) = match status {
        Ok(s) if s.success() => (true, "ok".to_owned()),
        Ok(s) => (false, format!("FAILED (exit {})", s.code().map_or("none".to_owned(), |c| c.to_string()))),
        Err(e) => (false, format!("FAILED (could not start: {e})")),
    };
    report.say(&format!("----- {label} : {status} in {secs} s"));
    report.note("");
    report.note(format!("== {label} : {status} in {secs} s : {shown}"));
    if warnings > 0 {
        report.note(format!("   {warnings} warning(s)"));
    }
    for line in picked.iter().take(MAX_SUMMARY_LINES) {
        report.note(format!("   {line}"));
    }
    if picked.len() > MAX_SUMMARY_LINES {
        report.note(format!("   ... {} more lines in target/check.log", picked.len() - MAX_SUMMARY_LINES));
    }
    ok
}

/// Runs `command` and hands every line of its standard output and error to
/// `on_line` as it arrives; waits for the end of the command.
fn run_streaming(command: &mut Command, mut on_line: impl FnMut(&str)) -> io::Result<std::process::ExitStatus> {
    let mut child = command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let (sender, receiver) = mpsc::channel::<String>();
    let readers: Vec<_> = [
        child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>),
        child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    .map(|stream| {
        let sender = sender.clone();
        thread::spawn(move || forward_lines(stream, &sender))
    })
    .collect();
    // the loop ends when both readers have finished and dropped their senders
    drop(sender);
    for line in receiver {
        on_line(&line);
    }
    for reader in readers {
        // a reader that panicked lost only output lines, not the status
        let _ = reader.join();
    }
    child.wait()
}

fn forward_lines(stream: Box<dyn Read + Send>, sender: &mpsc::Sender<String>) {
    let mut reader = BufReader::new(stream);
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        match reader.read_until(b'\n', &mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(_) => {
                let line = String::from_utf8_lossy(&buffer);
                if sender.send(line.trim_end_matches(['\r', '\n']).to_owned()).is_err() {
                    return;
                }
            }
        }
    }
}

/// Copies the DLLs next to the program into `deps/`, where the test
/// executables are: Windows loads a DLL from the executable's folder first,
/// and otherwise would find an old system DirectML (error 887A0004).
fn copy_dlls(profile_dir: &Path) -> Vec<String> {
    let deps = profile_dir.join("deps");
    let entries = match fs::read_dir(profile_dir) {
        Ok(entries) => entries,
        Err(e) => return vec![format!("no DLLs copied: {} not readable ({e})", profile_dir.display())],
    };
    let mut copied = Vec::new();
    let mut problems = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let is_dll = path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("dll"));
        if !is_dll || !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        match fs::copy(&path, deps.join(&name)) {
            Ok(_) => copied.push(name),
            Err(e) => problems.push(format!("could not copy {name} into deps: {e}")),
        }
    }
    copied.sort();
    let mut lines = vec![if copied.is_empty() {
        "no DLLs next to the program".to_owned()
    } else {
        format!("copied into deps: {}", copied.join(", "))
    }];
    lines.extend(problems);
    lines
}

/// Where the checks ran: commit, toolchain, platform, the models present,
/// and the caller's notes.
fn environment(paths: &Paths, notes: Option<&Path>) -> Vec<String> {
    let probe = |exe: &str, args: &[&str]| -> String {
        match Command::new(exe).args(args).current_dir(&paths.root).stdin(Stdio::null()).output() {
            Ok(out) if out.status.success() => {
                String::from_utf8_lossy(&out.stdout).lines().next().unwrap_or("(no output)").trim().to_owned()
            }
            Ok(out) => format!("not available (exit {:?})", out.status.code()),
            Err(e) => format!("not available ({e})"),
        }
    };
    let changed = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&paths.root)
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).lines().count())
        .unwrap_or(0);
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let mut lines = vec![
        format!(
            "git:      {} @ {} ({changed} changed or untracked files)",
            probe("git", &["rev-parse", "--abbrev-ref", "HEAD"]),
            probe("git", &["log", "-1", "--format=%h %s"])
        ),
        format!("rustc:    {}", probe("rustc", &["-V"])),
        format!("cargo:    {}", probe(&cargo, &["-V"])),
        format!("platform: {} {}", std::env::consts::OS, std::env::consts::ARCH),
    ];
    let mut models: Vec<String> = fs::read_dir(paths.root.join("models"))
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let stem = name.strip_suffix(".onnx")?;
                    Some(stem.strip_prefix("scunet_color_real_").unwrap_or(stem).to_owned())
                })
                .collect()
        })
        .unwrap_or_default();
    models.sort();
    lines.push(if models.is_empty() {
        "models:   none in models/ (the model tests are skipped; see models/README.txt)".to_owned()
    } else {
        format!("models:   {}", models.join(", "))
    });
    if let Some(file) = notes {
        match fs::read_to_string(file) {
            Ok(text) => lines.extend(text.lines().filter(|l| !l.trim().is_empty()).map(str::to_owned)),
            Err(e) => lines.push(format!("notes:    {} not readable ({e})", file.display())),
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<CheckArgs, String> {
        parse_args(args.iter().map(|a| (*a).to_owned()))
    }

    #[test]
    fn command_line() {
        let all = parse(&["check"]).unwrap();
        assert!(all.only.is_none() && all.notes.is_none());
        let one = parse(&["check", "--step", "clippy", "--notes", "n.txt"]).unwrap();
        assert_eq!(one.only, Some(Step::Clippy));
        assert_eq!(one.notes, Some(PathBuf::from("n.txt")));
    }

    #[test]
    fn wrong_command_lines_are_refused() {
        for args in [
            &[][..],
            &["build"][..],
            &["check", "--step"][..],
            &["check", "--step", "lint"][..],
            &["check", "--notes"][..],
            &["check", "--fast"][..],
        ] {
            assert!(parse(args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn output_lines_arrive_from_both_streams() {
        let mut command = if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.args(["/C", "echo out& echo err 1>&2"]);
            c
        } else {
            let mut c = Command::new("sh");
            c.args(["-c", "echo out; echo err 1>&2"]);
            c
        };
        let mut seen = Vec::new();
        let status = run_streaming(&mut command, |l| seen.push(l.trim().to_owned())).unwrap();
        assert!(status.success());
        seen.sort();
        assert_eq!(seen, ["err", "out"]);
    }

    #[test]
    fn dlls_are_copied_into_deps() {
        let dir = std::env::temp_dir().join(format!("xtask-dlls-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("deps")).unwrap();
        fs::write(dir.join("b.DLL"), b"b").unwrap();
        fs::write(dir.join("a.dll"), b"a").unwrap();
        fs::write(dir.join("apsophos.exe"), b"x").unwrap();
        let lines = copy_dlls(&dir);
        assert_eq!(lines, ["copied into deps: a.dll, b.DLL"]);
        assert_eq!(fs::read(dir.join("deps").join("a.dll")).unwrap(), b"a");
        assert!(!dir.join("deps").join("apsophos.exe").exists());
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(copy_dlls(&dir).len(), 1);
    }
}
