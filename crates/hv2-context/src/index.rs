//! What is left behind when something leaves the view.
//!
//! # A headline is a pointer, not a summary
//!
//! The distinction is the entire safety property. A summary *replaces* what it
//! describes: once the turns are gone, the summary is all there is, and every
//! detail it dropped is unrecoverable. A headline sits beside what it
//! describes and carries its address, so it is a table of contents -- wrong or
//! thin is survivable, because the events it points at are still there and
//! still exact.
//!
//! So every entry here anchors a [`Seq`] span, and nothing here is ever the
//! only copy of anything.
//!
//! # Why it is tiered
//!
//! One flat list of headlines grows without limit, which puts the index on the
//! same path as the context it was meant to relieve. Tiers keep recent history
//! fine-grained and let distant history coarsen: each tier holds at most `k`
//! blocks, and when one overflows, all but its newest block collapse to a line
//! apiece and merge upwards. After `n` evictions that is `O(k log_k n)` blocks
//! rather than `O(n)`.

use serde::{Deserialize, Serialize};

use crate::log::Seq;

/// Blocks per tier before it rolls up.
///
/// Small enough that the index stays short, large enough that recent work
/// keeps its detail for a while.
pub const DEFAULT_TIER_WIDTH: usize = 4;

/// How a piece of work ended.
///
/// `InProgress` exists because eviction does not wait for tasks to finish, and
/// an index that reported everything as done would be lying about the one
/// thing an agent most needs to know when it comes back to a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Finished, and the state line says what came of it.
    Done,
    /// Attempted and did not work.
    Failed,
    /// Deliberately dropped.
    Abandoned,
    /// Still open when this span left the view.
    InProgress,
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Abandoned => "abandoned",
            Self::InProgress => "in progress",
        };
        f.write_str(name)
    }
}

/// What an evicted span was about, and where to read it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Headline {
    /// What was being attempted.
    pub task: String,
    /// What is known to be true afterwards -- checked, not hoped.
    pub state: String,
    /// What the next step was going to be.
    pub next_action: String,
    /// How it ended.
    pub status: Status,
    /// First address of the span this describes.
    pub from: Seq,
    /// Last address of the span this describes.
    pub to: Seq,
}

impl Headline {
    /// A headline over `from..=to`.
    pub fn new(
        task: impl Into<String>,
        state: impl Into<String>,
        next_action: impl Into<String>,
        status: Status,
        from: Seq,
        to: Seq,
    ) -> Self {
        Self {
            task: task.into(),
            state: state.into(),
            next_action: next_action.into(),
            status,
            from,
            to,
        }
    }

    /// The one-line form used once a block has been rolled up.
    pub fn to_line(&self) -> String {
        format!("{}..{} {} [{}]", self.from, self.to, self.task, self.status)
    }
}

/// An entry in a block: either the whole headline or the line it collapsed to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "detail", rename_all = "snake_case")]
pub enum IndexEntry {
    /// Kept in full, because it is recent enough to still matter in detail.
    Detailed(Headline),
    /// Collapsed during a roll-up. Still carries its span.
    Line {
        /// The one-line form.
        text: String,
        /// First address covered.
        from: Seq,
        /// Last address covered.
        to: Seq,
    },
}

impl IndexEntry {
    /// The addresses this entry covers.
    pub fn span(&self) -> (Seq, Seq) {
        match self {
            Self::Detailed(headline) => (headline.from, headline.to),
            Self::Line { from, to, .. } => (*from, *to),
        }
    }

    /// Whether this entry covers `seq`.
    pub fn covers(&self, seq: Seq) -> bool {
        let (from, to) = self.span();
        from <= seq && seq <= to
    }

    /// Collapse to a line, keeping the span.
    fn collapse(&self) -> Self {
        match self {
            Self::Detailed(headline) => Self::Line {
                text: headline.to_line(),
                from: headline.from,
                to: headline.to,
            },
            line => line.clone(),
        }
    }

    fn render(&self) -> String {
        match self {
            Self::Detailed(headline) => format!(
                "{}..{} {} [{}] state: {} next: {}",
                headline.from,
                headline.to,
                headline.task,
                headline.status,
                headline.state,
                headline.next_action
            ),
            Self::Line { text, .. } => text.clone(),
        }
    }
}

/// One or more entries that moved together.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexBlock {
    /// The entries, oldest first.
    pub entries: Vec<IndexEntry>,
}

impl IndexBlock {
    /// A block holding a single headline.
    pub fn single(headline: Headline) -> Self {
        Self {
            entries: vec![IndexEntry::Detailed(headline)],
        }
    }

    /// The addresses this block covers, or `None` if it is empty.
    pub fn span(&self) -> Option<(Seq, Seq)> {
        let first = self.entries.first()?.span().0;
        let last = self.entries.iter().map(|e| e.span().1).max()?;
        Some((first, last))
    }
}

/// The tiered index of everything that has left the view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvictionIndex {
    /// Tier 0 is the most recent and most detailed.
    tiers: Vec<Vec<IndexBlock>>,
    width: usize,
}

impl Default for EvictionIndex {
    fn default() -> Self {
        Self::new(DEFAULT_TIER_WIDTH)
    }
}

impl EvictionIndex {
    /// An empty index whose tiers hold `width` blocks each.
    ///
    /// A width below 2 would collapse every block on arrival, leaving no tier
    /// with detail, so it is raised to 2.
    pub fn new(width: usize) -> Self {
        Self {
            tiers: Vec::new(),
            width: width.max(2),
        }
    }

    /// Record a headline for a span that has just left the view.
    pub fn push(&mut self, headline: Headline) {
        self.push_block(0, IndexBlock::single(headline));
    }

    fn push_block(&mut self, tier: usize, block: IndexBlock) {
        while self.tiers.len() <= tier {
            self.tiers.push(Vec::new());
        }
        self.tiers[tier].push(block);

        if self.tiers[tier].len() <= self.width {
            return;
        }

        // The newest block keeps its detail; everything older in this tier
        // collapses to one line apiece and moves up as a single block. The
        // spans travel with them, so nothing stops being reachable -- it only
        // stops being described at length.
        let newest = self.tiers[tier].pop().expect("just pushed");
        let older: Vec<IndexBlock> = std::mem::take(&mut self.tiers[tier]);
        self.tiers[tier].push(newest);

        let merged = IndexBlock {
            entries: older
                .iter()
                .flat_map(|block| block.entries.iter())
                .map(IndexEntry::collapse)
                .collect(),
        };
        if !merged.entries.is_empty() {
            self.push_block(tier + 1, merged);
        }
    }

    /// How many blocks the index holds across all tiers.
    pub fn block_count(&self) -> usize {
        self.tiers.iter().map(Vec::len).sum()
    }

    /// How many tiers exist.
    pub fn tier_count(&self) -> usize {
        self.tiers.len()
    }

    /// Whether anything has been evicted.
    pub fn is_empty(&self) -> bool {
        self.block_count() == 0
    }

    /// Every entry, most detailed tier first.
    pub fn entries(&self) -> impl Iterator<Item = &IndexEntry> {
        self.tiers
            .iter()
            .flat_map(|tier| tier.iter())
            .flat_map(|block| block.entries.iter())
    }

    /// The entry covering `seq`, if the index has one.
    ///
    /// This is the navigation the index exists for: an agent that knows
    /// roughly when something happened finds the entry, reads the span off it,
    /// and expands that range directly instead of searching the whole log.
    pub fn locate(&self, seq: Seq) -> Option<&IndexEntry> {
        self.entries().find(|entry| entry.covers(seq))
    }

    /// The index as the text a model reads, newest tier first.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for (level, tier) in self.tiers.iter().enumerate() {
            if tier.is_empty() {
                continue;
            }
            out.push_str(&format!("tier {level}:\n"));
            for block in tier {
                for entry in &block.entries {
                    out.push_str("  ");
                    out.push_str(&entry.render());
                    out.push('\n');
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headline(n: u64) -> Headline {
        Headline::new(
            format!("task {n}"),
            format!("state {n}"),
            format!("next {n}"),
            Status::Done,
            Seq(n * 10),
            Seq(n * 10 + 9),
        )
    }

    #[test]
    fn a_tier_never_holds_more_than_its_width() {
        let mut index = EvictionIndex::new(3);
        for n in 1..=40 {
            index.push(headline(n));
        }
        for level in 0..index.tier_count() {
            let count = index.tiers[level].len();
            assert!(count <= 3, "tier {level} holds {count} blocks");
        }
    }

    #[test]
    fn the_index_grows_logarithmically_rather_than_linearly() {
        // The point of the tiers. A flat list would be 200 blocks here, which
        // puts the index on the same path as the context it was relieving.
        let mut index = EvictionIndex::new(4);
        for n in 1..=200 {
            index.push(headline(n));
        }
        assert!(
            index.block_count() <= 20,
            "index holds {} blocks after 200 evictions",
            index.block_count()
        );
    }

    #[test]
    fn everything_evicted_is_still_addressable() {
        // The invariant the whole design rests on: rolling up loses detail and
        // never loses the address.
        let mut index = EvictionIndex::new(3);
        for n in 1..=50 {
            index.push(headline(n));
        }

        for n in 1..=50u64 {
            let probe = Seq(n * 10 + 4);
            let entry = index
                .locate(probe)
                .unwrap_or_else(|| panic!("{probe} is no longer reachable"));
            let (from, to) = entry.span();
            assert!(from <= probe && probe <= to);
        }
    }

    #[test]
    fn recent_history_keeps_its_detail_and_distant_history_does_not() {
        let mut index = EvictionIndex::new(3);
        for n in 1..=30 {
            index.push(headline(n));
        }

        let newest = index.locate(Seq(30 * 10 + 1)).unwrap();
        assert!(
            matches!(newest, IndexEntry::Detailed(_)),
            "the most recent eviction should still say what state it left things in"
        );

        let oldest = index.locate(Seq(14)).unwrap();
        assert!(
            matches!(oldest, IndexEntry::Line { .. }),
            "distant history should have collapsed"
        );
    }

    #[test]
    fn a_collapsed_entry_still_says_what_it_was_and_how_it_ended() {
        let mut index = EvictionIndex::new(2);
        index.push(Headline::new(
            "wire the vsock device",
            "queue reads bounded",
            "boot a guest against it",
            Status::InProgress,
            Seq(1),
            Seq(9),
        ));
        for n in 2..=10 {
            index.push(headline(n));
        }

        let entry = index.locate(Seq(5)).unwrap();
        let IndexEntry::Line { ref text, .. } = entry else {
            panic!("expected a collapsed entry, got {entry:?}");
        };
        assert!(text.contains("wire the vsock device"), "got: {text}");
        assert!(
            text.contains("in progress"),
            "an unfinished task must not read as finished: {text}"
        );
    }

    #[test]
    fn a_width_below_two_is_raised_rather_than_collapsing_everything() {
        let mut index = EvictionIndex::new(0);
        index.push(headline(1));
        assert!(matches!(
            index.locate(Seq(10)),
            Some(IndexEntry::Detailed(_))
        ));
    }

    #[test]
    fn an_empty_index_renders_to_nothing() {
        assert!(EvictionIndex::default().render().is_empty());
        assert!(EvictionIndex::default().is_empty());
    }
}
