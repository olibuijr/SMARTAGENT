//! Minimal HNSW (Hierarchical Navigable Small World) ANN index over
//! unit-normalized vectors. Distance = 1 - dot product. In-memory only;
//! rebuilt from storage on open (the log is the durable layer).

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashSet};

/// f32 wrapper with total ordering so it can live in heaps.
#[derive(PartialEq, Clone, Copy)]
struct Of32(f32);

impl Eq for Of32 {}
impl PartialOrd for Of32 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Of32 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.partial_cmp(&other.0).unwrap_or(Ordering::Equal)
    }
}

struct Node {
    /// Unit-normalized vector.
    vec: Vec<f32>,
    /// neighbors[level] = ids of connected nodes at that level.
    neighbors: Vec<Vec<usize>>,
    deleted: bool,
}

pub struct Hnsw {
    nodes: Vec<Node>,
    entry: Option<usize>,
    max_level: usize,
    m: usize,
    ef_construction: usize,
    rng: u64,
}

impl Hnsw {
    pub fn new() -> Hnsw {
        Hnsw {
            nodes: Vec::new(),
            entry: None,
            max_level: 0,
            m: 16,
            ef_construction: 100,
            rng: 0x9E37_79B9_7F4A_7C15,
        }
    }

    pub fn len(&self) -> usize {
        self.nodes.iter().filter(|n| !n.deleted).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn next_rand(&mut self) -> u64 {
        // xorshift64* — deterministic, std-only.
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn random_level(&mut self) -> usize {
        // Geometric distribution with p related to 1/ln(M).
        let mult = 1.0 / (self.m as f64).ln();
        let u = (self.next_rand() >> 11) as f64 / (1u64 << 53) as f64;
        let u = u.max(1e-12);
        (-(u.ln()) * mult) as usize
    }

    fn dist(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        1.0 - dot
    }

    /// Insert a unit-normalized vector; returns the internal node id.
    pub fn insert(&mut self, vec: Vec<f32>) -> usize {
        let id = self.nodes.len();
        let level = self.random_level();
        self.nodes.push(Node {
            vec,
            neighbors: vec![Vec::new(); level + 1],
            deleted: false,
        });

        let Some(mut ep) = self.entry else {
            self.entry = Some(id);
            self.max_level = level;
            return id;
        };

        // Greedy descend through levels above the new node's level.
        let q = self.nodes[id].vec.clone();
        for lev in (level + 1..=self.max_level).rev() {
            ep = self.greedy_closest(&q, ep, lev);
        }

        // Connect at each level from min(level, max_level) down to 0.
        for lev in (0..=level.min(self.max_level)).rev() {
            let found = self.search_layer(&q, ep, self.ef_construction, lev);
            let max_links = if lev == 0 { self.m * 2 } else { self.m };
            let chosen: Vec<usize> =
                found.iter().take(self.m).map(|&(_, n)| n).collect();
            for &n in &chosen {
                self.nodes[id].neighbors[lev].push(n);
                self.nodes[n].neighbors[lev].push(id);
                // Prune neighbor if it now exceeds its budget.
                if self.nodes[n].neighbors[lev].len() > max_links {
                    self.prune(n, lev, max_links);
                }
            }
            if let Some(&(_, best)) = found.first() {
                ep = best;
            }
        }

        if level > self.max_level {
            self.max_level = level;
            self.entry = Some(id);
        }
        id
    }

    pub fn mark_deleted(&mut self, id: usize) {
        if let Some(n) = self.nodes.get_mut(id) {
            n.deleted = true;
        }
    }

    fn prune(&mut self, node: usize, level: usize, max_links: usize) {
        let base = self.nodes[node].vec.clone();
        let mut scored: Vec<(Of32, usize)> = self.nodes[node].neighbors[level]
            .iter()
            .map(|&n| (Of32(Self::dist(&base, &self.nodes[n].vec)), n))
            .collect();
        scored.sort();
        scored.truncate(max_links);
        self.nodes[node].neighbors[level] = scored.into_iter().map(|(_, n)| n).collect();
    }

    fn greedy_closest(&self, q: &[f32], mut ep: usize, level: usize) -> usize {
        let mut best = Self::dist(q, &self.nodes[ep].vec);
        loop {
            let mut improved = false;
            for &n in &self.nodes[ep].neighbors[level] {
                let d = Self::dist(q, &self.nodes[n].vec);
                if d < best {
                    best = d;
                    ep = n;
                    improved = true;
                }
            }
            if !improved {
                return ep;
            }
        }
    }

    /// Beam search at one level. Returns (distance, id) sorted ascending.
    fn search_layer(&self, q: &[f32], ep: usize, ef: usize, level: usize) -> Vec<(f32, usize)> {
        let mut visited = HashSet::new();
        visited.insert(ep);
        let d0 = Self::dist(q, &self.nodes[ep].vec);
        // Candidates: min-heap by distance (via Reverse ordering trick).
        let mut candidates = BinaryHeap::new();
        candidates.push(std::cmp::Reverse((Of32(d0), ep)));
        // Results: max-heap by distance so we can evict the worst.
        let mut results = BinaryHeap::new();
        results.push((Of32(d0), ep));

        while let Some(std::cmp::Reverse((Of32(d), node))) = candidates.pop() {
            let worst = results.peek().map(|&(Of32(w), _)| w).unwrap_or(f32::MAX);
            if d > worst && results.len() >= ef {
                break;
            }
            for &n in &self.nodes[node].neighbors[level] {
                if !visited.insert(n) {
                    continue;
                }
                let dn = Self::dist(q, &self.nodes[n].vec);
                let worst = results.peek().map(|&(Of32(w), _)| w).unwrap_or(f32::MAX);
                if results.len() < ef || dn < worst {
                    candidates.push(std::cmp::Reverse((Of32(dn), n)));
                    results.push((Of32(dn), n));
                    if results.len() > ef {
                        results.pop();
                    }
                }
            }
        }
        let mut out: Vec<(f32, usize)> = results
            .into_iter()
            .map(|(Of32(d), n)| (d, n))
            .collect();
        out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal));
        out
    }

    /// k-NN search over live nodes. Returns (internal id, cosine similarity).
    pub fn search(&self, q: &[f32], k: usize, ef: usize) -> Vec<(usize, f32)> {
        let Some(mut ep) = self.entry else {
            return Vec::new();
        };
        for lev in (1..=self.max_level).rev() {
            ep = self.greedy_closest(q, ep, lev);
        }
        let ef = ef.max(k);
        self.search_layer(q, ep, ef, 0)
            .into_iter()
            .filter(|&(_, n)| !self.nodes[n].deleted)
            .take(k)
            .map(|(d, n)| (n, 1.0 - d))
            .collect()
    }
}

impl Default for Hnsw {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rand_unit(seed: &mut u64, dim: usize) -> Vec<f32> {
        let mut v = Vec::with_capacity(dim);
        for _ in 0..dim {
            *seed ^= *seed >> 12;
            *seed ^= *seed << 25;
            *seed ^= *seed >> 27;
            let r = (seed.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32;
            v.push(r / (1u64 << 24) as f32 - 0.5);
        }
        let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        v.iter().map(|x| x / n).collect()
    }

    #[test]
    fn finds_exact_match() {
        let mut h = Hnsw::new();
        let mut seed = 42u64;
        let mut vecs = Vec::new();
        for _ in 0..200 {
            let v = rand_unit(&mut seed, 16);
            h.insert(v.clone());
            vecs.push(v);
        }
        let hits = h.search(&vecs[50], 1, 50);
        assert_eq!(hits[0].0, 50);
        assert!(hits[0].1 > 0.999);
    }

    #[test]
    fn deleted_nodes_excluded() {
        let mut h = Hnsw::new();
        let mut seed = 7u64;
        for _ in 0..50 {
            let v = rand_unit(&mut seed, 8);
            h.insert(v);
        }
        let q = rand_unit(&mut seed, 8);
        let top = h.search(&q, 1, 30)[0].0;
        h.mark_deleted(top);
        let next = h.search(&q, 1, 30);
        assert!(next.is_empty() || next[0].0 != top);
    }
}
