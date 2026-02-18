//! Agent Memory System
//!
//! This module provides memory capabilities for AI agents:
//! - Episodic memory for experiences and events
//! - Semantic memory for facts and knowledge
//! - Working memory for current context
//! - Memory retrieval with relevance scoring
//! - Memory consolidation and forgetting
//! - Context window management

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

/// Memory error types
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum MemoryError {
    /// Memory item not found
    #[error("Memory not found: {0}")]
    NotFound(String),
    /// Memory capacity exceeded
    #[error("Memory capacity exceeded: {0}")]
    CapacityExceeded(usize),
    /// Invalid memory type
    #[error("Invalid memory type: {0}")]
    InvalidType(String),
    /// Retrieval error
    #[error("Retrieval error: {0}")]
    RetrievalError(String),
    /// Encoding error
    #[error("Encoding error: {0}")]
    EncodingError(String),
}

/// Result type for memory operations
pub type MemoryResult<T> = Result<T, MemoryError>;

/// Memory item identifier
pub type MemoryId = String;

/// Type of memory
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MemoryType {
    /// Episodic memory (experiences, events)
    Episodic,
    /// Semantic memory (facts, knowledge)
    Semantic,
    /// Procedural memory (skills, how-to)
    Procedural,
    /// Working memory (current context)
    Working,
}

impl fmt::Display for MemoryType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Episodic => write!(f, "episodic"),
            Self::Semantic => write!(f, "semantic"),
            Self::Procedural => write!(f, "procedural"),
            Self::Working => write!(f, "working"),
        }
    }
}

/// Importance level for memory items
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
pub enum Importance {
    /// Low importance, may be forgotten quickly
    Low = 1,
    /// Normal importance
    #[default]
    Normal = 2,
    /// High importance, retained longer
    High = 3,
    /// Critical importance, never forgotten
    Critical = 4,
}

impl Importance {
    /// Get decay rate multiplier (higher = slower decay)
    pub fn decay_multiplier(&self) -> f64 {
        match self {
            Self::Low => 0.5,
            Self::Normal => 1.0,
            Self::High => 2.0,
            Self::Critical => f64::INFINITY,
        }
    }
}

/// An episodic memory (experience/event)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    /// Unique identifier
    pub id: MemoryId,
    /// Episode content/description
    pub content: String,
    /// When the episode occurred
    pub timestamp: SystemTime,
    /// Location/context where it happened
    pub context: String,
    /// Entities involved
    pub entities: Vec<String>,
    /// Emotional valence (-1.0 to 1.0)
    pub valence: f64,
    /// Importance level
    pub importance: Importance,
    /// Associated tags
    pub tags: Vec<String>,
    /// Access count
    pub access_count: u32,
    /// Last accessed time
    pub last_accessed: SystemTime,
}

impl Episode {
    /// Create a new episode
    pub fn new(id: &str, content: &str) -> Self {
        let now = SystemTime::now();
        Self {
            id: id.to_string(),
            content: content.to_string(),
            timestamp: now,
            context: String::new(),
            entities: Vec::new(),
            valence: 0.0,
            importance: Importance::Normal,
            tags: Vec::new(),
            access_count: 0,
            last_accessed: now,
        }
    }

    /// Set context
    pub fn with_context(mut self, context: &str) -> Self {
        self.context = context.to_string();
        self
    }

    /// Add an entity
    pub fn with_entity(mut self, entity: &str) -> Self {
        self.entities.push(entity.to_string());
        self
    }

    /// Set emotional valence
    pub fn with_valence(mut self, valence: f64) -> Self {
        self.valence = valence.clamp(-1.0, 1.0);
        self
    }

    /// Set importance
    pub fn with_importance(mut self, importance: Importance) -> Self {
        self.importance = importance;
        self
    }

    /// Add a tag
    pub fn with_tag(mut self, tag: &str) -> Self {
        self.tags.push(tag.to_string());
        self
    }

    /// Record an access
    pub fn record_access(&mut self) {
        self.access_count += 1;
        self.last_accessed = SystemTime::now();
    }

    /// Calculate memory strength (for forgetting curve)
    pub fn strength(&self) -> f64 {
        let base_strength = 1.0 + (1.0 + self.access_count as f64).ln();
        let age = self
            .last_accessed
            .elapsed()
            .unwrap_or(Duration::ZERO)
            .as_secs_f64();
        let decay_rate = 0.1 / self.importance.decay_multiplier();
        base_strength * (-decay_rate * age / 3600.0).exp()
    }
}

/// A semantic memory (fact/knowledge)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticFact {
    /// Unique identifier
    pub id: MemoryId,
    /// Subject
    pub subject: String,
    /// Relation/predicate
    pub relation: String,
    /// Object
    pub object: String,
    /// Confidence (0.0 to 1.0)
    pub confidence: f64,
    /// Source of the fact
    pub source: String,
    /// When learned
    pub learned_at: SystemTime,
    /// Importance
    pub importance: Importance,
    /// Access count
    pub access_count: u32,
}

impl SemanticFact {
    /// Create a new semantic fact
    pub fn new(id: &str, subject: &str, relation: &str, object: &str) -> Self {
        Self {
            id: id.to_string(),
            subject: subject.to_string(),
            relation: relation.to_string(),
            object: object.to_string(),
            confidence: 1.0,
            source: String::new(),
            learned_at: SystemTime::now(),
            importance: Importance::Normal,
            access_count: 0,
        }
    }

    /// Set confidence
    pub fn with_confidence(mut self, confidence: f64) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }

    /// Set source
    pub fn with_source(mut self, source: &str) -> Self {
        self.source = source.to_string();
        self
    }

    /// Set importance
    pub fn with_importance(mut self, importance: Importance) -> Self {
        self.importance = importance;
        self
    }

    /// Record an access
    pub fn record_access(&mut self) {
        self.access_count += 1;
    }

    /// Get as triple string
    pub fn as_triple(&self) -> String {
        format!("({}, {}, {})", self.subject, self.relation, self.object)
    }
}

/// A working memory item (current context)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkingItem {
    /// Unique identifier
    pub id: MemoryId,
    /// Content
    pub content: String,
    /// Role (system, user, assistant, tool)
    pub role: String,
    /// Token count estimate
    pub tokens: usize,
    /// Created time
    pub created_at: SystemTime,
    /// Pinned (won't be evicted)
    pub pinned: bool,
}

impl WorkingItem {
    /// Create a new working memory item
    pub fn new(id: &str, role: &str, content: &str) -> Self {
        // Rough token estimate: ~4 chars per token
        let tokens = content.len() / 4 + 1;
        Self {
            id: id.to_string(),
            content: content.to_string(),
            role: role.to_string(),
            tokens,
            created_at: SystemTime::now(),
            pinned: false,
        }
    }

    /// Pin the item
    pub fn pin(mut self) -> Self {
        self.pinned = true;
        self
    }

    /// Set token count
    pub fn with_tokens(mut self, tokens: usize) -> Self {
        self.tokens = tokens;
        self
    }
}

/// Episodic memory store
#[derive(Debug)]
pub struct EpisodicMemory {
    /// Episodes by ID
    episodes: HashMap<MemoryId, Episode>,
    /// Maximum capacity
    capacity: usize,
    /// Minimum strength threshold for retention
    strength_threshold: f64,
}

impl Default for EpisodicMemory {
    fn default() -> Self {
        Self::new(1000)
    }
}

impl EpisodicMemory {
    /// Create a new episodic memory
    pub fn new(capacity: usize) -> Self {
        Self {
            episodes: HashMap::new(),
            capacity,
            strength_threshold: 0.1,
        }
    }

    /// Set strength threshold
    pub fn with_strength_threshold(mut self, threshold: f64) -> Self {
        self.strength_threshold = threshold;
        self
    }

    /// Store an episode
    pub fn store(&mut self, episode: Episode) -> MemoryResult<()> {
        if self.episodes.len() >= self.capacity {
            self.consolidate();
            if self.episodes.len() >= self.capacity {
                return Err(MemoryError::CapacityExceeded(self.capacity));
            }
        }
        self.episodes.insert(episode.id.clone(), episode);
        Ok(())
    }

    /// Retrieve an episode by ID
    pub fn retrieve(&mut self, id: &str) -> Option<&Episode> {
        if let Some(episode) = self.episodes.get_mut(id) {
            episode.record_access();
        }
        self.episodes.get(id)
    }

    /// Search episodes by content
    pub fn search(&self, query: &str) -> Vec<&Episode> {
        let query_lower = query.to_lowercase();
        let mut results: Vec<_> = self
            .episodes
            .values()
            .filter(|e| e.content.to_lowercase().contains(&query_lower))
            .collect();
        results.sort_by(|a, b| b.strength().partial_cmp(&a.strength()).unwrap_or(std::cmp::Ordering::Equal));
        results
    }

    /// Search by tag
    pub fn search_by_tag(&self, tag: &str) -> Vec<&Episode> {
        self.episodes
            .values()
            .filter(|e| e.tags.iter().any(|t| t == tag))
            .collect()
    }

    /// Search by entity
    pub fn search_by_entity(&self, entity: &str) -> Vec<&Episode> {
        self.episodes
            .values()
            .filter(|e| e.entities.iter().any(|ent| ent == entity))
            .collect()
    }

    /// Get recent episodes
    pub fn recent(&self, count: usize) -> Vec<&Episode> {
        let mut episodes: Vec<_> = self.episodes.values().collect();
        episodes.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        episodes.into_iter().take(count).collect()
    }

    /// Consolidate memory (forget weak memories)
    pub fn consolidate(&mut self) {
        let threshold = self.strength_threshold;
        self.episodes
            .retain(|_, e| e.importance == Importance::Critical || e.strength() >= threshold);
    }

    /// Get count
    pub fn len(&self) -> usize {
        self.episodes.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.episodes.is_empty()
    }

    /// Remove an episode
    pub fn remove(&mut self, id: &str) -> Option<Episode> {
        self.episodes.remove(id)
    }

    /// Clear all episodes
    pub fn clear(&mut self) {
        self.episodes.clear();
    }
}

/// Semantic memory store
#[derive(Debug)]
pub struct SemanticMemory {
    /// Facts by ID
    facts: HashMap<MemoryId, SemanticFact>,
    /// Index by subject
    by_subject: HashMap<String, Vec<MemoryId>>,
    /// Index by relation
    by_relation: HashMap<String, Vec<MemoryId>>,
    /// Index by object
    by_object: HashMap<String, Vec<MemoryId>>,
    /// Maximum capacity
    capacity: usize,
}

impl Default for SemanticMemory {
    fn default() -> Self {
        Self::new(5000)
    }
}

impl SemanticMemory {
    /// Create a new semantic memory
    pub fn new(capacity: usize) -> Self {
        Self {
            facts: HashMap::new(),
            by_subject: HashMap::new(),
            by_relation: HashMap::new(),
            by_object: HashMap::new(),
            capacity,
        }
    }

    /// Store a fact
    pub fn store(&mut self, fact: SemanticFact) -> MemoryResult<()> {
        if self.facts.len() >= self.capacity {
            return Err(MemoryError::CapacityExceeded(self.capacity));
        }

        // Update indexes
        self.by_subject
            .entry(fact.subject.clone())
            .or_default()
            .push(fact.id.clone());
        self.by_relation
            .entry(fact.relation.clone())
            .or_default()
            .push(fact.id.clone());
        self.by_object
            .entry(fact.object.clone())
            .or_default()
            .push(fact.id.clone());

        self.facts.insert(fact.id.clone(), fact);
        Ok(())
    }

    /// Retrieve a fact by ID
    pub fn retrieve(&mut self, id: &str) -> Option<&SemanticFact> {
        if let Some(fact) = self.facts.get_mut(id) {
            fact.record_access();
        }
        self.facts.get(id)
    }

    /// Query facts by subject
    pub fn query_by_subject(&self, subject: &str) -> Vec<&SemanticFact> {
        self.by_subject
            .get(subject)
            .map(|ids| ids.iter().filter_map(|id| self.facts.get(id)).collect())
            .unwrap_or_default()
    }

    /// Query facts by relation
    pub fn query_by_relation(&self, relation: &str) -> Vec<&SemanticFact> {
        self.by_relation
            .get(relation)
            .map(|ids| ids.iter().filter_map(|id| self.facts.get(id)).collect())
            .unwrap_or_default()
    }

    /// Query facts by object
    pub fn query_by_object(&self, object: &str) -> Vec<&SemanticFact> {
        self.by_object
            .get(object)
            .map(|ids| ids.iter().filter_map(|id| self.facts.get(id)).collect())
            .unwrap_or_default()
    }

    /// Query by pattern (subject, relation, object - None for wildcard)
    pub fn query(
        &self,
        subject: Option<&str>,
        relation: Option<&str>,
        object: Option<&str>,
    ) -> Vec<&SemanticFact> {
        self.facts
            .values()
            .filter(|f| {
                subject.is_none_or(|s| f.subject == s)
                    && relation.is_none_or(|r| f.relation == r)
                    && object.is_none_or(|o| f.object == o)
            })
            .collect()
    }

    /// Get count
    pub fn len(&self) -> usize {
        self.facts.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }

    /// Remove a fact
    pub fn remove(&mut self, id: &str) -> Option<SemanticFact> {
        if let Some(fact) = self.facts.remove(id) {
            // Update indexes
            if let Some(ids) = self.by_subject.get_mut(&fact.subject) {
                ids.retain(|i| i != id);
            }
            if let Some(ids) = self.by_relation.get_mut(&fact.relation) {
                ids.retain(|i| i != id);
            }
            if let Some(ids) = self.by_object.get_mut(&fact.object) {
                ids.retain(|i| i != id);
            }
            Some(fact)
        } else {
            None
        }
    }

    /// Clear all facts
    pub fn clear(&mut self) {
        self.facts.clear();
        self.by_subject.clear();
        self.by_relation.clear();
        self.by_object.clear();
    }
}

/// Working memory (context window)
#[derive(Debug)]
pub struct WorkingMemory {
    /// Items in order
    items: VecDeque<WorkingItem>,
    /// Maximum token capacity
    max_tokens: usize,
    /// Current token count
    current_tokens: usize,
}

impl Default for WorkingMemory {
    fn default() -> Self {
        Self::new(4096)
    }
}

impl WorkingMemory {
    /// Create a new working memory
    pub fn new(max_tokens: usize) -> Self {
        Self {
            items: VecDeque::new(),
            max_tokens,
            current_tokens: 0,
        }
    }

    /// Add an item to working memory
    pub fn add(&mut self, item: WorkingItem) -> MemoryResult<()> {
        // Evict old items if needed (except pinned)
        while self.current_tokens + item.tokens > self.max_tokens {
            if let Some(pos) = self.items.iter().position(|i| !i.pinned) {
                let removed = self.items.remove(pos).expect("position from iter was valid");
                self.current_tokens -= removed.tokens;
            } else {
                return Err(MemoryError::CapacityExceeded(self.max_tokens));
            }
        }

        self.current_tokens += item.tokens;
        self.items.push_back(item);
        Ok(())
    }

    /// Get all items
    pub fn items(&self) -> impl Iterator<Item = &WorkingItem> {
        self.items.iter()
    }

    /// Get items by role
    pub fn items_by_role(&self, role: &str) -> Vec<&WorkingItem> {
        self.items.iter().filter(|i| i.role == role).collect()
    }

    /// Get recent items
    pub fn recent(&self, count: usize) -> Vec<&WorkingItem> {
        self.items.iter().rev().take(count).collect()
    }

    /// Get item count
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Get current token count
    pub fn token_count(&self) -> usize {
        self.current_tokens
    }

    /// Get available tokens
    pub fn available_tokens(&self) -> usize {
        self.max_tokens.saturating_sub(self.current_tokens)
    }

    /// Clear working memory (except pinned)
    pub fn clear(&mut self) {
        let pinned: VecDeque<_> = self.items.drain(..).filter(|i| i.pinned).collect();
        self.current_tokens = pinned.iter().map(|i| i.tokens).sum();
        self.items = pinned;
    }

    /// Clear all (including pinned)
    pub fn clear_all(&mut self) {
        self.items.clear();
        self.current_tokens = 0;
    }

    /// Remove item by ID
    pub fn remove(&mut self, id: &str) -> Option<WorkingItem> {
        if let Some(pos) = self.items.iter().position(|i| i.id == id) {
            let item = self.items.remove(pos)?;
            self.current_tokens -= item.tokens;
            Some(item)
        } else {
            None
        }
    }
}

/// Memory retrieval result
#[derive(Debug, Clone)]
pub struct RetrievalResult {
    /// Memory ID
    pub id: MemoryId,
    /// Memory type
    pub memory_type: MemoryType,
    /// Content summary
    pub content: String,
    /// Relevance score (0.0 to 1.0)
    pub relevance: f64,
    /// Recency score (0.0 to 1.0)
    pub recency: f64,
    /// Combined score
    pub score: f64,
}

impl RetrievalResult {
    /// Create a new retrieval result
    pub fn new(id: &str, memory_type: MemoryType, content: &str) -> Self {
        Self {
            id: id.to_string(),
            memory_type,
            content: content.to_string(),
            relevance: 0.0,
            recency: 0.0,
            score: 0.0,
        }
    }

    /// Set relevance
    pub fn with_relevance(mut self, relevance: f64) -> Self {
        self.relevance = relevance.clamp(0.0, 1.0);
        self.update_score();
        self
    }

    /// Set recency
    pub fn with_recency(mut self, recency: f64) -> Self {
        self.recency = recency.clamp(0.0, 1.0);
        self.update_score();
        self
    }

    /// Update combined score
    fn update_score(&mut self) {
        // Weighted combination: relevance is more important
        self.score = 0.7 * self.relevance + 0.3 * self.recency;
    }
}

/// Retrieval configuration
#[derive(Debug, Clone)]
pub struct RetrievalConfig {
    /// Maximum results to return
    pub max_results: usize,
    /// Minimum relevance threshold
    pub min_relevance: f64,
    /// Weight for relevance (vs recency)
    pub relevance_weight: f64,
    /// Memory types to search
    pub memory_types: Vec<MemoryType>,
}

impl Default for RetrievalConfig {
    fn default() -> Self {
        Self {
            max_results: 10,
            min_relevance: 0.1,
            relevance_weight: 0.7,
            memory_types: vec![MemoryType::Episodic, MemoryType::Semantic],
        }
    }
}

impl RetrievalConfig {
    /// Set max results
    pub fn with_max_results(mut self, max: usize) -> Self {
        self.max_results = max;
        self
    }

    /// Set min relevance
    pub fn with_min_relevance(mut self, min: f64) -> Self {
        self.min_relevance = min;
        self
    }

    /// Set memory types
    pub fn with_memory_types(mut self, types: Vec<MemoryType>) -> Self {
        self.memory_types = types;
        self
    }
}

/// Unified memory system
#[derive(Debug)]
pub struct MemorySystem {
    /// Episodic memory
    pub episodic: EpisodicMemory,
    /// Semantic memory
    pub semantic: SemanticMemory,
    /// Working memory
    pub working: WorkingMemory,
    /// Statistics
    stats: MemoryStats,
}

impl Default for MemorySystem {
    fn default() -> Self {
        Self::new()
    }
}

impl MemorySystem {
    /// Create a new memory system
    pub fn new() -> Self {
        Self {
            episodic: EpisodicMemory::default(),
            semantic: SemanticMemory::default(),
            working: WorkingMemory::default(),
            stats: MemoryStats::default(),
        }
    }

    /// Configure episodic memory capacity
    pub fn with_episodic_capacity(mut self, capacity: usize) -> Self {
        self.episodic = EpisodicMemory::new(capacity);
        self
    }

    /// Configure semantic memory capacity
    pub fn with_semantic_capacity(mut self, capacity: usize) -> Self {
        self.semantic = SemanticMemory::new(capacity);
        self
    }

    /// Configure working memory tokens
    pub fn with_working_tokens(mut self, tokens: usize) -> Self {
        self.working = WorkingMemory::new(tokens);
        self
    }

    /// Store an episode
    pub fn remember_episode(&mut self, episode: Episode) -> MemoryResult<()> {
        self.stats.episodes_stored += 1;
        self.episodic.store(episode)
    }

    /// Store a semantic fact
    pub fn remember_fact(&mut self, fact: SemanticFact) -> MemoryResult<()> {
        self.stats.facts_stored += 1;
        self.semantic.store(fact)
    }

    /// Add to working memory
    pub fn add_to_context(&mut self, item: WorkingItem) -> MemoryResult<()> {
        self.stats.context_additions += 1;
        self.working.add(item)
    }

    /// Retrieve memories by query
    pub fn retrieve(&mut self, query: &str, config: &RetrievalConfig) -> Vec<RetrievalResult> {
        self.stats.retrievals += 1;
        let mut results = Vec::new();
        let now = SystemTime::now();

        // Search episodic memory
        if config.memory_types.contains(&MemoryType::Episodic) {
            for episode in self.episodic.search(query) {
                let recency = Self::calculate_recency(episode.timestamp, now);
                let relevance = Self::calculate_text_relevance(query, &episode.content);

                if relevance >= config.min_relevance {
                    results.push(
                        RetrievalResult::new(&episode.id, MemoryType::Episodic, &episode.content)
                            .with_relevance(relevance)
                            .with_recency(recency),
                    );
                }
            }
        }

        // Search semantic memory
        if config.memory_types.contains(&MemoryType::Semantic) {
            let query_lower = query.to_lowercase();
            for fact in self.semantic.facts.values() {
                let triple = fact.as_triple();
                if triple.to_lowercase().contains(&query_lower) {
                    let recency = Self::calculate_recency(fact.learned_at, now);
                    let relevance = Self::calculate_text_relevance(query, &triple);

                    if relevance >= config.min_relevance {
                        results.push(
                            RetrievalResult::new(&fact.id, MemoryType::Semantic, &triple)
                                .with_relevance(relevance * fact.confidence)
                                .with_recency(recency),
                        );
                    }
                }
            }
        }

        // Sort by score and limit
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(config.max_results);

        results
    }

    /// Calculate recency score
    fn calculate_recency(timestamp: SystemTime, now: SystemTime) -> f64 {
        let age = now
            .duration_since(timestamp)
            .unwrap_or(Duration::ZERO)
            .as_secs_f64();
        // Decay over 24 hours
        (-age / 86400.0).exp()
    }

    /// Calculate text relevance (simple word overlap)
    fn calculate_text_relevance(query: &str, text: &str) -> f64 {
        let query_lower = query.to_lowercase();
        let query_words: Vec<_> = query_lower.split_whitespace().collect();
        let text_lower = text.to_lowercase();

        if query_words.is_empty() {
            return 0.0;
        }

        let matches = query_words
            .iter()
            .filter(|w| text_lower.contains(*w))
            .count();

        matches as f64 / query_words.len() as f64
    }

    /// Consolidate all memories
    pub fn consolidate(&mut self) {
        self.episodic.consolidate();
        self.stats.consolidations += 1;
    }

    /// Get memory statistics
    pub fn stats(&self) -> &MemoryStats {
        &self.stats
    }

    /// Get total memory count
    pub fn total_count(&self) -> usize {
        self.episodic.len() + self.semantic.len() + self.working.len()
    }
}

/// Memory statistics
#[derive(Debug, Clone, Default)]
pub struct MemoryStats {
    /// Episodes stored
    pub episodes_stored: u64,
    /// Facts stored
    pub facts_stored: u64,
    /// Context additions
    pub context_additions: u64,
    /// Retrievals performed
    pub retrievals: u64,
    /// Consolidations performed
    pub consolidations: u64,
}

/// Thread-safe shared memory system
#[derive(Debug, Clone)]
pub struct SharedMemory {
    inner: Arc<RwLock<MemorySystem>>,
}

impl Default for SharedMemory {
    fn default() -> Self {
        Self::new(MemorySystem::new())
    }
}

impl SharedMemory {
    /// Create a new shared memory
    pub fn new(system: MemorySystem) -> Self {
        Self {
            inner: Arc::new(RwLock::new(system)),
        }
    }

    /// Remember an episode
    pub fn remember_episode(&self, episode: Episode) -> MemoryResult<()> {
        self.inner.write().unwrap_or_else(|e| e.into_inner()).remember_episode(episode)
    }

    /// Remember a fact
    pub fn remember_fact(&self, fact: SemanticFact) -> MemoryResult<()> {
        self.inner.write().unwrap_or_else(|e| e.into_inner()).remember_fact(fact)
    }

    /// Add to context
    pub fn add_to_context(&self, item: WorkingItem) -> MemoryResult<()> {
        self.inner.write().unwrap_or_else(|e| e.into_inner()).add_to_context(item)
    }

    /// Retrieve memories
    pub fn retrieve(&self, query: &str, config: &RetrievalConfig) -> Vec<RetrievalResult> {
        self.inner.write().unwrap_or_else(|e| e.into_inner()).retrieve(query, config)
    }

    /// Consolidate
    pub fn consolidate(&self) {
        self.inner.write().unwrap_or_else(|e| e.into_inner()).consolidate();
    }

    /// Get episode count
    pub fn episode_count(&self) -> usize {
        self.inner.read().unwrap_or_else(|e| e.into_inner()).episodic.len()
    }

    /// Get fact count
    pub fn fact_count(&self) -> usize {
        self.inner.read().unwrap_or_else(|e| e.into_inner()).semantic.len()
    }

    /// Get working memory count
    pub fn working_count(&self) -> usize {
        self.inner.read().unwrap_or_else(|e| e.into_inner()).working.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_importance_decay() {
        assert!(Importance::Low.decay_multiplier() < Importance::Normal.decay_multiplier());
        assert!(Importance::Normal.decay_multiplier() < Importance::High.decay_multiplier());
        assert!(Importance::Critical.decay_multiplier().is_infinite());
    }

    #[test]
    fn test_episode_creation() {
        let episode = Episode::new("ep-1", "Met Alice at the park")
            .with_context("park")
            .with_entity("Alice")
            .with_valence(0.8)
            .with_importance(Importance::High)
            .with_tag("social");

        assert_eq!(episode.id, "ep-1");
        assert_eq!(episode.context, "park");
        assert!(episode.entities.contains(&"Alice".to_string()));
        assert_eq!(episode.valence, 0.8);
        assert_eq!(episode.importance, Importance::High);
        assert!(episode.tags.contains(&"social".to_string()));
    }

    #[test]
    fn test_episode_strength() {
        let episode = Episode::new("ep-1", "Test episode");
        let strength = episode.strength();
        assert!(strength > 0.0);
        assert!(strength <= 1.0);
    }

    #[test]
    fn test_episode_access() {
        let mut episode = Episode::new("ep-1", "Test");
        assert_eq!(episode.access_count, 0);
        episode.record_access();
        assert_eq!(episode.access_count, 1);
    }

    #[test]
    fn test_semantic_fact_creation() {
        let fact = SemanticFact::new("fact-1", "Paris", "is_capital_of", "France")
            .with_confidence(0.95)
            .with_source("Wikipedia")
            .with_importance(Importance::High);

        assert_eq!(fact.subject, "Paris");
        assert_eq!(fact.relation, "is_capital_of");
        assert_eq!(fact.object, "France");
        assert_eq!(fact.confidence, 0.95);
        assert_eq!(fact.source, "Wikipedia");
    }

    #[test]
    fn test_semantic_fact_triple() {
        let fact = SemanticFact::new("f1", "A", "rel", "B");
        assert_eq!(fact.as_triple(), "(A, rel, B)");
    }

    #[test]
    fn test_working_item_creation() {
        let item = WorkingItem::new("w-1", "user", "Hello, how are you?");
        assert_eq!(item.role, "user");
        assert!(item.tokens > 0);
        assert!(!item.pinned);
    }

    #[test]
    fn test_working_item_pin() {
        let item = WorkingItem::new("w-1", "system", "You are a helpful assistant").pin();
        assert!(item.pinned);
    }

    #[test]
    fn test_episodic_memory_store_retrieve() {
        let mut mem = EpisodicMemory::new(100);

        mem.store(Episode::new("ep-1", "First event")).unwrap();
        mem.store(Episode::new("ep-2", "Second event")).unwrap();

        assert_eq!(mem.len(), 2);

        let ep = mem.retrieve("ep-1");
        assert!(ep.is_some());
        assert_eq!(ep.unwrap().content, "First event");
    }

    #[test]
    fn test_episodic_memory_search() {
        let mut mem = EpisodicMemory::new(100);

        mem.store(Episode::new("ep-1", "Met Alice at the park"))
            .unwrap();
        mem.store(Episode::new("ep-2", "Had lunch with Bob"))
            .unwrap();
        mem.store(Episode::new("ep-3", "Alice called me")).unwrap();

        let results = mem.search("Alice");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_episodic_memory_search_by_tag() {
        let mut mem = EpisodicMemory::new(100);

        mem.store(Episode::new("ep-1", "Event 1").with_tag("work"))
            .unwrap();
        mem.store(Episode::new("ep-2", "Event 2").with_tag("personal"))
            .unwrap();
        mem.store(Episode::new("ep-3", "Event 3").with_tag("work"))
            .unwrap();

        let results = mem.search_by_tag("work");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_episodic_memory_recent() {
        let mut mem = EpisodicMemory::new(100);

        mem.store(Episode::new("ep-1", "First")).unwrap();
        mem.store(Episode::new("ep-2", "Second")).unwrap();
        mem.store(Episode::new("ep-3", "Third")).unwrap();

        let recent = mem.recent(2);
        assert_eq!(recent.len(), 2);
    }

    #[test]
    fn test_semantic_memory_store_query() {
        let mut mem = SemanticMemory::new(100);

        mem.store(SemanticFact::new("f1", "Paris", "capital_of", "France"))
            .unwrap();
        mem.store(SemanticFact::new("f2", "Berlin", "capital_of", "Germany"))
            .unwrap();
        mem.store(SemanticFact::new("f3", "France", "continent", "Europe"))
            .unwrap();

        assert_eq!(mem.len(), 3);

        let results = mem.query_by_subject("Paris");
        assert_eq!(results.len(), 1);

        let results = mem.query_by_relation("capital_of");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_semantic_memory_query_pattern() {
        let mut mem = SemanticMemory::new(100);

        mem.store(SemanticFact::new("f1", "A", "r1", "B")).unwrap();
        mem.store(SemanticFact::new("f2", "A", "r2", "C")).unwrap();
        mem.store(SemanticFact::new("f3", "B", "r1", "C")).unwrap();

        let results = mem.query(Some("A"), None, None);
        assert_eq!(results.len(), 2);

        let results = mem.query(None, Some("r1"), None);
        assert_eq!(results.len(), 2);

        let results = mem.query(Some("A"), Some("r1"), Some("B"));
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_working_memory_add() {
        let mut mem = WorkingMemory::new(1000);

        mem.add(WorkingItem::new("w-1", "user", "Hello")).unwrap();
        mem.add(WorkingItem::new("w-2", "assistant", "Hi there!"))
            .unwrap();

        assert_eq!(mem.len(), 2);
        assert!(mem.token_count() > 0);
    }

    #[test]
    fn test_working_memory_eviction() {
        let mut mem = WorkingMemory::new(20); // Very small

        mem.add(WorkingItem::new("w-1", "user", "Short")).unwrap();
        mem.add(WorkingItem::new("w-2", "user", "Another short message"))
            .unwrap();

        // Should evict first to make room
        assert!(mem.len() <= 2);
    }

    #[test]
    fn test_working_memory_pinned_no_evict() {
        let mut mem = WorkingMemory::new(50);

        mem.add(WorkingItem::new("w-1", "system", "System prompt").pin())
            .unwrap();
        mem.add(WorkingItem::new("w-2", "user", "User message"))
            .unwrap();

        // Pinned items should not be evicted
        let items: Vec<_> = mem.items().collect();
        assert!(items.iter().any(|i| i.id == "w-1"));
    }

    #[test]
    fn test_working_memory_clear() {
        let mut mem = WorkingMemory::new(1000);

        mem.add(WorkingItem::new("w-1", "system", "System").pin())
            .unwrap();
        mem.add(WorkingItem::new("w-2", "user", "User")).unwrap();

        mem.clear();
        // Only pinned should remain
        assert_eq!(mem.len(), 1);

        mem.clear_all();
        assert_eq!(mem.len(), 0);
    }

    #[test]
    fn test_retrieval_result() {
        let result = RetrievalResult::new("mem-1", MemoryType::Episodic, "Test content")
            .with_relevance(0.8)
            .with_recency(0.6);

        assert_eq!(result.relevance, 0.8);
        assert_eq!(result.recency, 0.6);
        assert!(result.score > 0.0);
    }

    #[test]
    fn test_retrieval_config() {
        let config = RetrievalConfig::default()
            .with_max_results(5)
            .with_min_relevance(0.5)
            .with_memory_types(vec![MemoryType::Episodic]);

        assert_eq!(config.max_results, 5);
        assert_eq!(config.min_relevance, 0.5);
        assert_eq!(config.memory_types, vec![MemoryType::Episodic]);
    }

    #[test]
    fn test_memory_system_creation() {
        let system = MemorySystem::new()
            .with_episodic_capacity(500)
            .with_semantic_capacity(2000)
            .with_working_tokens(8192);

        assert_eq!(system.episodic.capacity, 500);
        assert_eq!(system.semantic.capacity, 2000);
        assert_eq!(system.working.max_tokens, 8192);
    }

    #[test]
    fn test_memory_system_remember() {
        let mut system = MemorySystem::new();

        system
            .remember_episode(Episode::new("ep-1", "Test episode"))
            .unwrap();
        system
            .remember_fact(SemanticFact::new("f-1", "A", "r", "B"))
            .unwrap();
        system
            .add_to_context(WorkingItem::new("w-1", "user", "Hello"))
            .unwrap();

        assert_eq!(system.episodic.len(), 1);
        assert_eq!(system.semantic.len(), 1);
        assert_eq!(system.working.len(), 1);
    }

    #[test]
    fn test_memory_system_retrieve() {
        let mut system = MemorySystem::new();

        system
            .remember_episode(Episode::new("ep-1", "Met Alice at the park"))
            .unwrap();
        system
            .remember_episode(Episode::new("ep-2", "Had coffee"))
            .unwrap();
        system
            .remember_fact(SemanticFact::new("f-1", "Alice", "friend_of", "Bob"))
            .unwrap();

        let config = RetrievalConfig::default();
        let results = system.retrieve("Alice", &config);

        assert!(!results.is_empty());
    }

    #[test]
    fn test_memory_system_stats() {
        let mut system = MemorySystem::new();

        system
            .remember_episode(Episode::new("ep-1", "Test"))
            .unwrap();
        system
            .remember_fact(SemanticFact::new("f-1", "A", "r", "B"))
            .unwrap();

        let stats = system.stats();
        assert_eq!(stats.episodes_stored, 1);
        assert_eq!(stats.facts_stored, 1);
    }

    #[test]
    fn test_shared_memory() {
        let memory = SharedMemory::default();

        memory
            .remember_episode(Episode::new("ep-1", "Test"))
            .unwrap();
        memory
            .remember_fact(SemanticFact::new("f-1", "A", "r", "B"))
            .unwrap();

        assert_eq!(memory.episode_count(), 1);
        assert_eq!(memory.fact_count(), 1);
    }

    #[test]
    fn test_shared_memory_retrieve() {
        let memory = SharedMemory::default();

        memory
            .remember_episode(Episode::new("ep-1", "Important meeting"))
            .unwrap();

        let config = RetrievalConfig::default();
        let results = memory.retrieve("meeting", &config);

        assert!(!results.is_empty());
    }

    #[test]
    fn test_memory_error_display() {
        let err = MemoryError::NotFound("test".to_string());
        assert!(err.to_string().contains("not found"));

        let err = MemoryError::CapacityExceeded(100);
        assert!(err.to_string().contains("100"));
    }

    #[test]
    fn test_memory_type_display() {
        assert_eq!(MemoryType::Episodic.to_string(), "episodic");
        assert_eq!(MemoryType::Semantic.to_string(), "semantic");
        assert_eq!(MemoryType::Working.to_string(), "working");
    }

    #[test]
    fn test_text_relevance_calculation() {
        let relevance = MemorySystem::calculate_text_relevance("hello world", "Hello World Test");
        assert!(relevance > 0.5);

        let relevance = MemorySystem::calculate_text_relevance("foo bar", "completely different");
        assert_eq!(relevance, 0.0);
    }
}
