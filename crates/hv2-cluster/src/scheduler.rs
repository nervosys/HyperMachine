//! Which node gets the next sandbox.
//!
//! Spread, not pack: the least-loaded node by fraction of its capacity, so a
//! node failing takes as few sandboxes with it as possible and one busy guest
//! crowds as few neighbours. Every node with room is returned, in order, so
//! a create that one node refuses moves on to the next.
//!
//! The ordering is only advice. Two control planes scheduling at once see
//! the same heartbeats and can pick the same node; the node is the authority
//! on its own capacity and refuses what it has no room for, and the caller
//! tries the next candidate. That is what makes the control plane stateless
//! without a lock: nothing it believes has to be true for a create to land
//! safely.

use crate::model::NodeInfo;

/// Nodes that may take a sandbox, best first.
#[must_use]
pub fn candidates(nodes: &[NodeInfo]) -> Vec<NodeInfo> {
    let mut open: Vec<NodeInfo> = nodes.iter().filter(|n| n.has_room()).cloned().collect();
    open.sort_by(|a, b| {
        // running/capacity, compared without division: a.r * b.c vs b.r * a.c.
        let load_a = u64::from(a.running) * u64::from(b.capacity.max(1));
        let load_b = u64::from(b.running) * u64::from(a.capacity.max(1));
        load_a
            .cmp(&load_b)
            .then(b.capacity.cmp(&a.capacity))
            .then(a.id.cmp(&b.id))
    });
    open
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::node;

    fn ids(nodes: &[NodeInfo]) -> Vec<&str> {
        nodes.iter().map(|n| n.id.as_str()).collect()
    }

    #[test]
    fn the_least_loaded_fraction_goes_first() {
        let nodes = [node("a", 3, 4), node("b", 1, 4), node("c", 4, 16)];
        // 3/4, 1/4, 4/16 = 1/4: b and c tie, and the bigger node wins the tie.
        assert_eq!(ids(&candidates(&nodes)), ["c", "b", "a"]);
    }

    #[test]
    fn a_full_node_is_not_a_candidate() {
        let nodes = [node("a", 4, 4), node("b", 0, 2)];
        assert_eq!(ids(&candidates(&nodes)), ["b"]);
        assert!(candidates(&[node("a", 1, 1)]).is_empty());
    }

    #[test]
    fn a_zero_capacity_node_takes_nothing_and_breaks_nothing() {
        let nodes = [node("a", 0, 0), node("b", 0, 1)];
        assert_eq!(ids(&candidates(&nodes)), ["b"]);
    }
}
