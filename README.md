# Benchmark data

This branch stores benchmark history for
[`benchmark-action/github-action-benchmark`](https://github.com/benchmark-action/github-action-benchmark),
written by the Benchmarks workflow under `dev/bench/`.

It is an orphan branch: it shares no history with `master` and contains no
source code. Nothing here is published — GitHub Pages is not enabled for this
repository, and this branch exists only because the action stores its data by
committing to a branch of this name.

Do not protect this branch. The action commits to it directly, and a ruleset
requiring pull requests makes it fail with "Repository rule violations found".
