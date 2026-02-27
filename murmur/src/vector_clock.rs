use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// Vector clock for tracking causal relationships between events.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VectorClock {
    /// Map from node_id to logical clock value
    clocks: HashMap<String, u64>,
}

impl VectorClock {
    /// Create a new empty vector clock.
    pub fn new() -> Self {
        Self {
            clocks: HashMap::new(),
        }
    }

    /// Increment the clock for a given node.
    pub fn increment(&mut self, node_id: &str) {
        let counter = self.clocks.entry(node_id.to_string()).or_insert(0);
        *counter += 1;
    }

    /// Get the clock value for a node.
    pub fn get(&self, node_id: &str) -> u64 {
        self.clocks.get(node_id).copied().unwrap_or(0)
    }

    /// Merge with another vector clock (take max of each component).
    pub fn merge(&mut self, other: &VectorClock) {
        for (node_id, &other_clock) in &other.clocks {
            let self_clock = self.clocks.entry(node_id.clone()).or_insert(0);
            *self_clock = (*self_clock).max(other_clock);
        }
    }

    /// Check if this clock happened before another (strict partial order).
    pub fn happens_before(&self, other: &VectorClock) -> bool {
        let mut strictly_less = false;

        // Check all nodes in self
        for (node_id, &self_clock) in &self.clocks {
            let other_clock = other.get(node_id);
            if self_clock > other_clock {
                return false; // Not happened-before
            }
            if self_clock < other_clock {
                strictly_less = true;
            }
        }

        // Check nodes only in other
        for (node_id, &other_clock) in &other.clocks {
            if !self.clocks.contains_key(node_id) && other_clock > 0 {
                strictly_less = true;
            }
        }

        strictly_less
    }

    /// Check if two clocks are concurrent (neither happens-before the other).
    pub fn is_concurrent(&self, other: &VectorClock) -> bool {
        !self.happens_before(other) && !other.happens_before(self) && self != other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vector_clock_increment() {
        let mut vc = VectorClock::new();
        vc.increment("node1");
        assert_eq!(vc.get("node1"), 1);
        vc.increment("node1");
        assert_eq!(vc.get("node1"), 2);
    }

    #[test]
    fn test_vector_clock_merge() {
        let mut vc1 = VectorClock::new();
        vc1.increment("node1");
        vc1.increment("node1");

        let mut vc2 = VectorClock::new();
        vc2.increment("node2");

        vc1.merge(&vc2);
        assert_eq!(vc1.get("node1"), 2);
        assert_eq!(vc1.get("node2"), 1);
    }

    #[test]
    fn test_happens_before() {
        let mut vc1 = VectorClock::new();
        vc1.increment("node1");

        let mut vc2 = VectorClock::new();
        vc2.increment("node1");
        vc2.increment("node1");

        assert!(vc1.happens_before(&vc2));
        assert!(!vc2.happens_before(&vc1));
    }

    #[test]
    fn test_concurrent() {
        let mut vc1 = VectorClock::new();
        vc1.increment("node1");

        let mut vc2 = VectorClock::new();
        vc2.increment("node2");

        assert!(vc1.is_concurrent(&vc2));
        assert!(vc2.is_concurrent(&vc1));
    }
}
