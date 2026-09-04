//! `pub-suite-conformance` — the contract between the reusable workflows of
//! [`public-software/.github`](https://github.com/public-software/.github), the caller workflows every repository carries and
//! the rules that protect `main`.
//!
//! A ruleset requires status checks by name, and the name of a check run a reusable workflow produces is
//! `<caller job id> / <reusable job name>`. Nothing on GitHub ties the two together: rename a job in `rust.yml`
//! and every pull request of every repository waits forever for a check that no longer exists. [`check`] reads
//! the three inputs and reports every required context that no caller produces, and every producing caller
//! that does not run on both `pull_request` and `merge_group` (a queue entry whose check never reports never
//! merges).
//!
//! The parser is YAML 1.2, so `on:` is a key and not the boolean `true` a YAML 1.1 parser makes of it, and the
//! rules, which GitHub hands out as JSON, parse with the same crate.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::fmt;

use yaml_rust2::{Yaml, YamlLoader};

/// The path fragment every reusable-workflow `uses:` carries, local (`./.github/workflows/x.yml`) or remote
/// (`org/.github/.github/workflows/x.yml@ref`); an action's `uses:` never does.
const WORKFLOWS_PATH: &str = ".github/workflows/";
/// The event a pull request's checks run on.
const PULL_REQUEST: &str = "pull_request";
/// The event the merge queue runs checks on.
const MERGE_GROUP: &str = "merge_group";
/// The event that makes a workflow reusable.
const WORKFLOW_CALL: &str = "workflow_call";
/// The rule type that names the required status checks.
const REQUIRED_STATUS_CHECKS: &str = "required_status_checks";
/// What separates the caller job from the reusable job in a check-run name.
const CONTEXT_SEPARATOR: &str = " / ";
/// The start of a GitHub expression; a job name carrying one differs from run to run.
const EXPRESSION: &str = "${{";

/// One input file: its file name (what a caller's `uses:` refers to) and its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// The file name, `rust.yml` for instance.
    pub name: String,
    /// The file's text.
    pub text: String,
}

impl Source {
    /// A named source.
    pub fn new(name: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            text: text.into(),
        }
    }
}

/// Why the inputs could not be judged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// A file is not YAML (JSON included).
    Parse {
        /// The file.
        file: String,
        /// What the parser said.
        detail: String,
    },
    /// A caller job `uses:` a workflow that is not among the reusable workflows given.
    UnknownWorkflow {
        /// The caller file.
        caller: String,
        /// The caller job.
        job: String,
        /// The workflow file the job uses.
        workflow: String,
    },
    /// A workflow a caller uses is not `on: workflow_call`.
    NotReusable {
        /// The workflow file.
        workflow: String,
    },
    /// The rules carry no `required_status_checks` rule: nothing protects `main`, so there is no contract.
    NoRequiredChecks,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse { file, detail } => write!(f, "{file}: not YAML: {detail}"),
            Self::UnknownWorkflow {
                caller,
                job,
                workflow,
            } => write!(
                f,
                "{caller}: job {job} uses {workflow}, which is not among the reusable workflows given"
            ),
            Self::NotReusable { workflow } => {
                write!(
                    f,
                    "{workflow}: not a reusable workflow (no `on: workflow_call`)"
                )
            }
            Self::NoRequiredChecks => write!(
                f,
                "the rules carry no required_status_checks rule: nothing protects main, so there is no contract to check"
            ),
        }
    }
}

impl std::error::Error for Error {}

/// Where a check run comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    /// The caller file, `ci.yml` for instance.
    pub caller: String,
    /// The caller job, the first half of the check-run name.
    pub caller_job: String,
    /// The reusable workflow file the caller job uses.
    pub workflow: String,
    /// The reusable job id.
    pub job: String,
    /// The events the caller workflow triggers on.
    pub events: BTreeSet<String>,
}

/// A check run a caller produces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckRun {
    /// The check-run name, `<caller job> / <job name>`; `None` when the name depends on the run (a matrix leg,
    /// an expression in the name), so that no ruleset can require it.
    pub context: Option<String>,
    /// The reusable job's `name:` as written, or its id when it has none.
    pub name: String,
    /// Where it comes from.
    pub origin: Origin,
}

/// One way the contract is broken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// A required context that no caller produces.
    Missing {
        /// The required context.
        context: String,
        /// What resembles it, when something does.
        hint: String,
    },
    /// The caller producing a required context does not trigger on `pull_request`.
    NotOnPullRequest {
        /// The required context.
        context: String,
        /// The caller file.
        caller: String,
    },
    /// The caller producing a required context does not trigger on `merge_group`.
    NotInMergeQueue {
        /// The required context.
        context: String,
        /// The caller file.
        caller: String,
    },
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing { context, hint } => {
                write!(f, "missing: {context} — no caller produces it ({hint})")
            }
            Self::NotOnPullRequest { context, caller } => write!(
                f,
                "not on pull requests: {context} — {caller} does not trigger on {PULL_REQUEST}"
            ),
            Self::NotInMergeQueue { context, caller } => write!(
                f,
                "not in the merge queue: {context} — {caller} does not trigger on {MERGE_GROUP}, so a queue entry waits for it forever"
            ),
        }
    }
}

/// The verdict: what the rules require, what the callers produce, and where the two disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// The required status-check contexts.
    pub required: BTreeSet<String>,
    /// Every check run the callers produce, run-dependent names included.
    pub produced: Vec<CheckRun>,
    /// Where the contract is broken; empty when it holds.
    pub findings: Vec<Finding>,
}

impl Report {
    /// The contract holds.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }

    /// The check run that produces a context, when one does.
    #[must_use]
    pub fn producer(&self, context: &str) -> Option<&CheckRun> {
        self.produced
            .iter()
            .find(|run| run.context.as_deref() == Some(context))
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "required status checks: {}", self.required.len())?;
        for context in &self.required {
            match self.producer(context) {
                Some(run) if run.origin.workflow == run.origin.caller => writeln!(
                    f,
                    "  {context:<20} {}: job {}",
                    run.origin.caller, run.origin.job
                )?,
                Some(run) => writeln!(
                    f,
                    "  {context:<20} {}: {} -> {}: {}",
                    run.origin.caller, run.origin.caller_job, run.origin.workflow, run.origin.job
                )?,
                None => writeln!(f, "  {context:<20} (no caller produces it)")?,
            }
        }
        let unrequired: Vec<&str> = self
            .produced
            .iter()
            .filter_map(|run| run.context.as_deref())
            .filter(|context| !self.required.contains(*context))
            .collect();
        if !unrequired.is_empty() {
            writeln!(f, "also produced, not required: {}", unrequired.join(", "))?;
        }
        let dynamic: Vec<String> = self
            .produced
            .iter()
            .filter(|run| run.context.is_none())
            .map(|run| {
                format!(
                    "{}: {} \"{}\"",
                    run.origin.workflow, run.origin.job, run.name
                )
            })
            .collect();
        if !dynamic.is_empty() {
            writeln!(
                f,
                "run-dependent names, never required: {}",
                dynamic.join(", ")
            )?;
        }
        Ok(())
    }
}

/// Judges the contract: every required context of `rules` must be a check run the `callers` produce through
/// the reusable `workflows`, from a workflow that triggers on `pull_request` and on `merge_group`.
///
/// # Errors
///
/// A file that is not YAML, a caller using a workflow that is not given, a used workflow that is not
/// reusable, or rules without a `required_status_checks` rule.
pub fn check(callers: &[Source], workflows: &[Source], rules: &Source) -> Result<Report, Error> {
    let required = required_contexts(rules)?;
    let produced = check_runs(callers, workflows)?;
    let findings = required
        .iter()
        .flat_map(|context| judge(context, &produced))
        .collect();
    Ok(Report {
        required,
        produced,
        findings,
    })
}

/// The status-check contexts the rules require: from a ruleset document (`rules: [...]`) or from the array
/// `gh api repos/<org>/<repo>/rules/branches/main` answers.
///
/// # Errors
///
/// A file that is not YAML, or rules without a `required_status_checks` rule.
pub fn required_contexts(rules: &Source) -> Result<BTreeSet<String>, Error> {
    let doc = parse(rules)?;
    let list: &[Yaml] = match &doc {
        Yaml::Array(rules) => rules,
        Yaml::Hash(_) => doc["rules"].as_vec().map_or(&[], Vec::as_slice),
        _ => &[],
    };
    let contexts: BTreeSet<String> = list
        .iter()
        .filter(|rule| rule["type"].as_str() == Some(REQUIRED_STATUS_CHECKS))
        .flat_map(|rule| {
            rule["parameters"][REQUIRED_STATUS_CHECKS]
                .as_vec()
                .into_iter()
                .flatten()
        })
        .filter_map(|required| required["context"].as_str().map(str::to_owned))
        .collect();
    if contexts.is_empty() {
        return Err(Error::NoRequiredChecks);
    }
    Ok(contexts)
}

/// Every check run the `callers` produce: one per reusable job of every caller job that `uses:` a workflow,
/// named `<caller job> / <reusable job name>`, and one per job a caller runs itself, named by that job.
///
/// # Errors
///
/// A file that is not YAML, a caller using a workflow that is not given, or a used workflow that is not
/// reusable.
pub fn check_runs(callers: &[Source], workflows: &[Source]) -> Result<Vec<CheckRun>, Error> {
    let parsed: Vec<(&Source, Yaml)> = workflows
        .iter()
        .map(|workflow| parse(workflow).map(|doc| (workflow, doc)))
        .collect::<Result<_, _>>()?;
    let mut runs = Vec::new();
    for caller in callers {
        let doc = parse(caller)?;
        let caller_events = events(&doc);
        for (caller_job, job) in jobs(&doc) {
            let Some(used) = job["uses"].as_str().and_then(used_workflow) else {
                runs.push(own_check_run(caller, caller_job, job, &caller_events));
                continue;
            };
            let (source, reusable) = parsed
                .iter()
                .find(|(workflow, _)| workflow.name == used)
                .ok_or_else(|| Error::UnknownWorkflow {
                    caller: caller.name.clone(),
                    job: caller_job.to_owned(),
                    workflow: used.to_owned(),
                })?;
            if !events(reusable).contains(WORKFLOW_CALL) {
                return Err(Error::NotReusable {
                    workflow: source.name.clone(),
                });
            }
            for (job_id, reusable_job) in jobs(reusable) {
                let name = reusable_job["name"].as_str().unwrap_or(job_id).to_owned();
                let context = (!is_run_dependent(&name, reusable_job))
                    .then(|| format!("{caller_job}{CONTEXT_SEPARATOR}{name}"));
                runs.push(CheckRun {
                    context,
                    name,
                    origin: Origin {
                        caller: caller.name.clone(),
                        caller_job: caller_job.to_owned(),
                        workflow: source.name.clone(),
                        job: job_id.to_owned(),
                        events: caller_events.clone(),
                    },
                });
            }
        }
    }
    Ok(runs)
}

/// The check run a caller's own job produces (no reusable workflow): GitHub names it by the job's `name:`, or
/// its id; the workflow of its origin is the caller itself.
fn own_check_run(caller: &Source, job_id: &str, job: &Yaml, events: &BTreeSet<String>) -> CheckRun {
    let name = job["name"].as_str().unwrap_or(job_id).to_owned();
    let context = (!is_run_dependent(&name, job)).then(|| name.clone());
    CheckRun {
        context,
        name,
        origin: Origin {
            caller: caller.name.clone(),
            caller_job: job_id.to_owned(),
            workflow: caller.name.clone(),
            job: job_id.to_owned(),
            events: events.clone(),
        },
    }
}

/// The first document of a source.
fn parse(source: &Source) -> Result<Yaml, Error> {
    let docs = YamlLoader::load_from_str(&source.text).map_err(|problem| Error::Parse {
        file: source.name.clone(),
        detail: problem.to_string(),
    })?;
    Ok(docs.into_iter().next().unwrap_or(Yaml::Null))
}

/// The events a workflow triggers on: the keys of `on:`, or its list, or its one string.
fn events(doc: &Yaml) -> BTreeSet<String> {
    match &doc["on"] {
        Yaml::Hash(map) => map
            .keys()
            .filter_map(Yaml::as_str)
            .map(str::to_owned)
            .collect(),
        Yaml::Array(list) => list
            .iter()
            .filter_map(Yaml::as_str)
            .map(str::to_owned)
            .collect(),
        Yaml::String(one) => BTreeSet::from([one.clone()]),
        _ => BTreeSet::new(),
    }
}

/// The jobs of a workflow, in order: `(id, job)`.
fn jobs(doc: &Yaml) -> impl Iterator<Item = (&str, &Yaml)> {
    doc["jobs"]
        .as_hash()
        .into_iter()
        .flat_map(|map| map.iter())
        .filter_map(|(id, job)| id.as_str().map(|id| (id, job)))
}

/// The reusable workflow file a `uses:` names, or `None` for an action.
fn used_workflow(uses: &str) -> Option<&str> {
    let start = uses.find(WORKFLOWS_PATH)? + WORKFLOWS_PATH.len();
    let rest = &uses[start..];
    let end = rest.find('@').unwrap_or(rest.len());
    Some(rest[..end].trim())
}

/// A name that differs from run to run: an expression in it, or a matrix (GitHub appends the leg's values).
fn is_run_dependent(name: &str, job: &Yaml) -> bool {
    name.contains(EXPRESSION) || !job["strategy"]["matrix"].is_badvalue()
}

/// The findings one required context raises.
fn judge(context: &str, produced: &[CheckRun]) -> Vec<Finding> {
    let Some(run) = produced
        .iter()
        .find(|run| run.context.as_deref() == Some(context))
    else {
        return vec![Finding::Missing {
            context: context.to_owned(),
            hint: hint(context, produced),
        }];
    };
    let mut findings = Vec::new();
    if !run.origin.events.contains(PULL_REQUEST) {
        findings.push(Finding::NotOnPullRequest {
            context: context.to_owned(),
            caller: run.origin.caller.clone(),
        });
    }
    if !run.origin.events.contains(MERGE_GROUP) {
        findings.push(Finding::NotInMergeQueue {
            context: context.to_owned(),
            caller: run.origin.caller.clone(),
        });
    }
    findings
}

/// What to say about a context nobody produces: the job with that id was renamed or is run-dependent, the
/// caller job was renamed, or nothing resembles it.
fn hint(context: &str, produced: &[CheckRun]) -> String {
    let (caller_job, job_name) = context
        .split_once(CONTEXT_SEPARATOR)
        .unwrap_or(("", context));
    // a context without the separator names a caller's own job, whose origin is the caller itself
    let own = caller_job.is_empty();
    if let Some(run) = produced.iter().find(|run| {
        run.origin.job == job_name
            && if own {
                run.origin.workflow == run.origin.caller
            } else {
                run.origin.caller_job == caller_job
            }
    }) {
        return match run.context {
            Some(_) => format!(
                "job {job_name} of {} is named \"{}\"",
                run.origin.workflow, run.name
            ),
            None => format!(
                "job {job_name} of {} has the run-dependent name \"{}\", which no ruleset can require",
                run.origin.workflow, run.name
            ),
        };
    }
    if let Some(run) = produced.iter().find(|run| run.name == job_name) {
        return format!(
            "{}: {} produces \"{}\" — the caller job is {}, not {caller_job}",
            run.origin.caller,
            run.origin.caller_job,
            run.context.as_deref().unwrap_or(&run.name),
            run.origin.caller_job
        );
    }
    let names: BTreeSet<&str> = produced
        .iter()
        .filter_map(|run| run.context.as_deref())
        .collect();
    format!(
        "the callers produce: {}",
        names.into_iter().collect::<Vec<_>>().join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const CI: &str = "name: ci
on:
  push: { branches: [main] }
  pull_request:
  merge_group:
jobs:
  suite:
    uses: acme/.github/.github/workflows/rust.yml@0123abc # v1.0.0
";
    const REVIEW: &str = "name: review
on:
  pull_request: { types: [opened, synchronize] }
  merge_group:
jobs:
  suite:
    uses: acme/.github/.github/workflows/review.yml@0123abc # v1.0.0
";
    const RUST: &str = "name: rust
on:
  workflow_call:
jobs:
  pub-check:
    name: pub-check
    runs-on: ubuntu-latest
  probe:
    runs-on: ubuntu-latest
  test-os:
    name: test-os (${{ matrix.os }})
    strategy: { matrix: { os: [ubuntu-latest, macos-latest] } }
    runs-on: ${{ matrix.os }}
  test:
    name: test
    needs: [probe, test-os]
    if: always()
  deny:
    name: deny
    if: needs.probe.outputs.has_crates == 'true'
";
    const REVIEW_WORKFLOW: &str = "name: review
on:
  workflow_call:
jobs:
  agent:
    name: agent
    if: github.event_name == 'pull_request'
  policy:
    name: policy
    needs: agent
    if: always()
";
    const RULESET: &str = r#"{"name": "main", "rules": [{"type": "deletion"}, {"type": "required_status_checks", "parameters": {"strict_required_status_checks_policy": true, "required_status_checks": [{"context": "suite / pub-check"}, {"context": "suite / probe"}, {"context": "suite / test"}, {"context": "suite / deny"}, {"context": "suite / policy"}]}}]}"#;

    fn sources(files: &[(&str, &str)]) -> Vec<Source> {
        files
            .iter()
            .map(|(name, text)| Source::new(*name, *text))
            .collect()
    }

    fn report(callers: &[(&str, &str)], workflows: &[(&str, &str)], rules: &str) -> Report {
        check(
            &sources(callers),
            &sources(workflows),
            &Source::new("rules.json", rules),
        )
        .expect("the inputs parse")
    }

    fn standard() -> Report {
        report(
            &[("ci.yml", CI), ("review.yml", REVIEW)],
            &[("rust.yml", RUST), ("review.yml", REVIEW_WORKFLOW)],
            RULESET,
        )
    }

    /// A caller whose job runs on its own (no reusable workflow): the check run is named by the job.
    const OWN: &str = "name: conformance
on:
  push: { branches: [main] }
  pull_request:
  merge_group:
jobs:
  conformance:
    name: conformance
    runs-on: ubuntu-latest
";
    /// The rules a repository with its own check carries: the branch-rules array shape.
    const OWN_RULES: &str = r#"[{"type": "required_status_checks", "parameters": {"required_status_checks": [{"context": "suite / deny"}, {"context": "conformance"}]}}]"#;

    fn own(conformance: &str) -> Report {
        report(
            &[("ci.yml", CI), ("conformance.yml", conformance)],
            &[("rust.yml", RUST)],
            OWN_RULES,
        )
    }

    #[test]
    fn a_callers_own_job_is_the_check_run_named_by_its_name() {
        let report = own(OWN);
        assert!(report.is_clean(), "{:?}", report.findings);
        let run = report.producer("conformance").unwrap();
        assert_eq!(run.origin.caller, "conformance.yml");
        assert_eq!(run.origin.workflow, "conformance.yml");
        assert_eq!(run.origin.job, "conformance");
        assert!(
            report
                .to_string()
                .contains("conformance.yml: job conformance"),
            "{report}"
        );
    }

    #[test]
    fn a_renamed_own_job_is_missing_with_its_new_name() {
        let renamed = OWN.replace("    name: conformance", "    name: contract");
        let report = own(&renamed);
        let [Finding::Missing { context, hint }] = report.findings.as_slice() else {
            panic!("{:?}", report.findings);
        };
        assert_eq!(context, "conformance");
        assert!(hint.contains("contract"), "{hint}");
    }

    #[test]
    fn an_own_job_with_a_matrix_is_run_dependent() {
        let legs = OWN.replace(
            "    runs-on: ubuntu-latest\n",
            "    strategy: { matrix: { os: [ubuntu-latest, macos-latest] } }\n    runs-on: ${{ matrix.os }}\n",
        );
        let report = own(&legs);
        assert!(report.producer("conformance").is_none());
        assert!(matches!(
            report.findings.as_slice(),
            [Finding::Missing { .. }]
        ));
    }

    #[test]
    fn an_own_job_without_merge_group_is_a_finding_too() {
        let unqueued = OWN.replace("  merge_group:\n", "");
        let report = own(&unqueued);
        assert!(matches!(
            report.findings.as_slice(),
            [Finding::NotInMergeQueue { context, caller }] if context == "conformance" && caller == "conformance.yml"
        ));
    }

    fn contexts(findings: &[Finding]) -> Vec<&str> {
        findings
            .iter()
            .map(|finding| match finding {
                Finding::Missing { context, .. }
                | Finding::NotOnPullRequest { context, .. }
                | Finding::NotInMergeQueue { context, .. } => context.as_str(),
            })
            .collect()
    }

    #[test]
    fn every_required_context_is_produced_and_queued() {
        let report = standard();
        assert!(report.is_clean(), "{:?}", report.findings);
        assert_eq!(report.required.len(), 5);
        assert_eq!(report.producer("suite / deny").unwrap().origin.job, "deny");
    }

    #[test]
    fn a_renamed_reusable_job_is_missing_with_its_new_name() {
        let renamed = RUST.replace("name: deny", "name: deny-check");
        let report = report(
            &[("ci.yml", CI), ("review.yml", REVIEW)],
            &[("rust.yml", &renamed), ("review.yml", REVIEW_WORKFLOW)],
            RULESET,
        );
        let [Finding::Missing { context, hint }] = report.findings.as_slice() else {
            panic!("{:?}", report.findings);
        };
        assert_eq!(context, "suite / deny");
        assert!(hint.contains("deny-check"), "{hint}");
        assert!(
            report.findings[0]
                .to_string()
                .starts_with("missing: suite / deny"),
            "{}",
            report.findings[0]
        );
    }

    #[test]
    fn a_renamed_caller_job_is_missing_with_the_caller_named() {
        let renamed = CI.replace("  suite:", "  ci:");
        let report = report(
            &[("ci.yml", &renamed), ("review.yml", REVIEW)],
            &[("rust.yml", RUST), ("review.yml", REVIEW_WORKFLOW)],
            RULESET,
        );
        assert_eq!(
            contexts(&report.findings),
            [
                "suite / deny",
                "suite / probe",
                "suite / pub-check",
                "suite / test"
            ]
        );
        let Finding::Missing { hint, .. } = &report.findings[0] else {
            panic!("{:?}", report.findings);
        };
        assert!(hint.contains("the caller job is ci"), "{hint}");
    }

    #[test]
    fn a_dynamic_job_name_can_never_be_required() {
        let rules = RULESET.replace("suite / deny", "suite / test-os");
        let report = report(
            &[("ci.yml", CI), ("review.yml", REVIEW)],
            &[("rust.yml", RUST), ("review.yml", REVIEW_WORKFLOW)],
            &rules,
        );
        let [Finding::Missing { context, hint }] = report.findings.as_slice() else {
            panic!("{:?}", report.findings);
        };
        assert_eq!(context, "suite / test-os");
        assert!(hint.contains("run-dependent"), "{hint}");
        assert!(report.producer("suite / test-os").is_none());
    }

    #[test]
    fn a_caller_without_merge_group_hangs_the_queue() {
        let unqueued = CI.replace("  merge_group:\n", "");
        let report = report(
            &[("ci.yml", &unqueued), ("review.yml", REVIEW)],
            &[("rust.yml", RUST), ("review.yml", REVIEW_WORKFLOW)],
            RULESET,
        );
        assert_eq!(report.findings.len(), 4, "{:?}", report.findings);
        assert!(report.findings.iter().all(|finding| matches!(
            finding,
            Finding::NotInMergeQueue { caller, .. } if caller == "ci.yml"
        )));
        assert!(report.findings[0].to_string().contains(MERGE_GROUP));
    }

    #[test]
    fn a_caller_without_pull_request_never_reports() {
        let pushed = CI.replace("  pull_request:\n", "");
        let report = report(
            &[("ci.yml", &pushed), ("review.yml", REVIEW)],
            &[("rust.yml", RUST), ("review.yml", REVIEW_WORKFLOW)],
            RULESET,
        );
        assert_eq!(report.findings.len(), 4, "{:?}", report.findings);
        assert!(
            report
                .findings
                .iter()
                .all(|finding| matches!(finding, Finding::NotOnPullRequest { .. }))
        );
    }

    #[test]
    fn the_branch_rules_api_array_is_read_like_a_ruleset_file() {
        let array = r#"[{"type": "deletion", "ruleset_source_type": "Repository", "ruleset_id": 1}, {"type": "required_status_checks", "parameters": {"required_status_checks": [{"context": "suite / deny", "integration_id": 15368}, {"context": "suite / test"}]}, "ruleset_id": 1}]"#;
        let from_array = required_contexts(&Source::new("rules.json", array)).unwrap();
        assert_eq!(
            from_array,
            BTreeSet::from(["suite / deny".to_owned(), "suite / test".to_owned()])
        );
        let from_file = required_contexts(&Source::new("main-default.json", RULESET)).unwrap();
        assert_eq!(from_file.len(), 5);
    }

    #[test]
    fn no_required_checks_rule_is_an_error() {
        let rules = r#"{"name": "main", "rules": [{"type": "deletion"}]}"#;
        assert_eq!(
            required_contexts(&Source::new("rules.json", rules)),
            Err(Error::NoRequiredChecks)
        );
        assert_eq!(
            required_contexts(&Source::new("rules.json", "[]")),
            Err(Error::NoRequiredChecks)
        );
    }

    #[test]
    fn on_is_a_string_key_not_a_boolean() {
        let doc = parse(&Source::new("x.yml", "on:\n  push:\n  merge_group:\n")).unwrap();
        assert_eq!(
            events(&doc),
            BTreeSet::from(["push".to_owned(), MERGE_GROUP.to_owned()])
        );
        let list = parse(&Source::new("x.yml", "on: [push, pull_request]\n")).unwrap();
        assert!(events(&list).contains(PULL_REQUEST));
        let one = parse(&Source::new("x.yml", "on: workflow_call\n")).unwrap();
        assert!(events(&one).contains(WORKFLOW_CALL));
    }

    #[test]
    fn an_unrequired_check_run_is_not_a_finding() {
        let report = standard();
        assert!(report.producer("suite / agent").is_some());
        assert!(report.is_clean());
        assert!(
            report
                .to_string()
                .contains("also produced, not required: suite / agent")
        );
    }

    #[test]
    fn an_action_is_not_a_reusable_workflow() {
        assert_eq!(used_workflow("actions/checkout@0123abc"), None);
        assert_eq!(
            used_workflow("acme/.github/.github/workflows/rust.yml@0123abc"),
            Some("rust.yml")
        );
        assert_eq!(
            used_workflow("./.github/workflows/local.yml"),
            Some("local.yml")
        );
    }

    #[test]
    fn a_caller_using_an_unknown_workflow_is_an_error() {
        let problem = check(
            &sources(&[("ci.yml", CI)]),
            &sources(&[("review.yml", REVIEW_WORKFLOW)]),
            &Source::new("rules.json", RULESET),
        )
        .unwrap_err();
        assert_eq!(
            problem,
            Error::UnknownWorkflow {
                caller: "ci.yml".to_owned(),
                job: "suite".to_owned(),
                workflow: "rust.yml".to_owned(),
            }
        );
    }

    #[test]
    fn a_used_workflow_must_be_reusable() {
        let problem = check(
            &sources(&[("ci.yml", CI)]),
            &sources(&[("rust.yml", CI)]),
            &Source::new("rules.json", RULESET),
        )
        .unwrap_err();
        assert_eq!(
            problem,
            Error::NotReusable {
                workflow: "rust.yml".to_owned()
            }
        );
    }

    #[test]
    fn a_file_that_is_not_yaml_is_an_error() {
        let problem = required_contexts(&Source::new("rules.json", "{\"rules\": [")).unwrap_err();
        assert!(matches!(problem, Error::Parse { ref file, .. } if file == "rules.json"));
    }

    #[test]
    fn a_job_without_a_name_is_named_by_its_id() {
        let report = standard();
        assert_eq!(report.producer("suite / probe").unwrap().name, "probe");
    }

    #[test]
    fn the_report_lists_each_required_context_with_its_producer() {
        let text = standard().to_string();
        assert!(text.contains("required status checks: 5"), "{text}");
        assert!(text.contains("suite / deny"), "{text}");
        assert!(text.contains("ci.yml: suite -> rust.yml: deny"), "{text}");
        assert!(
            text.contains("run-dependent names, never required: rust.yml: test-os"),
            "{text}"
        );
    }
}
