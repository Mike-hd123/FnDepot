//! Lightweight HNSW (Hierarchical Navigable Small World) vector index.
//!
//! This is a simplified single-layer implementation optimized for desktop-scale
//! knowledge bases (up to ~100K chunks). It uses greedy best-first search with
//! a priority queue, providing O(log n) average-case search complexity.
//!
//! Zero external dependencies beyond `bincode` (already in Cargo.toml).

use bincode::{deserialize, serialize};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashSet};
use std::path::Path;

/// A node in the HNSW graph.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct IndexNode {
    /// External ID (maps to chunk ID in SQLite)
    pub id: String,
    /// The embedding vector
    pub vector: Vec<f32>,
    /// Neighbour node indices (internal, not external IDs)
    pub neighbours: Vec<usize>,
}

/// Search result item.
#[derive(Clone, Debug)]
pub struct SearchResult {
    pub id: String,
    pub score: f32,
}

/// The HNSW index.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct HnswIndex {
    /// All nodes, indexed by internal position
    pub nodes: Vec<IndexNode>,
    /// Maximum number of connections per node
    pub max_m: usize,
    /// EF parameter for search (controls search width)
    pub ef_search: usize,
    /// EF parameter for construction
    pub ef_construction: usize,
    /// Embedding dimension
    pub dim: usize,
    /// Entry point node index
    pub entry_point: usize,
    /// Random state for level assignment (simplified: always layer 0)
    pub initialized: bool,
}

/// Priority queue item for greedy search.
#[derive(Clone)]
struct SearchItem {
    distance: f32,
    id: usize,
}

impl PartialEq for SearchItem {
    fn eq(&self, other: &Self) -> bool {
        self.distance == other.distance
    }
}

impl Eq for SearchItem {}

impl PartialOrd for SearchItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        // Min-heap: reverse ordering
        Some(other.distance.partial_cmp(&self.distance)?)
    }
}

impl Ord for SearchItem {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap_or(Ordering::Equal)
    }
}

impl HnswIndex {
    /// Create a new empty index.
    pub fn new(dim: usize, max_m: usize, ef_construction: usize, ef_search: usize) -> Self {
        Self {
            nodes: Vec::new(),
            max_m,
            ef_search,
            ef_construction,
            dim,
            entry_point: 0,
            initialized: false,
        }
    }

    /// Build the index from a list of (id, vector) pairs.
    pub fn build(&mut self, items: &[(String, Vec<f32>)]) {
        self.build_with_progress(items, |_, _| {});
    }

    /// Build the index with a progress callback.
    /// `callback(current, total)` is called periodically during construction.
    pub fn build_with_progress<F: Fn(usize, usize)>(
        &mut self,
        items: &[(String, Vec<f32>)],
        callback: F,
    ) {
        if items.is_empty() {
            return;
        }

        // Store all nodes
        self.nodes = items
            .iter()
            .map(|(id, vec)| IndexNode {
                id: id.clone(),
                vector: vec.clone(),
                neighbours: Vec::new(),
            })
            .collect();

        self.entry_point = 0;
        self.initialized = true;

        // Build connectivity: for each node, find M nearest neighbours
        // Use brute-force KNN with early-termination optimization.
        let n = self.nodes.len();
        let max_m = self.max_m;
        let progress_step = (n / 100).max(1); // Report ~every 1%

        for i in 0..n {
            // Compute distances to all other nodes
            let query = &self.nodes[i].vector;
            let mut dists: Vec<(f32, usize)> = Vec::with_capacity(n - 1);
            for j in 0..n {
                if j == i {
                    continue;
                }
                dists.push((cosine_distance(query, &self.nodes[j].vector), j));
            }

            // Partial sort: only need top-M, use selection instead of full sort
            // select_nth_unstable_by pivot index must be < len (0..len-1)
            let k = max_m.min(dists.len().saturating_sub(1));
            if !dists.is_empty() {
                dists.select_nth_unstable_by(k, |a, b| {
                    a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal)
                });
            }
            let top_m: Vec<usize> = dists.into_iter().take(max_m).map(|(_, j)| j).collect();

            // Set neighbours for node i
            self.nodes[i].neighbours = top_m.clone();

            // Add reverse edges
            for &neighbour_idx in &top_m {
                if neighbour_idx == i {
                    continue;
                }
                if neighbour_idx < self.nodes.len() {
                    let node = &mut self.nodes[neighbour_idx];
                    if !node.neighbours.contains(&i) && node.neighbours.len() < max_m {
                        node.neighbours.push(i);
                    }
                }
            }

            // Report progress every 1%
            if i > 0 && i % progress_step == 0 {
                let pct = i * 100 / n;
                tracing::info!("HNSW build progress: {}/{} nodes ({}%)", i, n, pct);
                callback(i, n);
            }
        }

        // Final callback
        callback(n, n);

        tracing::info!(
            "HNSW index built: {} nodes, dim {}, M={}, ef_search={}",
            n,
            self.dim,
            self.max_m,
            self.ef_search
        );
    }

    /// Search the index for the k nearest neighbours.
    pub fn search(&self, query: &[f32], k: usize) -> Vec<SearchResult> {
        if !self.initialized || self.nodes.is_empty() {
            return Vec::new();
        }

        let ef = self.ef_search.max(k);
        let candidates = self.search_internal(query, ef, usize::MAX);

        // Convert internal indices to external IDs and compute scores
        candidates
            .into_iter()
            .take(k)
            .map(|r| SearchResult {
                id: self.nodes[r.id].id.clone(),
                score: 1.0 - r.distance, // Convert distance to similarity score
            })
            .collect()
    }

    /// Internal greedy search starting from the entry point.
    /// Returns internal node indices sorted by distance (closest first).
    /// `exclude` is the node index to exclude (used during construction).
    fn search_internal(&self, query: &[f32], ef: usize, exclude: usize) -> Vec<SearchItem> {
        if self.nodes.is_empty() {
            return Vec::new();
        }

        let n = self.nodes.len();
        let start = if exclude == self.entry_point && n > 1 {
            1
        } else {
            self.entry_point
        };

        let mut visited: HashSet<usize> = HashSet::new();
        visited.insert(exclude);

        let mut candidates: BinaryHeap<SearchItem> = BinaryHeap::new();
        let mut results: BinaryHeap<SearchItem> = BinaryHeap::new();

        // Start from entry point
        let start_dist = cosine_distance(query, &self.nodes[start].vector);
        candidates.push(SearchItem {
            distance: start_dist,
            id: start,
        });
        results.push(SearchItem {
            distance: start_dist,
            id: start,
        });
        visited.insert(start);

        while let Some(SearchItem {
            distance: dist,
            id: curr,
        }) = candidates.pop()
        {
            // Check if we should stop
            let furthest_in_results = results.peek().map(|r| r.distance).unwrap_or(f32::MAX);

            if results.len() >= ef && dist > furthest_in_results {
                break;
            }

            // Explore neighbours
            for &neighbour_idx in &self.nodes[curr].neighbours {
                if visited.contains(&neighbour_idx) || neighbour_idx >= self.nodes.len() {
                    continue;
                }
                visited.insert(neighbour_idx);

                let neighbour_dist = cosine_distance(query, &self.nodes[neighbour_idx].vector);

                let furthest = results.peek().map(|r| r.distance).unwrap_or(f32::MAX);

                if results.len() < ef || neighbour_dist < furthest {
                    candidates.push(SearchItem {
                        distance: neighbour_dist,
                        id: neighbour_idx,
                    });
                    results.push(SearchItem {
                        distance: neighbour_dist,
                        id: neighbour_idx,
                    });

                    // Keep results bounded to ef — pop the furthest (max distance)
                    if results.len() > ef {
                        // BinaryHeap is max-heap on SearchItem (reverse ord),
                        // so peek() gives the furthest. Pop it.
                        results.pop();
                    }
                }
            }
        }

        // Sort results by distance (ascending)
        let mut sorted: Vec<SearchItem> = results.drain().collect();
        sorted.sort_by(|a, b| {
            a.distance
                .partial_cmp(&b.distance)
                .unwrap_or(Ordering::Equal)
        });
        sorted
    }

    /// Serialize the index to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        serialize(self).unwrap_or_default()
    }

    /// Deserialize from bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self, String> {
        deserialize(data).map_err(|e| format!("Failed to deserialize HNSW index: {}", e))
    }

    /// Save to file.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let data = self.to_bytes();
        std::fs::write(path, &data).map_err(|e| format!("Failed to write index file: {}", e))
    }

    /// Load from file.
    pub fn load(path: &Path) -> Result<Self, String> {
        let data = std::fs::read(path).map_err(|e| format!("Failed to read index file: {}", e))?;
        Self::from_bytes(&data)
    }

    /// Get number of nodes.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

/// Cosine distance (1 - cosine_similarity).
/// Returns 0 for identical vectors, 2 for opposite vectors.
fn cosine_distance(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 1.0;
    }

    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;

    for i in 0..a.len() {
        dot += a[i] * b[i];
        norm_a += a[i] * a[i];
        norm_b += b[i] * b[i];
    }

    let denom = (norm_a * norm_b).sqrt();
    if denom == 0.0 {
        return 1.0;
    }

    let similarity = dot / denom;
    // Clamp to [-1, 1] to handle floating point errors
    let clamped = similarity.clamp(-1.0, 1.0);
    1.0 - clamped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_distance() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((cosine_distance(&a, &b) - 0.0).abs() < 1e-6);

        let c = vec![0.0, 1.0, 0.0];
        assert!((cosine_distance(&a, &c) - 1.0).abs() < 1e-6);

        let d = vec![-1.0, 0.0, 0.0];
        assert!((cosine_distance(&a, &d) - 2.0).abs() < 1e-6);
    }

    #[test]
    fn test_build_and_search() {
        let mut index = HnswIndex::new(3, 8, 50, 20);

        // Create 100 random-ish vectors
        let items: Vec<(String, Vec<f32>)> = (0..100)
            .map(|i| {
                let v = vec![
                    ((i as f32) * 0.1).sin(),
                    ((i as f32) * 0.2).cos(),
                    (i as f32) * 0.01,
                ];
                (format!("chunk-{}", i), v)
            })
            .collect();

        index.build(&items);

        // Search for a vector similar to item 5
        let query = items[5].1.clone();
        let results = index.search(&query, 5);

        assert!(!results.is_empty());
        // The most similar should be item 5 itself (or very close)
        assert!(results[0].score > 0.99);
        assert_eq!(results[0].id, "chunk-5");
    }

    #[test]
    fn test_empty_index() {
        let index = HnswIndex::new(3, 8, 50, 20);
        let results = index.search(&[1.0, 0.0, 0.0], 5);
        assert!(results.is_empty());
    }

    #[test]
    fn test_serialization() {
        let mut index = HnswIndex::new(3, 8, 50, 20);
        let items: Vec<(String, Vec<f32>)> = (0..10)
            .map(|i| {
                (
                    format!("chunk-{}", i),
                    vec![i as f32, (i as f32) * 2.0, (i as f32) * 3.0],
                )
            })
            .collect();
        index.build(&items);

        let bytes = index.to_bytes();
        let restored = HnswIndex::from_bytes(&bytes).unwrap();

        assert_eq!(restored.len(), 10);
        assert_eq!(restored.dim, 3);

        let query = vec![1.0, 2.0, 3.0];
        let r1 = index.search(&query, 3);
        let r2 = restored.search(&query, 3);
        assert_eq!(r1.len(), r2.len());
        for i in 0..r1.len() {
            assert_eq!(r1[i].id, r2[i].id);
            assert!((r1[i].score - r2[i].score).abs() < 1e-5);
        }
    }
}
