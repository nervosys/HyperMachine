//! Every TOML block in the configuration documentation must be config this
//! build can actually read.
//!
//! # Why this is a test rather than a review
//!
//! Three pages documented configuration that does not exist. Not loosely: the
//! deployment guide's main example had one key out of six that the build
//! reads, and the getting-started page named a file location that is never
//! opened, thirteen sections that do not exist, and six environment variables
//! out of seven that nothing looks up. The access-control page documented
//! per-key permissions, so an operator could believe they had issued a
//! read-only key — and following it produced a server with no API-key
//! authentication at all, because the list the middleware reads stayed empty.
//!
//! None of that fails anything. The schema is `#[serde(default)]` with no
//! `deny_unknown_fields`, which is right for a config shared between versions
//! and is also why a wrong key is silent. Documentation drifted from the
//! schema for as long as nobody compared them by hand.
//!
//! So they are compared here, on the only authority there is:
//! [`ConfigFile::unknown_keys`], the same check `hv2 config check` runs.
//!
//! # Adding a page
//!
//! Put it in `PAGES`. The contract is narrow on purpose: *every* fenced
//! ```` ```toml ```` block in a listed page must be config this build reads.
//! A page that wants to show configuration that does not exist — a roadmap, a
//! migration note — must say so in prose rather than in a block someone can
//! copy, because a block someone can copy is the failure this is about.

use hv2_api::config::ConfigFile;
use std::path::{Path, PathBuf};

/// Documentation pages whose TOML blocks describe `hv2.toml`.
const PAGES: &[&str] = &[
    "docs/DEPLOYMENT_GUIDE.md",
    "docs/src/getting-started/configuration.md",
    "docs/src/security/access-control.md",
];

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is crates/hv2-api.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .to_path_buf()
}

/// The contents of every ```` ```toml ```` fenced block, in order.
///
/// Deliberately simple-minded: a line that is exactly the opening fence starts
/// a block and the next line that is exactly ``` ends it. Anything cleverer
/// would be a markdown parser, and the blocks this has to find are the ones a
/// reader can copy, which are always plainly fenced.
fn toml_blocks(markdown: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current: Option<Vec<&str>> = None;
    for line in markdown.lines() {
        match current {
            None => {
                if line.trim_end() == "```toml" {
                    current = Some(Vec::new());
                }
            }
            Some(ref mut lines) => {
                if line.trim_end() == "```" {
                    blocks.push(lines.join("\n"));
                    current = None;
                } else {
                    lines.push(line);
                }
            }
        }
    }
    blocks
}

#[test]
fn every_documented_config_block_is_config_this_build_reads() {
    let root = repo_root();
    let mut failures = Vec::new();

    for page in PAGES {
        let path = root.join(page);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{page}: {e} (listed in PAGES; rename or remove it there)"));

        let blocks = toml_blocks(&text);
        assert!(
            !blocks.is_empty(),
            "{page} has no ```toml blocks. If its configuration examples moved, \
             update PAGES -- a page that silently stops being checked is how this \
             drifted in the first place."
        );

        for (i, block) in blocks.iter().enumerate() {
            match ConfigFile::unknown_keys(block) {
                Ok(unknown) if unknown.is_empty() => {}
                Ok(unknown) => failures.push(format!(
                    "{page} block {i}: {} key(s) this build does not read: {}",
                    unknown.len(),
                    unknown.join(", ")
                )),
                Err(e) => failures.push(format!("{page} block {i}: not valid TOML: {e}")),
            }
        }
    }

    assert!(
        failures.is_empty(),
        "documentation describes configuration that does nothing:\n  {}\n\n\
         Either correct the block to keys the schema reads (`hv2 config init` \
         lists them all), or move the example into prose if it is describing \
         something unimplemented.",
        failures.join("\n  ")
    );
}

/// The scanner has to find the blocks, or the test above passes by looking at
/// nothing — which is the shape of failure it exists to prevent.
#[test]
fn the_block_scanner_finds_blocks() {
    let md = "intro\n\n```toml\n[server]\nhost = \"x\"\n```\n\ntext\n\n```bash\nnot this\n```\n\n```toml\n[runtime]\n```\n";
    let blocks = toml_blocks(md);
    assert_eq!(blocks.len(), 2, "found: {blocks:?}");
    assert_eq!(blocks[0], "[server]\nhost = \"x\"");
    assert_eq!(blocks[1], "[runtime]");
}
