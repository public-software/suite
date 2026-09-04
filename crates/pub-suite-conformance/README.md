# pub-suite-conformance

The `conformance` command of [suite](https://github.com/public-software/suite), part of Public Software. Kind: `app`; the binary is `conformance`.

A ruleset requires status checks by name, and the name of a check run a reusable workflow produces is `<caller job id> / <reusable job name>`: `suite / deny` is the job `deny` of `rust.yml`, called from the job `suite` of a repository's `ci.yml`. A job a repository runs directly is the check run named by that job: `conformance` is the job `conformance` of this repository's `conformance.yml`, which the suite-only ruleset requires. Nothing on GitHub ties the two together, so a renamed job in `rust.yml` leaves every pull request of every repository waiting forever for a check that no longer exists. This crate reads the three inputs and fails when they disagree:

```sh
conformance --callers .github/workflows                     # a repository's own workflows: the jobs that `uses:` a reusable one
            --workflows ../.github/.github/workflows        # the reusable workflows of public-software/.github
            --rules rules.json                              # a ruleset file, or `gh api repos/public-software/suite/rules/branches/main`
```

Every required context must be a check run the callers produce, with a name that does not depend on the run (a matrix leg or an expression in the name can never be required), from a workflow that triggers on `pull_request` and on `merge_group`, so that the same name serves the pull request and the merge queue; a queue entry waiting for a check that never reports never merges. A check run the callers produce without being required is listed, not judged. Exit status 0 when the contract holds, 1 with a finding per broken context on stderr, 2 when an input cannot be read.

The parser is [yaml-rust2](https://crates.io/crates/yaml-rust2), YAML 1.2, so `on:` stays a key (a YAML 1.1 parser reads it as the boolean `true`) and the rules, which GitHub hands out as JSON, parse with the same crate; there is no other dependency.

```sh
cargo run -p pub-suite-conformance -- --help
cargo nextest run -p pub-suite-conformance      # unit tests and tests/cli.rs, which runs the built binary
```

Where it runs: `.github/workflows/conformance.yml` of this repository, against the release of the reusable workflows its `ci.yml` pins and the rules GitHub applies to `main`; and `lint.yml` of `public-software/.github`, against the workflows under review there. The bootstrap kit runs it offline against its own templates and rulesets.

Its entry in the repository's `CATALOG.toml`:

```toml
[[component]]
crate     = "pub-suite-conformance"
kind      = "app"
ledger    = "ci-conformance"
readiness = "partial"
effort    = 1
specs     = []
provides  = []
requires  = []
```
