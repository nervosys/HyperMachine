//! Agent Reasoning and Decision Making
//!
//! This module provides reasoning and decision-making capabilities for AI agents:
//! - Inference engine with rule-based reasoning
//! - Fact database with truth maintenance
//! - Goal-driven planning and decomposition
//! - Decision trees and utility-based decisions
//! - Belief-desire-intention (BDI) architecture
//! - Knowledge representation with triples

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

/// Reasoning error types
#[derive(Debug, Clone, PartialEq)]
pub enum ReasoningError {
    /// Fact already exists
    FactExists(String),
    /// Fact not found
    FactNotFound(String),
    /// Rule already exists
    RuleExists(String),
    /// Rule not found
    RuleNotFound(String),
    /// Invalid rule definition
    InvalidRule(String),
    /// Goal not achievable
    GoalUnreachable(String),
    /// Inference cycle detected
    CycleDetected(String),
    /// Maximum inference depth exceeded
    MaxDepthExceeded(u32),
    /// Invalid belief
    InvalidBelief(String),
    /// Decision error
    DecisionError(String),
}

impl fmt::Display for ReasoningError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FactExists(s) => write!(f, "Fact already exists: {}", s),
            Self::FactNotFound(s) => write!(f, "Fact not found: {}", s),
            Self::RuleExists(s) => write!(f, "Rule already exists: {}", s),
            Self::RuleNotFound(s) => write!(f, "Rule not found: {}", s),
            Self::InvalidRule(s) => write!(f, "Invalid rule: {}", s),
            Self::GoalUnreachable(s) => write!(f, "Goal unreachable: {}", s),
            Self::CycleDetected(s) => write!(f, "Cycle detected: {}", s),
            Self::MaxDepthExceeded(d) => write!(f, "Max depth exceeded: {}", d),
            Self::InvalidBelief(s) => write!(f, "Invalid belief: {}", s),
            Self::DecisionError(s) => write!(f, "Decision error: {}", s),
        }
    }
}

impl std::error::Error for ReasoningError {}

/// Result type for reasoning operations
pub type ReasoningResult<T> = Result<T, ReasoningError>;

/// Truth value for facts
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum TruthValue {
    /// Definitely true
    True,
    /// Definitely false
    False,
    /// Unknown
    #[default]
    Unknown,
    /// Probably true (with confidence)
    Probable(f64),
}

impl TruthValue {
    /// Check if truth value is true or probably true
    pub fn is_true(&self) -> bool {
        match self {
            Self::True => true,
            Self::Probable(p) => *p > 0.5,
            _ => false,
        }
    }

    /// Get confidence level (0.0 to 1.0)
    pub fn confidence(&self) -> f64 {
        match self {
            Self::True => 1.0,
            Self::False => 0.0,
            Self::Unknown => 0.5,
            Self::Probable(p) => *p,
        }
    }

    /// Combine with another truth value (AND)
    pub fn and(&self, other: &Self) -> Self {
        match (self, other) {
            (Self::False, _) | (_, Self::False) => Self::False,
            (Self::True, Self::True) => Self::True,
            (Self::Probable(p1), Self::Probable(p2)) => Self::Probable(p1 * p2),
            (Self::Probable(p), Self::True) | (Self::True, Self::Probable(p)) => Self::Probable(*p),
            _ => Self::Unknown,
        }
    }

    /// Combine with another truth value (OR)
    pub fn or(&self, other: &Self) -> Self {
        match (self, other) {
            (Self::True, _) | (_, Self::True) => Self::True,
            (Self::False, Self::False) => Self::False,
            (Self::Probable(p1), Self::Probable(p2)) => Self::Probable(1.0 - (1.0 - p1) * (1.0 - p2)),
            (Self::Probable(p), Self::False) | (Self::False, Self::Probable(p)) => Self::Probable(*p),
            _ => Self::Unknown,
        }
    }

    /// Negate the truth value
    pub fn not(&self) -> Self {
        match self {
            Self::True => Self::False,
            Self::False => Self::True,
            Self::Probable(p) => Self::Probable(1.0 - p),
            Self::Unknown => Self::Unknown,
        }
    }
}

/// A fact in the knowledge base
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fact {
    /// Fact identifier
    pub id: String,
    /// Subject of the fact
    pub subject: String,
    /// Predicate (relationship)
    pub predicate: String,
    /// Object of the fact
    pub object: String,
    /// Truth value
    pub truth: TruthValue,
    /// Creation time
    pub created_at: SystemTime,
    /// Source of the fact
    pub source: FactSource,
    /// Metadata
    pub metadata: HashMap<String, String>,
}

impl Fact {
    /// Create a new fact
    pub fn new(subject: &str, predicate: &str, object: &str) -> Self {
        Self {
            id: format!("{}-{}-{}", subject, predicate, object),
            subject: subject.to_string(),
            predicate: predicate.to_string(),
            object: object.to_string(),
            truth: TruthValue::True,
            created_at: SystemTime::now(),
            source: FactSource::Asserted,
            metadata: HashMap::new(),
        }
    }

    /// Set truth value
    pub fn with_truth(mut self, truth: TruthValue) -> Self {
        self.truth = truth;
        self
    }

    /// Set source
    pub fn with_source(mut self, source: FactSource) -> Self {
        self.source = source;
        self
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: &str, value: &str) -> Self {
        self.metadata.insert(key.to_string(), value.to_string());
        self
    }

    /// Check if this fact matches a pattern
    pub fn matches(&self, subject: Option<&str>, predicate: Option<&str>, object: Option<&str>) -> bool {
        let subj_match = subject.map_or(true, |s| self.subject == s);
        let pred_match = predicate.map_or(true, |p| self.predicate == p);
        let obj_match = object.map_or(true, |o| self.object == o);
        subj_match && pred_match && obj_match
    }
}

/// Source of a fact
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FactSource {
    /// Directly asserted
    Asserted,
    /// Derived from rules
    Derived(String),
    /// Observed from environment
    Observed,
    /// From external source
    External(String),
}

/// A rule for inference
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    /// Rule identifier
    pub id: String,
    /// Rule name
    pub name: String,
    /// Conditions (antecedent patterns)
    pub conditions: Vec<FactPattern>,
    /// Conclusions (consequent facts)
    pub conclusions: Vec<FactPattern>,
    /// Rule priority (higher = more important)
    pub priority: i32,
    /// Whether rule is enabled
    pub enabled: bool,
}

impl Rule {
    /// Create a new rule
    pub fn new(id: &str, name: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            conditions: Vec::new(),
            conclusions: Vec::new(),
            priority: 0,
            enabled: true,
        }
    }

    /// Add a condition
    pub fn with_condition(mut self, pattern: FactPattern) -> Self {
        self.conditions.push(pattern);
        self
    }

    /// Add a conclusion
    pub fn with_conclusion(mut self, pattern: FactPattern) -> Self {
        self.conclusions.push(pattern);
        self
    }

    /// Set priority
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// Enable or disable
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// Pattern for matching facts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactPattern {
    /// Subject pattern (None = wildcard)
    pub subject: Option<String>,
    /// Predicate pattern (None = wildcard)
    pub predicate: Option<String>,
    /// Object pattern (None = wildcard)
    pub object: Option<String>,
    /// Variable bindings (e.g., "?x" -> position)
    pub variables: HashMap<String, PatternPosition>,
}

impl FactPattern {
    /// Create a pattern with all wildcards
    pub fn any() -> Self {
        Self {
            subject: None,
            predicate: None,
            object: None,
            variables: HashMap::new(),
        }
    }

    /// Create a pattern with specific subject
    pub fn with_subject(mut self, subject: &str) -> Self {
        self.subject = Some(subject.to_string());
        self
    }

    /// Create a pattern with specific predicate
    pub fn with_predicate(mut self, predicate: &str) -> Self {
        self.predicate = Some(predicate.to_string());
        self
    }

    /// Create a pattern with specific object
    pub fn with_object(mut self, object: &str) -> Self {
        self.object = Some(object.to_string());
        self
    }

    /// Add a variable binding
    pub fn with_variable(mut self, name: &str, position: PatternPosition) -> Self {
        self.variables.insert(name.to_string(), position);
        self
    }

    /// Match against a fact and return bindings
    pub fn match_fact(&self, fact: &Fact) -> Option<HashMap<String, String>> {
        // Check fixed parts
        if let Some(ref s) = self.subject {
            if s != &fact.subject && !s.starts_with('?') {
                return None;
            }
        }
        if let Some(ref p) = self.predicate {
            if p != &fact.predicate && !p.starts_with('?') {
                return None;
            }
        }
        if let Some(ref o) = self.object {
            if o != &fact.object && !o.starts_with('?') {
                return None;
            }
        }

        // Extract variable bindings
        let mut bindings = HashMap::new();
        for (var_name, position) in &self.variables {
            let value = match position {
                PatternPosition::Subject => &fact.subject,
                PatternPosition::Predicate => &fact.predicate,
                PatternPosition::Object => &fact.object,
            };
            bindings.insert(var_name.clone(), value.clone());
        }

        Some(bindings)
    }
}

/// Position in a fact pattern
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PatternPosition {
    /// Subject position
    Subject,
    /// Predicate position
    Predicate,
    /// Object position
    Object,
}

/// Knowledge base for facts
#[derive(Debug)]
pub struct KnowledgeBase {
    /// All facts
    facts: HashMap<String, Fact>,
    /// Index by subject
    by_subject: HashMap<String, HashSet<String>>,
    /// Index by predicate
    by_predicate: HashMap<String, HashSet<String>>,
    /// Index by object
    by_object: HashMap<String, HashSet<String>>,
}

impl Default for KnowledgeBase {
    fn default() -> Self {
        Self::new()
    }
}

impl KnowledgeBase {
    /// Create a new knowledge base
    pub fn new() -> Self {
        Self {
            facts: HashMap::new(),
            by_subject: HashMap::new(),
            by_predicate: HashMap::new(),
            by_object: HashMap::new(),
        }
    }

    /// Add a fact
    pub fn add(&mut self, fact: Fact) -> ReasoningResult<()> {
        if self.facts.contains_key(&fact.id) {
            return Err(ReasoningError::FactExists(fact.id));
        }

        // Update indexes
        self.by_subject
            .entry(fact.subject.clone())
            .or_default()
            .insert(fact.id.clone());
        self.by_predicate
            .entry(fact.predicate.clone())
            .or_default()
            .insert(fact.id.clone());
        self.by_object
            .entry(fact.object.clone())
            .or_default()
            .insert(fact.id.clone());

        self.facts.insert(fact.id.clone(), fact);
        Ok(())
    }

    /// Remove a fact
    pub fn remove(&mut self, id: &str) -> ReasoningResult<Fact> {
        let fact = self.facts.remove(id)
            .ok_or_else(|| ReasoningError::FactNotFound(id.to_string()))?;

        // Update indexes
        if let Some(set) = self.by_subject.get_mut(&fact.subject) {
            set.remove(id);
        }
        if let Some(set) = self.by_predicate.get_mut(&fact.predicate) {
            set.remove(id);
        }
        if let Some(set) = self.by_object.get_mut(&fact.object) {
            set.remove(id);
        }

        Ok(fact)
    }

    /// Get a fact by ID
    pub fn get(&self, id: &str) -> Option<&Fact> {
        self.facts.get(id)
    }

    /// Query facts by pattern
    pub fn query(
        &self,
        subject: Option<&str>,
        predicate: Option<&str>,
        object: Option<&str>,
    ) -> Vec<&Fact> {
        // Use most selective index
        let candidate_ids: HashSet<String> = if let Some(s) = subject {
            self.by_subject.get(s).cloned().unwrap_or_default()
        } else if let Some(p) = predicate {
            self.by_predicate.get(p).cloned().unwrap_or_default()
        } else if let Some(o) = object {
            self.by_object.get(o).cloned().unwrap_or_default()
        } else {
            self.facts.keys().cloned().collect()
        };

        candidate_ids
            .iter()
            .filter_map(|id| self.facts.get(id))
            .filter(|f| f.matches(subject, predicate, object))
            .collect()
    }

    /// Get fact count
    pub fn len(&self) -> usize {
        self.facts.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }

    /// Get all facts
    pub fn all_facts(&self) -> Vec<&Fact> {
        self.facts.values().collect()
    }

    /// Clear all facts
    pub fn clear(&mut self) {
        self.facts.clear();
        self.by_subject.clear();
        self.by_predicate.clear();
        self.by_object.clear();
    }
}

/// Inference engine for rule-based reasoning
#[derive(Debug)]
pub struct InferenceEngine {
    /// Knowledge base
    knowledge_base: KnowledgeBase,
    /// Rules
    rules: HashMap<String, Rule>,
    /// Maximum inference depth
    max_depth: u32,
    /// Derived facts in current cycle
    derived_facts: Vec<Fact>,
}

impl Default for InferenceEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl InferenceEngine {
    /// Create a new inference engine
    pub fn new() -> Self {
        Self {
            knowledge_base: KnowledgeBase::new(),
            rules: HashMap::new(),
            max_depth: 10,
            derived_facts: Vec::new(),
        }
    }

    /// Set maximum inference depth
    pub fn with_max_depth(mut self, depth: u32) -> Self {
        self.max_depth = depth;
        self
    }

    /// Assert a fact
    pub fn assert_fact(&mut self, fact: Fact) -> ReasoningResult<()> {
        self.knowledge_base.add(fact)
    }

    /// Retract a fact
    pub fn retract_fact(&mut self, id: &str) -> ReasoningResult<Fact> {
        self.knowledge_base.remove(id)
    }

    /// Add a rule
    pub fn add_rule(&mut self, rule: Rule) -> ReasoningResult<()> {
        if self.rules.contains_key(&rule.id) {
            return Err(ReasoningError::RuleExists(rule.id));
        }
        self.rules.insert(rule.id.clone(), rule);
        Ok(())
    }

    /// Remove a rule
    pub fn remove_rule(&mut self, id: &str) -> ReasoningResult<Rule> {
        self.rules
            .remove(id)
            .ok_or_else(|| ReasoningError::RuleNotFound(id.to_string()))
    }

    /// Query the knowledge base
    pub fn query(
        &self,
        subject: Option<&str>,
        predicate: Option<&str>,
        object: Option<&str>,
    ) -> Vec<&Fact> {
        self.knowledge_base.query(subject, predicate, object)
    }

    /// Run forward chaining inference
    pub fn infer(&mut self) -> ReasoningResult<Vec<Fact>> {
        self.derived_facts.clear();
        let mut depth = 0;

        loop {
            if depth >= self.max_depth {
                return Err(ReasoningError::MaxDepthExceeded(self.max_depth));
            }

            let new_facts = self.infer_one_step()?;
            if new_facts.is_empty() {
                break;
            }

            for fact in &new_facts {
                if self.knowledge_base.get(&fact.id).is_none() {
                    self.knowledge_base.add(fact.clone())?;
                    self.derived_facts.push(fact.clone());
                }
            }

            depth += 1;
        }

        Ok(self.derived_facts.clone())
    }

    /// Run one step of inference
    fn infer_one_step(&self) -> ReasoningResult<Vec<Fact>> {
        let mut new_facts = Vec::new();

        // Get rules sorted by priority
        let mut rules: Vec<&Rule> = self.rules.values().filter(|r| r.enabled).collect();
        rules.sort_by(|a, b| b.priority.cmp(&a.priority));

        for rule in rules {
            // Find all bindings that satisfy conditions
            let bindings = self.find_bindings(&rule.conditions);

            for binding in bindings {
                // Generate conclusion facts
                for conclusion in &rule.conclusions {
                    let fact = self.instantiate_pattern(conclusion, &binding, &rule.id);
                    if self.knowledge_base.get(&fact.id).is_none() {
                        new_facts.push(fact);
                    }
                }
            }
        }

        Ok(new_facts)
    }

    /// Find all variable bindings that satisfy a set of patterns
    fn find_bindings(&self, patterns: &[FactPattern]) -> Vec<HashMap<String, String>> {
        if patterns.is_empty() {
            return vec![HashMap::new()];
        }

        let mut all_bindings = vec![HashMap::new()];

        for pattern in patterns {
            let mut new_bindings = Vec::new();

            for existing_binding in &all_bindings {
                // Query with pattern, considering existing bindings
                let subject = pattern.subject.as_ref().and_then(|s| {
                    if s.starts_with('?') {
                        existing_binding.get(s).map(|v: &String| v.as_str())
                    } else {
                        Some(s.as_str())
                    }
                });
                let predicate = pattern.predicate.as_ref().and_then(|p| {
                    if p.starts_with('?') {
                        existing_binding.get(p).map(|v: &String| v.as_str())
                    } else {
                        Some(p.as_str())
                    }
                });
                let object = pattern.object.as_ref().and_then(|o| {
                    if o.starts_with('?') {
                        existing_binding.get(o).map(|v: &String| v.as_str())
                    } else {
                        Some(o.as_str())
                    }
                });

                let facts = self.knowledge_base.query(subject, predicate, object);

                for fact in facts {
                    if let Some(fact_bindings) = pattern.match_fact(fact) {
                        // Merge bindings
                        let mut merged = existing_binding.clone();
                        let mut consistent = true;

                        for (k, v) in fact_bindings {
                            if let Some(existing) = merged.get(&k) {
                                if existing != &v {
                                    consistent = false;
                                    break;
                                }
                            } else {
                                merged.insert(k, v);
                            }
                        }

                        if consistent {
                            new_bindings.push(merged);
                        }
                    }
                }
            }

            all_bindings = new_bindings;
        }

        all_bindings
    }

    /// Instantiate a pattern with bindings to create a fact
    fn instantiate_pattern(
        &self,
        pattern: &FactPattern,
        bindings: &HashMap<String, String>,
        rule_id: &str,
    ) -> Fact {
        let subject = pattern
            .subject
            .as_ref()
            .map(|s| {
                if s.starts_with('?') {
                    bindings.get(s).cloned().unwrap_or_else(|| s.clone())
                } else {
                    s.clone()
                }
            })
            .unwrap_or_else(|| "unknown".to_string());

        let predicate = pattern
            .predicate
            .as_ref()
            .map(|p| {
                if p.starts_with('?') {
                    bindings.get(p).cloned().unwrap_or_else(|| p.clone())
                } else {
                    p.clone()
                }
            })
            .unwrap_or_else(|| "unknown".to_string());

        let object = pattern
            .object
            .as_ref()
            .map(|o| {
                if o.starts_with('?') {
                    bindings.get(o).cloned().unwrap_or_else(|| o.clone())
                } else {
                    o.clone()
                }
            })
            .unwrap_or_else(|| "unknown".to_string());

        Fact::new(&subject, &predicate, &object)
            .with_source(FactSource::Derived(rule_id.to_string()))
    }

    /// Get knowledge base reference
    pub fn knowledge_base(&self) -> &KnowledgeBase {
        &self.knowledge_base
    }

    /// Get fact count
    pub fn fact_count(&self) -> usize {
        self.knowledge_base.len()
    }

    /// Get rule count
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}

/// Goal for planning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    /// Goal identifier
    pub id: String,
    /// Goal description
    pub description: String,
    /// Desired facts (postconditions)
    pub postconditions: Vec<FactPattern>,
    /// Priority (higher = more important)
    pub priority: i32,
    /// Deadline (optional)
    pub deadline: Option<SystemTime>,
    /// Status
    pub status: GoalStatus,
}

impl Goal {
    /// Create a new goal
    pub fn new(id: &str, description: &str) -> Self {
        Self {
            id: id.to_string(),
            description: description.to_string(),
            postconditions: Vec::new(),
            priority: 0,
            deadline: None,
            status: GoalStatus::Pending,
        }
    }

    /// Add a postcondition
    pub fn with_postcondition(mut self, pattern: FactPattern) -> Self {
        self.postconditions.push(pattern);
        self
    }

    /// Set priority
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// Set deadline
    pub fn with_deadline(mut self, deadline: SystemTime) -> Self {
        self.deadline = Some(deadline);
        self
    }
}

/// Goal status
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum GoalStatus {
    /// Goal not yet started
    Pending,
    /// Goal is being pursued
    Active,
    /// Goal achieved
    Achieved,
    /// Goal failed
    Failed,
    /// Goal abandoned
    Abandoned,
}

/// Action that can be taken to achieve goals
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    /// Action identifier
    pub id: String,
    /// Action name
    pub name: String,
    /// Preconditions (required facts)
    pub preconditions: Vec<FactPattern>,
    /// Effects (facts added/removed)
    pub effects: Vec<ActionEffect>,
    /// Cost of action
    pub cost: f64,
    /// Duration estimate
    pub duration: Option<Duration>,
}

impl Action {
    /// Create a new action
    pub fn new(id: &str, name: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            preconditions: Vec::new(),
            effects: Vec::new(),
            cost: 1.0,
            duration: None,
        }
    }

    /// Add a precondition
    pub fn with_precondition(mut self, pattern: FactPattern) -> Self {
        self.preconditions.push(pattern);
        self
    }

    /// Add an effect
    pub fn with_effect(mut self, effect: ActionEffect) -> Self {
        self.effects.push(effect);
        self
    }

    /// Set cost
    pub fn with_cost(mut self, cost: f64) -> Self {
        self.cost = cost;
        self
    }

    /// Set duration
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }
}

/// Effect of an action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActionEffect {
    /// Add a fact
    Add(FactPattern),
    /// Remove a fact
    Remove(FactPattern),
}

/// Simple planner for goal-action planning
#[derive(Debug)]
pub struct Planner {
    /// Available actions
    actions: HashMap<String, Action>,
    /// Maximum plan depth
    max_depth: u32,
}

impl Default for Planner {
    fn default() -> Self {
        Self::new()
    }
}

impl Planner {
    /// Create a new planner
    pub fn new() -> Self {
        Self {
            actions: HashMap::new(),
            max_depth: 10,
        }
    }

    /// Set maximum plan depth
    pub fn with_max_depth(mut self, depth: u32) -> Self {
        self.max_depth = depth;
        self
    }

    /// Add an action
    pub fn add_action(&mut self, action: Action) {
        self.actions.insert(action.id.clone(), action);
    }

    /// Remove an action
    pub fn remove_action(&mut self, id: &str) -> Option<Action> {
        self.actions.remove(id)
    }

    /// Find a plan to achieve a goal
    pub fn plan(&self, goal: &Goal, kb: &KnowledgeBase) -> ReasoningResult<Vec<String>> {
        // Simple backward chaining planner
        let mut plan = Vec::new();
        let mut visited = HashSet::new();

        // Check if goal already satisfied
        if self.goal_satisfied(goal, kb) {
            return Ok(plan);
        }

        // Try to find actions that achieve the goal
        self.backward_chain(goal, kb, &mut plan, &mut visited, 0)?;

        Ok(plan)
    }

    /// Check if goal is satisfied
    fn goal_satisfied(&self, goal: &Goal, kb: &KnowledgeBase) -> bool {
        goal.postconditions.iter().all(|pattern| {
            let facts = kb.query(
                pattern.subject.as_deref(),
                pattern.predicate.as_deref(),
                pattern.object.as_deref(),
            );
            !facts.is_empty()
        })
    }

    /// Backward chain to find plan
    fn backward_chain(
        &self,
        goal: &Goal,
        kb: &KnowledgeBase,
        plan: &mut Vec<String>,
        visited: &mut HashSet<String>,
        depth: u32,
    ) -> ReasoningResult<bool> {
        if depth > self.max_depth {
            return Err(ReasoningError::MaxDepthExceeded(self.max_depth));
        }

        // Find actions that can achieve any of the goal's postconditions
        for pattern in &goal.postconditions {
            for action in self.actions.values() {
                if visited.contains(&action.id) {
                    continue;
                }

                // Check if action achieves this pattern
                let achieves = action.effects.iter().any(|effect| {
                    if let ActionEffect::Add(add_pattern) = effect {
                        patterns_match(add_pattern, pattern)
                    } else {
                        false
                    }
                });

                if achieves {
                    visited.insert(action.id.clone());

                    // Check if preconditions are satisfied
                    let preconditions_met = action.preconditions.iter().all(|pre| {
                        let facts = kb.query(
                            pre.subject.as_deref(),
                            pre.predicate.as_deref(),
                            pre.object.as_deref(),
                        );
                        !facts.is_empty()
                    });

                    if preconditions_met {
                        plan.push(action.id.clone());
                        return Ok(true);
                    }

                    // Otherwise, try to satisfy preconditions first
                    // (simplified - would need full recursive planning)
                }
            }
        }

        Err(ReasoningError::GoalUnreachable(goal.id.clone()))
    }

    /// Get action count
    pub fn action_count(&self) -> usize {
        self.actions.len()
    }
}

/// Check if two patterns match
fn patterns_match(p1: &FactPattern, p2: &FactPattern) -> bool {
    let subj_match = match (&p1.subject, &p2.subject) {
        (Some(a), Some(b)) => a == b || a.starts_with('?') || b.starts_with('?'),
        _ => true,
    };
    let pred_match = match (&p1.predicate, &p2.predicate) {
        (Some(a), Some(b)) => a == b || a.starts_with('?') || b.starts_with('?'),
        _ => true,
    };
    let obj_match = match (&p1.object, &p2.object) {
        (Some(a), Some(b)) => a == b || a.starts_with('?') || b.starts_with('?'),
        _ => true,
    };
    subj_match && pred_match && obj_match
}

/// Decision node in a decision tree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionNode {
    /// Node identifier
    pub id: String,
    /// Node type
    pub node_type: DecisionNodeType,
}

/// Type of decision node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DecisionNodeType {
    /// Condition to check
    Condition {
        /// Condition pattern to match
        pattern: FactPattern,
        /// Node if true
        if_true: String,
        /// Node if false
        if_false: String,
    },
    /// Final decision
    Decision {
        /// Decision value
        value: String,
        /// Confidence
        confidence: f64,
    },
}

/// Decision tree for decision making
#[derive(Debug)]
pub struct DecisionTree {
    /// Nodes in the tree
    nodes: HashMap<String, DecisionNode>,
    /// Root node ID
    root: Option<String>,
}

impl Default for DecisionTree {
    fn default() -> Self {
        Self::new()
    }
}

impl DecisionTree {
    /// Create a new decision tree
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            root: None,
        }
    }

    /// Set root node
    pub fn set_root(&mut self, node_id: &str) {
        self.root = Some(node_id.to_string());
    }

    /// Add a node
    pub fn add_node(&mut self, node: DecisionNode) {
        self.nodes.insert(node.id.clone(), node);
    }

    /// Evaluate the tree
    pub fn evaluate(&self, kb: &KnowledgeBase) -> ReasoningResult<(String, f64)> {
        let root_id = self.root.as_ref()
            .ok_or_else(|| ReasoningError::DecisionError("No root node".to_string()))?;

        self.evaluate_node(root_id, kb)
    }

    /// Evaluate a single node
    fn evaluate_node(&self, node_id: &str, kb: &KnowledgeBase) -> ReasoningResult<(String, f64)> {
        let node = self.nodes.get(node_id)
            .ok_or_else(|| ReasoningError::DecisionError(format!("Node not found: {}", node_id)))?;

        match &node.node_type {
            DecisionNodeType::Condition { pattern, if_true, if_false } => {
                let facts = kb.query(
                    pattern.subject.as_deref(),
                    pattern.predicate.as_deref(),
                    pattern.object.as_deref(),
                );

                let next_node = if !facts.is_empty() { if_true } else { if_false };
                self.evaluate_node(next_node, kb)
            }
            DecisionNodeType::Decision { value, confidence } => {
                Ok((value.clone(), *confidence))
            }
        }
    }

    /// Get node count
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
}

/// Belief in BDI architecture
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Belief {
    /// Belief identifier
    pub id: String,
    /// Subject
    pub subject: String,
    /// Attribute
    pub attribute: String,
    /// Value
    pub value: String,
    /// Confidence (0.0 to 1.0)
    pub confidence: f64,
    /// Last updated
    pub updated_at: SystemTime,
}

impl Belief {
    /// Create a new belief
    pub fn new(id: &str, subject: &str, attribute: &str, value: &str) -> Self {
        Self {
            id: id.to_string(),
            subject: subject.to_string(),
            attribute: attribute.to_string(),
            value: value.to_string(),
            confidence: 1.0,
            updated_at: SystemTime::now(),
        }
    }

    /// Set confidence
    pub fn with_confidence(mut self, confidence: f64) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }
}

/// Desire in BDI architecture
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Desire {
    /// Desire identifier
    pub id: String,
    /// Description
    pub description: String,
    /// Desired state
    pub desired_state: HashMap<String, String>,
    /// Priority
    pub priority: i32,
    /// Active
    pub active: bool,
}

impl Desire {
    /// Create a new desire
    pub fn new(id: &str, description: &str) -> Self {
        Self {
            id: id.to_string(),
            description: description.to_string(),
            desired_state: HashMap::new(),
            priority: 0,
            active: true,
        }
    }

    /// Add desired state
    pub fn with_desired(mut self, key: &str, value: &str) -> Self {
        self.desired_state.insert(key.to_string(), value.to_string());
        self
    }

    /// Set priority
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }
}

/// Intention in BDI architecture
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intention {
    /// Intention identifier
    pub id: String,
    /// Associated desire
    pub desire_id: String,
    /// Plan (sequence of action IDs)
    pub plan: Vec<String>,
    /// Current step
    pub current_step: usize,
    /// Status
    pub status: IntentionStatus,
}

impl Intention {
    /// Create a new intention
    pub fn new(id: &str, desire_id: &str, plan: Vec<String>) -> Self {
        Self {
            id: id.to_string(),
            desire_id: desire_id.to_string(),
            plan,
            current_step: 0,
            status: IntentionStatus::Active,
        }
    }

    /// Get current action
    pub fn current_action(&self) -> Option<&String> {
        self.plan.get(self.current_step)
    }

    /// Advance to next step
    pub fn advance(&mut self) -> bool {
        if self.current_step + 1 < self.plan.len() {
            self.current_step += 1;
            true
        } else {
            self.status = IntentionStatus::Completed;
            false
        }
    }

    /// Check if completed
    pub fn is_completed(&self) -> bool {
        self.status == IntentionStatus::Completed
    }
}

/// Intention status
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum IntentionStatus {
    /// Actively being executed
    Active,
    /// Suspended
    Suspended,
    /// Successfully completed
    Completed,
    /// Failed
    Failed,
}

/// BDI Agent reasoning system
#[derive(Debug)]
pub struct BdiAgent {
    /// Agent identifier
    pub id: String,
    /// Beliefs
    beliefs: HashMap<String, Belief>,
    /// Desires
    desires: HashMap<String, Desire>,
    /// Intentions
    intentions: HashMap<String, Intention>,
}

impl BdiAgent {
    /// Create a new BDI agent
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            beliefs: HashMap::new(),
            desires: HashMap::new(),
            intentions: HashMap::new(),
        }
    }

    /// Add a belief
    pub fn add_belief(&mut self, belief: Belief) {
        self.beliefs.insert(belief.id.clone(), belief);
    }

    /// Remove a belief
    pub fn remove_belief(&mut self, id: &str) -> Option<Belief> {
        self.beliefs.remove(id)
    }

    /// Get a belief
    pub fn get_belief(&self, id: &str) -> Option<&Belief> {
        self.beliefs.get(id)
    }

    /// Add a desire
    pub fn add_desire(&mut self, desire: Desire) {
        self.desires.insert(desire.id.clone(), desire);
    }

    /// Remove a desire
    pub fn remove_desire(&mut self, id: &str) -> Option<Desire> {
        self.desires.remove(id)
    }

    /// Get a desire
    pub fn get_desire(&self, id: &str) -> Option<&Desire> {
        self.desires.get(id)
    }

    /// Add an intention
    pub fn add_intention(&mut self, intention: Intention) {
        self.intentions.insert(intention.id.clone(), intention);
    }

    /// Remove an intention
    pub fn remove_intention(&mut self, id: &str) -> Option<Intention> {
        self.intentions.remove(id)
    }

    /// Get an intention
    pub fn get_intention(&self, id: &str) -> Option<&Intention> {
        self.intentions.get(id)
    }

    /// Get mutable intention
    pub fn get_intention_mut(&mut self, id: &str) -> Option<&mut Intention> {
        self.intentions.get_mut(id)
    }

    /// Get active intentions
    pub fn active_intentions(&self) -> Vec<&Intention> {
        self.intentions
            .values()
            .filter(|i| i.status == IntentionStatus::Active)
            .collect()
    }

    /// Get belief count
    pub fn belief_count(&self) -> usize {
        self.beliefs.len()
    }

    /// Get desire count
    pub fn desire_count(&self) -> usize {
        self.desires.len()
    }

    /// Get intention count
    pub fn intention_count(&self) -> usize {
        self.intentions.len()
    }
}

/// Thread-safe shared reasoning engine
#[derive(Debug, Clone)]
pub struct SharedReasoner {
    inner: Arc<RwLock<InferenceEngine>>,
}

impl Default for SharedReasoner {
    fn default() -> Self {
        Self::new(InferenceEngine::new())
    }
}

impl SharedReasoner {
    /// Create a new shared reasoner
    pub fn new(engine: InferenceEngine) -> Self {
        Self {
            inner: Arc::new(RwLock::new(engine)),
        }
    }

    /// Assert a fact
    pub fn assert_fact(&self, fact: Fact) -> ReasoningResult<()> {
        self.inner.write().unwrap().assert_fact(fact)
    }

    /// Query facts
    pub fn query(
        &self,
        subject: Option<&str>,
        predicate: Option<&str>,
        object: Option<&str>,
    ) -> Vec<Fact> {
        self.inner
            .read()
            .unwrap()
            .query(subject, predicate, object)
            .into_iter()
            .cloned()
            .collect()
    }

    /// Run inference
    pub fn infer(&self) -> ReasoningResult<Vec<Fact>> {
        self.inner.write().unwrap().infer()
    }

    /// Get fact count
    pub fn fact_count(&self) -> usize {
        self.inner.read().unwrap().fact_count()
    }

    /// Get rule count
    pub fn rule_count(&self) -> usize {
        self.inner.read().unwrap().rule_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truth_value_operations() {
        assert!(TruthValue::True.is_true());
        assert!(!TruthValue::False.is_true());
        assert!(!TruthValue::Unknown.is_true());
        assert!(TruthValue::Probable(0.8).is_true());
        assert!(!TruthValue::Probable(0.3).is_true());
    }

    #[test]
    fn test_truth_value_and() {
        assert_eq!(TruthValue::True.and(&TruthValue::True), TruthValue::True);
        assert_eq!(TruthValue::True.and(&TruthValue::False), TruthValue::False);
        assert_eq!(TruthValue::False.and(&TruthValue::True), TruthValue::False);
    }

    #[test]
    fn test_truth_value_or() {
        assert_eq!(TruthValue::True.or(&TruthValue::False), TruthValue::True);
        assert_eq!(TruthValue::False.or(&TruthValue::True), TruthValue::True);
        assert_eq!(TruthValue::False.or(&TruthValue::False), TruthValue::False);
    }

    #[test]
    fn test_truth_value_not() {
        assert_eq!(TruthValue::True.not(), TruthValue::False);
        assert_eq!(TruthValue::False.not(), TruthValue::True);
    }

    #[test]
    fn test_fact_creation() {
        let fact = Fact::new("alice", "knows", "bob")
            .with_truth(TruthValue::True)
            .with_metadata("source", "observation");

        assert_eq!(fact.subject, "alice");
        assert_eq!(fact.predicate, "knows");
        assert_eq!(fact.object, "bob");
        assert_eq!(fact.truth, TruthValue::True);
        assert_eq!(fact.metadata.get("source"), Some(&"observation".to_string()));
    }

    #[test]
    fn test_fact_matching() {
        let fact = Fact::new("alice", "knows", "bob");

        assert!(fact.matches(Some("alice"), None, None));
        assert!(fact.matches(None, Some("knows"), None));
        assert!(fact.matches(Some("alice"), Some("knows"), Some("bob")));
        assert!(!fact.matches(Some("charlie"), None, None));
    }

    #[test]
    fn test_knowledge_base_add_query() {
        let mut kb = KnowledgeBase::new();

        kb.add(Fact::new("alice", "knows", "bob")).unwrap();
        kb.add(Fact::new("alice", "knows", "charlie")).unwrap();
        kb.add(Fact::new("bob", "knows", "alice")).unwrap();

        assert_eq!(kb.len(), 3);

        let results = kb.query(Some("alice"), Some("knows"), None);
        assert_eq!(results.len(), 2);

        let results = kb.query(None, Some("knows"), Some("alice"));
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_knowledge_base_remove() {
        let mut kb = KnowledgeBase::new();

        let fact = Fact::new("alice", "knows", "bob");
        let id = fact.id.clone();

        kb.add(fact).unwrap();
        assert_eq!(kb.len(), 1);

        kb.remove(&id).unwrap();
        assert_eq!(kb.len(), 0);
    }

    #[test]
    fn test_rule_creation() {
        let rule = Rule::new("rule-1", "Knows Transitivity")
            .with_condition(
                FactPattern::any()
                    .with_subject("?x")
                    .with_predicate("knows")
                    .with_object("?y"),
            )
            .with_conclusion(
                FactPattern::any()
                    .with_subject("?x")
                    .with_predicate("acquainted")
                    .with_object("?y"),
            )
            .with_priority(10);

        assert_eq!(rule.id, "rule-1");
        assert_eq!(rule.priority, 10);
        assert_eq!(rule.conditions.len(), 1);
        assert_eq!(rule.conclusions.len(), 1);
    }

    #[test]
    fn test_inference_engine_facts() {
        let mut engine = InferenceEngine::new();

        engine.assert_fact(Fact::new("socrates", "is", "human")).unwrap();
        engine.assert_fact(Fact::new("plato", "is", "human")).unwrap();

        assert_eq!(engine.fact_count(), 2);

        let results = engine.query(None, Some("is"), Some("human"));
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_inference_engine_rules() {
        let mut engine = InferenceEngine::new();

        engine.add_rule(Rule::new("rule-1", "Rule 1")).unwrap();
        engine.add_rule(Rule::new("rule-2", "Rule 2")).unwrap();

        assert_eq!(engine.rule_count(), 2);
    }

    #[test]
    fn test_goal_creation() {
        let goal = Goal::new("goal-1", "Find food")
            .with_postcondition(
                FactPattern::any()
                    .with_subject("agent")
                    .with_predicate("has")
                    .with_object("food"),
            )
            .with_priority(5);

        assert_eq!(goal.id, "goal-1");
        assert_eq!(goal.priority, 5);
        assert_eq!(goal.postconditions.len(), 1);
    }

    #[test]
    fn test_action_creation() {
        let action = Action::new("action-1", "Pick up food")
            .with_precondition(
                FactPattern::any()
                    .with_subject("agent")
                    .with_predicate("near")
                    .with_object("food"),
            )
            .with_effect(ActionEffect::Add(
                FactPattern::any()
                    .with_subject("agent")
                    .with_predicate("has")
                    .with_object("food"),
            ))
            .with_cost(1.0);

        assert_eq!(action.id, "action-1");
        assert_eq!(action.cost, 1.0);
        assert_eq!(action.preconditions.len(), 1);
        assert_eq!(action.effects.len(), 1);
    }

    #[test]
    fn test_planner_creation() {
        let mut planner = Planner::new();

        planner.add_action(Action::new("action-1", "Action 1"));
        planner.add_action(Action::new("action-2", "Action 2"));

        assert_eq!(planner.action_count(), 2);

        planner.remove_action("action-1");
        assert_eq!(planner.action_count(), 1);
    }

    #[test]
    fn test_decision_tree() {
        let mut tree = DecisionTree::new();

        tree.add_node(DecisionNode {
            id: "root".to_string(),
            node_type: DecisionNodeType::Condition {
                pattern: FactPattern::any()
                    .with_subject("weather")
                    .with_predicate("is")
                    .with_object("sunny"),
                if_true: "go_outside".to_string(),
                if_false: "stay_inside".to_string(),
            },
        });

        tree.add_node(DecisionNode {
            id: "go_outside".to_string(),
            node_type: DecisionNodeType::Decision {
                value: "Go outside".to_string(),
                confidence: 0.9,
            },
        });

        tree.add_node(DecisionNode {
            id: "stay_inside".to_string(),
            node_type: DecisionNodeType::Decision {
                value: "Stay inside".to_string(),
                confidence: 0.8,
            },
        });

        tree.set_root("root");
        assert_eq!(tree.node_count(), 3);
    }

    #[test]
    fn test_decision_tree_evaluate() {
        let mut tree = DecisionTree::new();
        let mut kb = KnowledgeBase::new();

        tree.add_node(DecisionNode {
            id: "root".to_string(),
            node_type: DecisionNodeType::Condition {
                pattern: FactPattern::any()
                    .with_subject("weather")
                    .with_predicate("is")
                    .with_object("sunny"),
                if_true: "go_outside".to_string(),
                if_false: "stay_inside".to_string(),
            },
        });

        tree.add_node(DecisionNode {
            id: "go_outside".to_string(),
            node_type: DecisionNodeType::Decision {
                value: "Go outside".to_string(),
                confidence: 0.9,
            },
        });

        tree.add_node(DecisionNode {
            id: "stay_inside".to_string(),
            node_type: DecisionNodeType::Decision {
                value: "Stay inside".to_string(),
                confidence: 0.8,
            },
        });

        tree.set_root("root");

        // Without sunny weather
        let (decision, confidence) = tree.evaluate(&kb).unwrap();
        assert_eq!(decision, "Stay inside");
        assert_eq!(confidence, 0.8);

        // With sunny weather
        kb.add(Fact::new("weather", "is", "sunny")).unwrap();
        let (decision, confidence) = tree.evaluate(&kb).unwrap();
        assert_eq!(decision, "Go outside");
        assert_eq!(confidence, 0.9);
    }

    #[test]
    fn test_belief_creation() {
        let belief = Belief::new("belief-1", "world", "state", "peaceful")
            .with_confidence(0.85);

        assert_eq!(belief.id, "belief-1");
        assert_eq!(belief.subject, "world");
        assert_eq!(belief.confidence, 0.85);
    }

    #[test]
    fn test_desire_creation() {
        let desire = Desire::new("desire-1", "Find shelter")
            .with_desired("location", "safe")
            .with_priority(10);

        assert_eq!(desire.id, "desire-1");
        assert_eq!(desire.priority, 10);
        assert_eq!(desire.desired_state.get("location"), Some(&"safe".to_string()));
    }

    #[test]
    fn test_intention_creation() {
        let intention = Intention::new(
            "intention-1",
            "desire-1",
            vec!["action-1".to_string(), "action-2".to_string()],
        );

        assert_eq!(intention.id, "intention-1");
        assert_eq!(intention.desire_id, "desire-1");
        assert_eq!(intention.plan.len(), 2);
        assert_eq!(intention.current_action(), Some(&"action-1".to_string()));
    }

    #[test]
    fn test_intention_advance() {
        let mut intention = Intention::new(
            "intention-1",
            "desire-1",
            vec!["action-1".to_string(), "action-2".to_string()],
        );

        assert_eq!(intention.current_step, 0);
        assert!(intention.advance());
        assert_eq!(intention.current_step, 1);
        assert!(!intention.advance());
        assert!(intention.is_completed());
    }

    #[test]
    fn test_bdi_agent() {
        let mut agent = BdiAgent::new("agent-1");

        agent.add_belief(Belief::new("b1", "env", "temp", "cold"));
        agent.add_desire(Desire::new("d1", "Stay warm"));
        agent.add_intention(Intention::new("i1", "d1", vec!["find_shelter".to_string()]));

        assert_eq!(agent.belief_count(), 1);
        assert_eq!(agent.desire_count(), 1);
        assert_eq!(agent.intention_count(), 1);

        assert!(agent.get_belief("b1").is_some());
        assert!(agent.get_desire("d1").is_some());
        assert!(agent.get_intention("i1").is_some());
    }

    #[test]
    fn test_bdi_agent_active_intentions() {
        let mut agent = BdiAgent::new("agent-1");

        agent.add_intention(Intention::new("i1", "d1", vec!["a1".to_string()]));
        
        let mut completed = Intention::new("i2", "d2", vec!["a2".to_string()]);
        completed.status = IntentionStatus::Completed;
        agent.add_intention(completed);

        let active = agent.active_intentions();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, "i1");
    }

    #[test]
    fn test_shared_reasoner() {
        let reasoner = SharedReasoner::default();

        reasoner.assert_fact(Fact::new("a", "r", "b")).unwrap();
        reasoner.assert_fact(Fact::new("b", "r", "c")).unwrap();

        assert_eq!(reasoner.fact_count(), 2);

        let facts = reasoner.query(Some("a"), None, None);
        assert_eq!(facts.len(), 1);
    }

    #[test]
    fn test_fact_pattern_matching() {
        let pattern = FactPattern::any()
            .with_subject("alice")
            .with_predicate("knows")
            .with_variable("?obj", PatternPosition::Object);

        let fact = Fact::new("alice", "knows", "bob");
        let bindings = pattern.match_fact(&fact);

        assert!(bindings.is_some());
        let bindings = bindings.unwrap();
        assert_eq!(bindings.get("?obj"), Some(&"bob".to_string()));
    }

    #[test]
    fn test_fact_pattern_no_match() {
        let pattern = FactPattern::any()
            .with_subject("alice")
            .with_predicate("hates");

        let fact = Fact::new("alice", "knows", "bob");
        let bindings = pattern.match_fact(&fact);

        assert!(bindings.is_none());
    }

    #[test]
    fn test_planner_goal_satisfied() {
        let planner = Planner::new();
        let mut kb = KnowledgeBase::new();

        let goal = Goal::new("goal-1", "Have food")
            .with_postcondition(
                FactPattern::any()
                    .with_subject("agent")
                    .with_predicate("has")
                    .with_object("food"),
            );

        // Goal not satisfied initially
        let plan = planner.plan(&goal, &kb);
        assert!(plan.is_err()); // No actions to achieve it

        // Add the goal fact
        kb.add(Fact::new("agent", "has", "food")).unwrap();

        // Now goal is satisfied
        let plan = planner.plan(&goal, &kb).unwrap();
        assert!(plan.is_empty()); // No actions needed
    }

    #[test]
    fn test_reasoning_error_display() {
        let err = ReasoningError::FactNotFound("test".to_string());
        assert!(err.to_string().contains("Fact not found"));

        let err = ReasoningError::MaxDepthExceeded(10);
        assert!(err.to_string().contains("10"));
    }
}
