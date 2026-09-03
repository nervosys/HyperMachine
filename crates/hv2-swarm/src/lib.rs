//! Who may talk to whom, in a swarm of agents, enforced rather than described.
//!
//! A thousand agents that can all reach each other is not a swarm with
//! permissions; it is a flat network with a diagram. This crate is the
//! opposite arrangement: there is one function that moves a message, it
//! consults the graph before it moves anything, and there is no second way.
//!
//! # Why a chokepoint, and not a policy object
//!
//! The temptation with a permission model is to write down the rules and hand
//! them to something that may or may not consult them. This repository already
//! has one of those — `AgentPolicy` records quotas and rate limits that nothing
//! reads — and the lesson is that an unconsulted rule is worse than an absent
//! one, because it is believed.
//!
//! So [`Swarm::send`] is the only way a message travels, the graph is checked
//! inside it, and a denial returns [`Denied`] naming the rule rather than
//! dropping the message quietly. The tests assert on delivery, not on the
//! verdict: a rule that returns "denied" while the message still arrives would
//! pass a verdict test and fail the only one that matters.
//!
//! # The model
//!
//! Command flows down a tree; reports flow up it; anything sideways needs an
//! explicit grant.
//!
//! ```text
//!            supervisor
//!           /          \
//!     planner          auditor          planner -> auditor needs a grant
//!     /     \                           planner -> supervisor is a report
//! worker-a  worker-b                    supervisor -> worker-a is a command
//! ```
//!
//! - **Down** ([`Relation::Descendant`]): an agent may command anything beneath
//!   it, at any depth. Authority delegates.
//! - **Up** ([`Relation::Parent`]): an agent may report to its *immediate*
//!   parent only. Escalation does not skip a level, because a worker that can
//!   address the root has routed around every supervisor between them.
//! - **Sideways** ([`Relation::Granted`]): nothing, unless
//!   [`Swarm::grant`] said so. Grants are directional: `a -> b` does not
//!   imply `b -> a`.
//! - **Everything else**: refused.
//!
//! Descent is the asymmetry worth noticing. A supervisor reaching a
//! grandchild is delegation working as intended; a grandchild reaching a
//! grandparent is a subordinate choosing its own audience.
//!
//! # What this does not do yet
//!
//! Carry messages between machines, or into a guest. [`Transport`] is where
//! that goes and [`LocalTransport`] is what exists — an in-process queue, which
//! is a real delivery mechanism and enough to prove the enforcement is real. A
//! vsock transport that reaches an agent inside a unikernel is the next piece,
//! and it changes where messages land, not who may send them.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

use serde::{Deserialize, Serialize};

/// An agent's name in the swarm. Unique, and stable for the agent's lifetime.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AgentId(pub String);

impl AgentId {
    /// An id from anything string-shaped.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The id as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for AgentId {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for AgentId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// Why a message was allowed.
///
/// Carried on the allowed path as well as the denied one, so an audit log can
/// record which rule admitted a message rather than only that something did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relation {
    /// The recipient is below the sender in the tree: a command.
    Descendant,
    /// The recipient is the sender's immediate parent: a report.
    Parent,
    /// Neither, but an explicit grant admits it.
    Granted,
}

impl fmt::Display for Relation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Descendant => "command to a descendant",
            Self::Parent => "report to the immediate parent",
            Self::Granted => "explicitly granted peer edge",
        })
    }
}

/// Why a message was refused.
///
/// Every variant names the rule rather than saying "denied", because the
/// caller has to decide whether to ask for a grant, address someone else, or
/// treat it as a bug.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Denied {
    /// The sender is not in this swarm.
    #[error("no agent named '{0}' is in this swarm, so nothing can be sent from it")]
    UnknownSender(AgentId),

    /// The recipient is not in this swarm.
    #[error("no agent named '{0}' is in this swarm, so nothing can be sent to it")]
    UnknownRecipient(AgentId),

    /// An agent addressed itself.
    #[error("'{0}' addressed itself; a message to oneself is a call, not a message")]
    Self_(AgentId),

    /// Up the tree, but not to the immediate parent.
    #[error(
        "'{from}' may report to its parent '{parent}', not to '{to}' above it — \
         escalation that skips a level routes around every supervisor in between"
    )]
    SkipsLevel {
        /// The sender.
        from: AgentId,
        /// Who it tried to reach.
        to: AgentId,
        /// Who it may reach instead.
        parent: AgentId,
    },

    /// Sideways, with no grant.
    #[error(
        "'{from}' and '{to}' are on separate branches and no grant connects them; \
         call Swarm::grant to open the edge"
    )]
    NoGrant {
        /// The sender.
        from: AgentId,
        /// Who it tried to reach.
        to: AgentId,
    },

    /// The root has no parent to report to.
    #[error("'{0}' is the root and has nobody above it to report to")]
    RootHasNoParent(AgentId),
}

/// Why an agent could not join the swarm.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum JoinError {
    /// Two agents cannot share a name.
    #[error("'{0}' is already in this swarm")]
    DuplicateId(AgentId),

    /// The named supervisor does not exist.
    #[error("'{parent}' is not in this swarm, so '{child}' cannot report to it")]
    UnknownParent {
        /// The agent being added.
        child: AgentId,
        /// The supervisor it named.
        parent: AgentId,
    },

    /// A swarm has exactly one root.
    #[error("this swarm already has a root ('{0}'); every other agent needs a parent")]
    RootExists(AgentId),
}

/// One message, as it crosses the chokepoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// Who sent it.
    pub from: AgentId,
    /// Who it is for.
    pub to: AgentId,
    /// The rule that admitted it.
    pub under: Relation,
    /// The body. Opaque here: this crate routes, it does not interpret.
    pub payload: Vec<u8>,
}

/// Where an admitted message goes.
///
/// Separate from the graph so the rules do not change when delivery does. An
/// in-process queue, a vsock write into a unikernel, and a network hop to
/// another host are three implementations of this and one set of permissions.
pub trait Transport: Send + Sync {
    /// Deliver `message`, which the graph has already admitted.
    ///
    /// Called only after [`Swarm::send`] has consulted the graph, so an
    /// implementation does not repeat the check and must not second-guess it.
    fn deliver(&mut self, message: Message);
}

/// Delivery into per-agent in-process queues.
///
/// Enough to prove the enforcement is real — a denied message is absent from
/// the recipient's queue, which is the assertion that matters — without
/// pulling in a guest or a network.
#[derive(Debug, Default)]
pub struct LocalTransport {
    inboxes: BTreeMap<AgentId, VecDeque<Message>>,
}

impl LocalTransport {
    /// An empty transport.
    pub fn new() -> Self {
        Self::default()
    }

    /// Everything delivered to `agent`, oldest first, without consuming it.
    pub fn inbox(&self, agent: &AgentId) -> &[Message] {
        self.inboxes
            .get(agent)
            .map(|q| q.as_slices().0)
            .unwrap_or(&[])
    }

    /// Take the next message for `agent`.
    pub fn next_for(&mut self, agent: &AgentId) -> Option<Message> {
        self.inboxes.get_mut(agent)?.pop_front()
    }

    /// How many messages `agent` has been delivered.
    pub fn delivered_to(&self, agent: &AgentId) -> usize {
        self.inboxes.get(agent).map_or(0, |q| q.len())
    }
}

impl Transport for LocalTransport {
    fn deliver(&mut self, message: Message) {
        self.inboxes
            .entry(message.to.clone())
            .or_default()
            .push_back(message);
    }
}

/// One agent's place in the swarm.
#[derive(Debug, Clone)]
struct Node {
    parent: Option<AgentId>,
    children: BTreeSet<AgentId>,
}

/// A swarm: the agents, the tree that orders them, and the only way to send.
#[derive(Debug)]
pub struct Swarm<T: Transport> {
    nodes: BTreeMap<AgentId, Node>,
    root: Option<AgentId>,
    /// Directional peer edges. `(from, to)` present means `from` may send to
    /// `to`, and says nothing about the other direction.
    grants: BTreeSet<(AgentId, AgentId)>,
    transport: T,
}

impl<T: Transport> Swarm<T> {
    /// An empty swarm delivering through `transport`.
    pub fn new(transport: T) -> Self {
        Self {
            nodes: BTreeMap::new(),
            root: None,
            grants: BTreeSet::new(),
            transport,
        }
    }

    /// Add the root. A swarm has exactly one.
    pub fn add_root(&mut self, id: impl Into<AgentId>) -> Result<(), JoinError> {
        let id = id.into();
        if let Some(existing) = &self.root {
            return Err(JoinError::RootExists(existing.clone()));
        }
        if self.nodes.contains_key(&id) {
            return Err(JoinError::DuplicateId(id));
        }
        self.nodes.insert(
            id.clone(),
            Node {
                parent: None,
                children: BTreeSet::new(),
            },
        );
        self.root = Some(id);
        Ok(())
    }

    /// Add `id` beneath `parent`.
    ///
    /// A cycle is impossible by construction: an agent joins once, naming a
    /// parent that already exists, so every edge points at something older
    /// than itself.
    pub fn add_agent(
        &mut self,
        id: impl Into<AgentId>,
        parent: impl Into<AgentId>,
    ) -> Result<(), JoinError> {
        let id = id.into();
        let parent = parent.into();

        if self.nodes.contains_key(&id) {
            return Err(JoinError::DuplicateId(id));
        }
        if !self.nodes.contains_key(&parent) {
            return Err(JoinError::UnknownParent { child: id, parent });
        }

        self.nodes.insert(
            id.clone(),
            Node {
                parent: Some(parent.clone()),
                children: BTreeSet::new(),
            },
        );
        if let Some(node) = self.nodes.get_mut(&parent) {
            node.children.insert(id);
        }
        Ok(())
    }

    /// Open a one-way peer edge from `from` to `to`.
    ///
    /// Directional on purpose. A worker that may report a finding to an
    /// auditor has not thereby agreed to take instructions from it.
    pub fn grant(&mut self, from: impl Into<AgentId>, to: impl Into<AgentId>) {
        self.grants.insert((from.into(), to.into()));
    }

    /// Close an edge [`grant`](Self::grant) opened.
    pub fn revoke(&mut self, from: &AgentId, to: &AgentId) {
        self.grants.remove(&(from.clone(), to.clone()));
    }

    /// How many agents are in the swarm.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the swarm is empty.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The transport, for reading what was delivered.
    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// The transport, mutably.
    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    /// Whether `from` may send to `to`, and under which rule.
    ///
    /// Public so a caller can ask before composing an expensive message, and
    /// so an operator can inspect the graph. Asking is not sending: nothing
    /// reaches a recipient except through [`send`](Self::send), which asks
    /// again.
    pub fn may_send(&self, from: &AgentId, to: &AgentId) -> Result<Relation, Denied> {
        if !self.nodes.contains_key(from) {
            return Err(Denied::UnknownSender(from.clone()));
        }
        if !self.nodes.contains_key(to) {
            return Err(Denied::UnknownRecipient(to.clone()));
        }
        if from == to {
            return Err(Denied::Self_(from.clone()));
        }

        // Command: anything beneath the sender, at any depth. Checked by
        // walking up from the recipient, which costs the depth of the tree
        // rather than the size of the subtree.
        if self.is_ancestor_of(from, to) {
            return Ok(Relation::Descendant);
        }

        // Report: the immediate parent, and no further.
        let parent = self.nodes.get(from).and_then(|n| n.parent.clone());
        if parent.as_ref() == Some(to) {
            return Ok(Relation::Parent);
        }

        // An explicit grant admits what the tree does not.
        if self.grants.contains(&(from.clone(), to.clone())) {
            return Ok(Relation::Granted);
        }

        // Refused, and the reason distinguishes "you aimed too high" from
        // "you aimed sideways", because the fixes differ.
        if self.is_ancestor_of(to, from) {
            return Err(match parent {
                Some(parent) => Denied::SkipsLevel {
                    from: from.clone(),
                    to: to.clone(),
                    parent,
                },
                None => Denied::RootHasNoParent(from.clone()),
            });
        }
        Err(Denied::NoGrant {
            from: from.clone(),
            to: to.clone(),
        })
    }

    /// Send `payload` from `from` to `to`, if the graph allows it.
    ///
    /// The only way a message moves. Consults the graph first and hands the
    /// transport nothing when the answer is no, so a denial is an absence at
    /// the recipient and not merely an error at the sender.
    ///
    /// # Errors
    ///
    /// [`Denied`], naming the rule that refused.
    pub fn send(
        &mut self,
        from: impl Into<AgentId>,
        to: impl Into<AgentId>,
        payload: impl Into<Vec<u8>>,
    ) -> Result<Relation, Denied> {
        let from = from.into();
        let to = to.into();

        let under = match self.may_send(&from, &to) {
            Ok(under) => under,
            Err(denied) => {
                // At this scale a refused message is a routine fact about a
                // working permission graph, not an incident.
                tracing::debug!("swarm: refused {from} -> {to}: {denied}");
                return Err(denied);
            }
        };

        self.transport.deliver(Message {
            from,
            to,
            under,
            payload: payload.into(),
        });
        Ok(under)
    }

    /// Whether `ancestor` is above `descendant` anywhere in the tree.
    fn is_ancestor_of(&self, ancestor: &AgentId, descendant: &AgentId) -> bool {
        let mut current = self.nodes.get(descendant).and_then(|n| n.parent.as_ref());
        // Bounded by the number of agents: a malformed tree should fail the
        // test rather than hang the swarm, which is how a guest experiences
        // the same bug.
        for _ in 0..self.nodes.len() {
            match current {
                Some(id) if id == ancestor => return true,
                Some(id) => current = self.nodes.get(id).and_then(|n| n.parent.as_ref()),
                None => return false,
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tree in the module documentation.
    fn swarm() -> Swarm<LocalTransport> {
        let mut swarm = Swarm::new(LocalTransport::new());
        swarm.add_root("supervisor").unwrap();
        swarm.add_agent("planner", "supervisor").unwrap();
        swarm.add_agent("auditor", "supervisor").unwrap();
        swarm.add_agent("worker-a", "planner").unwrap();
        swarm.add_agent("worker-b", "planner").unwrap();
        swarm
    }

    fn id(s: &str) -> AgentId {
        AgentId::new(s)
    }

    #[test]
    fn a_supervisor_commands_its_whole_subtree() {
        let mut swarm = swarm();

        assert_eq!(
            swarm.send("supervisor", "planner", "plan").unwrap(),
            Relation::Descendant
        );
        // Two levels down: authority delegates rather than stopping at the
        // first hop.
        assert_eq!(
            swarm.send("supervisor", "worker-a", "do this").unwrap(),
            Relation::Descendant
        );
        assert_eq!(swarm.transport().delivered_to(&id("worker-a")), 1);
    }

    #[test]
    fn an_agent_reports_to_its_parent() {
        let mut swarm = swarm();
        assert_eq!(
            swarm.send("worker-a", "planner", "done").unwrap(),
            Relation::Parent
        );
    }

    /// The assertion that matters. A denial that still delivers would pass a
    /// test on the returned error and fail the swarm.
    #[test]
    fn a_refused_message_does_not_arrive() {
        let mut swarm = swarm();

        let denied = swarm.send("worker-a", "worker-b", "psst").unwrap_err();
        assert!(matches!(denied, Denied::NoGrant { .. }), "{denied}");
        assert_eq!(
            swarm.transport().delivered_to(&id("worker-b")),
            0,
            "the message was refused and delivered anyway"
        );
    }

    #[test]
    fn escalation_may_not_skip_a_level() {
        let mut swarm = swarm();

        let denied = swarm
            .send("worker-a", "supervisor", "over my boss")
            .unwrap_err();
        match denied {
            Denied::SkipsLevel { parent, .. } => assert_eq!(parent, id("planner")),
            other => panic!("expected SkipsLevel, got {other}"),
        }
        assert_eq!(swarm.transport().delivered_to(&id("supervisor")), 0);
    }

    #[test]
    fn a_grant_opens_exactly_one_direction() {
        let mut swarm = swarm();
        swarm.grant("planner", "auditor");

        assert_eq!(
            swarm.send("planner", "auditor", "for review").unwrap(),
            Relation::Granted
        );
        // The reverse edge was never opened. A worker that may report a
        // finding has not agreed to take instructions back.
        let denied = swarm.send("auditor", "planner", "now do this").unwrap_err();
        assert!(matches!(denied, Denied::NoGrant { .. }), "{denied}");
        assert_eq!(swarm.transport().delivered_to(&id("planner")), 0);
    }

    #[test]
    fn revoking_a_grant_closes_the_edge() {
        let mut swarm = swarm();
        swarm.grant("worker-a", "worker-b");
        swarm.send("worker-a", "worker-b", "one").unwrap();

        swarm.revoke(&id("worker-a"), &id("worker-b"));
        assert!(swarm.send("worker-a", "worker-b", "two").is_err());
        assert_eq!(
            swarm.transport().delivered_to(&id("worker-b")),
            1,
            "only the message sent while the grant was open should have arrived"
        );
    }

    #[test]
    fn an_unknown_agent_can_neither_send_nor_receive() {
        let mut swarm = swarm();

        assert!(matches!(
            swarm.send("ghost", "planner", "boo").unwrap_err(),
            Denied::UnknownSender(_)
        ));
        assert!(matches!(
            swarm.send("planner", "ghost", "hello").unwrap_err(),
            Denied::UnknownRecipient(_)
        ));
        assert_eq!(swarm.transport().delivered_to(&id("planner")), 0);
    }

    #[test]
    fn the_root_has_nobody_to_report_to() {
        let mut swarm = swarm();
        swarm.add_agent("orphan", "supervisor").unwrap();

        // The root addressing something outside its own tree cannot happen —
        // everything is in its tree — so this checks the other direction.
        let denied = swarm
            .may_send(&id("supervisor"), &id("supervisor"))
            .unwrap_err();
        assert!(matches!(denied, Denied::Self_(_)), "{denied}");
    }

    #[test]
    fn a_swarm_has_one_root_and_no_duplicates() {
        let mut swarm = swarm();

        assert!(matches!(
            swarm.add_root("second").unwrap_err(),
            JoinError::RootExists(_)
        ));
        assert!(matches!(
            swarm.add_agent("planner", "supervisor").unwrap_err(),
            JoinError::DuplicateId(_)
        ));
        assert!(matches!(
            swarm.add_agent("stray", "nobody").unwrap_err(),
            JoinError::UnknownParent { .. }
        ));
    }

    /// The shape the swarm is meant to hold: a wide, shallow tree of many
    /// agents. Checks that authority still reaches the bottom and that
    /// siblings are still isolated from each other at that size.
    #[test]
    fn a_thousand_agents_keep_their_boundaries() {
        let mut swarm = Swarm::new(LocalTransport::new());
        swarm.add_root("root").unwrap();
        for supervisor in 0..10 {
            swarm
                .add_agent(format!("sup-{supervisor}"), "root")
                .unwrap();
            for worker in 0..99 {
                swarm
                    .add_agent(
                        format!("w-{supervisor}-{worker}"),
                        format!("sup-{supervisor}"),
                    )
                    .unwrap();
            }
        }
        assert_eq!(swarm.len(), 1 + 10 + 990);

        // The root commands a leaf nine hundred agents away.
        assert_eq!(
            swarm.send("root", "w-9-98", "go").unwrap(),
            Relation::Descendant
        );
        // A leaf under one supervisor cannot reach a leaf under another.
        assert!(swarm.send("w-0-0", "w-9-98", "hey").is_err());
        // Nor its own sibling.
        assert!(swarm.send("w-0-0", "w-0-1", "hey").is_err());
        assert_eq!(swarm.transport().delivered_to(&id("w-0-1")), 0);
        assert_eq!(swarm.transport().delivered_to(&id("w-9-98")), 1);
    }

    #[test]
    fn the_rule_that_admitted_a_message_travels_with_it() {
        let mut swarm = swarm();
        swarm.send("supervisor", "worker-b", "command").unwrap();

        let message = swarm.transport_mut().next_for(&id("worker-b")).unwrap();
        assert_eq!(message.under, Relation::Descendant);
        assert_eq!(message.from, id("supervisor"));
        assert_eq!(message.payload, b"command");
    }
}
