# Secret scanning in CI

The `Secret Detection` job in `.github/workflows/security.yml` fails the
Security workflow when a credential is committed.

## What it does

- Installs the gitleaks CLI (MIT-licensed, no key needed) at a pinned version.
  The download is checked against a hard-coded sha256 of the release tarball,
  so a swapped release fails the job. Upgrading means changing the version and
  the hash together.
- Runs `gitleaks git --config .gitleaks.toml --redact --exit-code 1` over the
  full history (`fetch-depth: 0`), so a secret that was committed and later
  deleted is still found. `--redact` keeps the value out of the log.
- Uploads `gitleaks.sarif` as the `gitleaks-report` artifact on every run.
- Has no `continue-on-error`: a finding is a red build.

Until 2026-11 the job used `gitleaks/gitleaks-action`, which exits at
"missing gitleaks license" for organization repositories, so nothing had ever
been scanned. A first review of the history (2,543 findings with the default
rules) found no real credential; the findings were content hashes in benchmark
manifests, redaction and JWT test vectors, AWS-style documentation examples
and doc placeholders. `.gitleaks.toml` allowlists exactly those.

## Run it locally

```bash
gitleaks git --config .gitleaks.toml --redact .          # history
gitleaks dir --config .gitleaks.toml --redact .          # working tree
```

`gitleaks dir` walks everything under the path, including `target/`; point it
at a clean export (`git archive HEAD`) rather than a build tree.

## A finding appeared

1. Open the `gitleaks-report` artifact (or rerun locally without `--redact`
   on a private terminal) and look at the file, line and rule.
2. If it is a real credential: **rotate it first.** Removing it from the tree
   or rewriting history does not un-leak it; anyone who cloned or forked has
   it. Then remove it from the code. Never add a real secret to the allowlist.
3. If it is a false positive (a test vector, a digest, a placeholder): add a
   narrow entry to `.gitleaks.toml` in a pull request a reviewer will read.
   Every entry must be commented with what the value is and why it is not a
   secret, and must pin all of: the exact path (not a directory glob), the
   rule (`targetRules`), and the flagged value (`regexes`), with
   `condition = "AND"`. Do not allowlist a crate, all of `docs/`, or a rule
   across the repository.

New test fixtures are better written so they do not look like secrets (build
the string at runtime, or use an obviously dummy value) than allowlisted.
