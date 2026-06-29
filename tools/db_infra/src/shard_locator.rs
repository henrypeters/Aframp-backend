use crc32fast::Hasher;
use std::collections::BTreeMap;

/// Simple consistent-hash ring using crc32 and virtual nodes.
pub struct ShardLocator {
    ring: BTreeMap<u32, String>,
    replicas: usize,
}

impl ShardLocator {
    pub fn new(nodes: Vec<String>, replicas: usize) -> Self {
        let mut ring = BTreeMap::new();
        for node in nodes.iter() {
            for i in 0..replicas {
                let mut h = Hasher::new();
                h.update(node.as_bytes());
                h.update(&i.to_le_bytes());
                let key = h.finalize();
                ring.insert(key, node.clone());
            }
        }
        ShardLocator { ring, replicas }
    }

    pub fn locate(&self, key: &str) -> Option<&str> {
        if self.ring.is_empty() {
            return None;
        }
        let mut h = Hasher::new();
        h.update(key.as_bytes());
        let hash = h.finalize();
        // find first entry with key >= hash
        let tail = self.ring.range(hash..).next();
        if let Some((_, node)) = tail {
            return Some(node.as_str());
        }
        // wrap
        self.ring.iter().next().map(|(_, v)| v.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::ShardLocator;
    use std::collections::HashMap;

    #[test]
    fn balanced_distribution() {
        let nodes = vec!["shard-a".to_string(), "shard-b".to_string(), "shard-c".to_string()];
        let ring = ShardLocator::new(nodes, 128);
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for i in 0..1000u32 {
            let key = format!("account-{}", i);
            let node = ring.locate(&key).unwrap();
            *counts.entry(node).or_default() += 1;
        }
        // ensure roughly balanced
        for (&_node, &count) in counts.iter() {
            assert!(count > 200, "unbalanced distribution: {}", count);
        }
    }
}
