//! `conformance` — the command of [`suite`](https://github.com/public-software/suite) that proves the reusable
//! workflows of `public-software/.github` still produce every check run the rulesets require, on pull requests and in
//! the merge queue alike. `main` does the I/O; [`run`] does the work and is what the unit tests call.

#![forbid(unsafe_code)]

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

// the crate's identifier: pub-suite-conformance with the hyphens as underscores
use pub_suite_conformance::{Report, Source, check};

/// Exit status when the contract holds.
const CLEAN: u8 = 0;
/// Exit status when a finding breaks the contract.
const BROKEN: u8 = 1;
/// Exit status when the inputs cannot be read or the command line is wrong.
const UNUSABLE: u8 = 2;
/// What `--help` and a wrong command line print.
const USAGE: &str = "usage: conformance --callers <dir> --workflows <dir> --rules <file>
  --callers    a repository's own .github/workflows: the jobs that `uses:` a reusable workflow name the check runs
  --workflows  the reusable workflows of the .github repository (rust.yml, review.yml)
  --rules      a ruleset file, or what `gh api repos/<org>/<repo>/rules/branches/main` answers";
/// What `--version` prints.
const VERSION_LINE: &str = concat!("conformance ", env!("CARGO_PKG_VERSION"));

/// The three inputs, as paths.
#[derive(Debug, PartialEq, Eq)]
struct Inputs {
    callers: PathBuf,
    workflows: PathBuf,
    rules: PathBuf,
}

/// What a run prints and how it exits.
#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    stdout: String,
    stderr: String,
    status: u8,
}

impl Outcome {
    fn clean(stdout: String) -> Self {
        Self {
            stdout,
            stderr: String::new(),
            status: CLEAN,
        }
    }

    fn broken(report: &Report) -> Self {
        Self {
            stdout: report.to_string(),
            stderr: report
                .findings
                .iter()
                .map(|finding| format!("conformance: {finding}\n"))
                .collect(),
            status: BROKEN,
        }
    }

    fn unusable(problem: &str) -> Self {
        Self {
            stdout: String::new(),
            stderr: format!("conformance: {problem}\n"),
            status: UNUSABLE,
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let outcome = run(&args);
    print!("{}", outcome.stdout);
    eprint!("{}", outcome.stderr);
    ExitCode::from(outcome.status)
}

/// Runs the command on `args`; free of process I/O, so tests call it directly.
fn run(args: &[String]) -> Outcome {
    let flags: Vec<&str> = args.iter().map(String::as_str).collect();
    match flags.as_slice() {
        ["--version"] => return Outcome::clean(format!("{VERSION_LINE}\n")),
        ["--help"] | ["-h"] => return Outcome::clean(format!("{USAGE}\n")),
        _ => {}
    }
    let inputs = match parse_args(&flags) {
        Ok(inputs) => inputs,
        Err(problem) => return Outcome::unusable(&format!("{problem}\n{USAGE}")),
    };
    match judge(&inputs) {
        Ok(report) if report.is_clean() => Outcome::clean(report.to_string()),
        Ok(report) => Outcome::broken(&report),
        Err(problem) => Outcome::unusable(&problem),
    }
}

/// `--callers <dir> --workflows <dir> --rules <file>`, in any order, each once.
fn parse_args(flags: &[&str]) -> Result<Inputs, String> {
    let (mut callers, mut workflows, mut rules) = (None, None, None);
    let mut rest = flags;
    while let [flag, value, tail @ ..] = rest {
        let slot = match *flag {
            "--callers" => &mut callers,
            "--workflows" => &mut workflows,
            "--rules" => &mut rules,
            other => return Err(format!("unknown argument `{other}`")),
        };
        if slot.replace(PathBuf::from(value)).is_some() {
            return Err(format!("{flag} given twice"));
        }
        rest = tail;
    }
    if let [dangling] = rest {
        return Err(format!("`{dangling}` needs a value"));
    }
    Ok(Inputs {
        callers: callers.ok_or("--callers is required")?,
        workflows: workflows.ok_or("--workflows is required")?,
        rules: rules.ok_or("--rules is required")?,
    })
}

/// Reads the inputs and judges the contract.
fn judge(inputs: &Inputs) -> Result<Report, String> {
    let callers = read_workflows(&inputs.callers)?;
    let workflows = read_workflows(&inputs.workflows)?;
    let rules = read_source(&inputs.rules)?;
    check(&callers, &workflows, &rules).map_err(|problem| problem.to_string())
}

/// Every `.yml` and `.yaml` file of a directory, by name.
fn read_workflows(dir: &Path) -> Result<Vec<Source>, String> {
    let located = |problem: std::io::Error| format!("{}: {problem}", dir.display());
    let entries = fs::read_dir(dir).map_err(located)?;
    let mut paths: Vec<PathBuf> = entries
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<_, _>>()
        .map_err(located)?;
    paths.sort();
    paths
        .iter()
        .filter(|path| {
            matches!(
                path.extension().and_then(OsStr::to_str),
                Some("yml" | "yaml")
            )
        })
        .map(|path| read_source(path))
        .collect()
}

/// One file as a [`Source`] named by its file name.
fn read_source(path: &Path) -> Result<Source, String> {
    let name = path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| format!("{}: not a file name", path.display()))?;
    let text =
        fs::read_to_string(path).map_err(|problem| format!("{}: {problem}", path.display()))?;
    Ok(Source::new(name, text))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn the_three_inputs_are_read_in_any_order() {
        let inputs =
            parse_args(&["--rules", "r.json", "--callers", "c", "--workflows", "w"]).unwrap();
        assert_eq!(
            inputs,
            Inputs {
                callers: PathBuf::from("c"),
                workflows: PathBuf::from("w"),
                rules: PathBuf::from("r.json"),
            }
        );
    }

    #[test]
    fn a_missing_input_is_named() {
        let problem = parse_args(&["--callers", "c", "--workflows", "w"]).unwrap_err();
        assert!(problem.contains("--rules"), "{problem}");
    }

    #[test]
    fn an_unknown_argument_is_named() {
        let problem = parse_args(&["--bogus", "x"]).unwrap_err();
        assert!(problem.contains("--bogus"), "{problem}");
    }

    #[test]
    fn a_repeated_or_dangling_flag_is_refused() {
        assert!(parse_args(&["--rules", "a", "--rules", "b"]).is_err());
        assert!(parse_args(&["--rules"]).is_err());
    }

    #[test]
    fn a_wrong_command_line_is_unusable_and_prints_the_usage() {
        let outcome = run(&args(&["--nope"]));
        assert_eq!(outcome.status, UNUSABLE);
        assert!(outcome.stderr.contains("usage:"), "{}", outcome.stderr);
    }

    #[test]
    fn a_directory_that_does_not_exist_is_unusable_and_named() {
        let outcome = run(&args(&[
            "--callers",
            "/nonexistent/callers",
            "--workflows",
            "/nonexistent/workflows",
            "--rules",
            "/nonexistent/rules.json",
        ]));
        assert_eq!(outcome.status, UNUSABLE);
        assert!(
            outcome.stderr.contains("/nonexistent/callers"),
            "{}",
            outcome.stderr
        );
    }

    #[test]
    fn version_names_the_command() {
        assert!(
            run(&args(&["--version"]))
                .stdout
                .starts_with("conformance ")
        );
    }
}
