# Benchmark data

This branch stores benchmark history for
[`benchmark-action/github-action-benchmark`](https://github.com/benchmark-action/github-action-benchmark),
written by the Benchmarks workflow under `dev/bench/`.

It is an orphan branch: it shares no history with `master` and contains no
source code.

**Everything here is published.** GitHub Pages serves this branch at
<https://nervosys.github.io/HyperMachine/>, so the chart of stored
measurements is public at <https://nervosys.github.io/HyperMachine/dev/bench/>.
An earlier version of this file said the opposite; that was wrong, and this
page was public the whole time it said so.

Only pushes to `master` add measurements. Pull requests still run the
benchmarks and still receive a comparison comment, but they no longer record
themselves here — while they did, most of the stored history came from
unmerged branches, and a run comparing itself against "the previous
measurement" was usually comparing against someone else's pull request.

Do not protect this branch. The action commits to it directly, and a ruleset
requiring pull requests makes it fail with "Repository rule violations found".
