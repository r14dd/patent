//! Semantic ranking.
//!
//! Embeds the idea and each match description with `fastembed`, computes cosine
//! similarity, sorts, and keeps the top N.

use crate::model::{Match, Query};

/// Default number of matches to keep after ranking.
pub const DEFAULT_LIMIT: usize = 50;

/// Cosine similarity between two equal-length vectors, in `[-1.0, 1.0]`.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na * nb)
    }
}

/// Score each match against the query embedding, sort by similarity descending,
/// and keep at most `limit`.
fn score_sort_limit(
    query_emb: &[f32],
    mut matches: Vec<Match>,
    match_embs: &[Vec<f32>],
    limit: usize,
) -> Vec<Match> {
    for (m, emb) in matches.iter_mut().zip(match_embs) {
        m.similarity = cosine(query_emb, emb);
    }
    matches.sort_by(|a, b| {
        b.similarity
            .partial_cmp(&a.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    matches.retain(|m| m.similarity >= 0.0);
    matches.truncate(limit);
    matches
}

/// Pre-loaded embedding model for reuse across pipeline stages.
///
/// Splitting model init from ranking lets the binary overlap the expensive model
/// load (~1-3 s) with I/O-bound source searches.
pub struct Ranker {
    model: fastembed::TextEmbedding,
}

/// Where the embedding model is cached between runs.
///
/// Defaults to a stable per-user cache directory (e.g. `~/.cache/patent` on
/// Linux, `~/Library/Caches/patent` on macOS) so the ~80 MB model downloads
/// once for the whole machine — not once per working directory, which is what
/// `fastembed`'s default CWD-relative `.fastembed_cache` would do.
fn model_cache_dir() -> Option<std::path::PathBuf> {
    dirs::cache_dir().map(|d| d.join("patent").join("fastembed"))
}

/// Whether the embedding model already appears to be cached locally.
///
/// Best-effort, used only so the binary can print a one-time "downloading…"
/// notice on the first run (the ~80 MB fetch otherwise looks like a hang before
/// `fastembed`'s own progress bar appears). If the cache dir can't be resolved
/// we assume it's present and stay quiet rather than risk a spurious notice.
pub fn model_is_cached() -> bool {
    match model_cache_dir() {
        Some(dir) => dir
            .read_dir()
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(false),
        None => true,
    }
}

impl Ranker {
    /// Load the embedding model. This is the expensive step; on the very first
    /// run it downloads ~80 MB into `model_cache_dir`.
    pub fn new() -> crate::Result<Self> {
        let mut opts = fastembed::InitOptions::new(fastembed::EmbeddingModel::AllMiniLML6V2)
            .with_show_download_progress(true);
        if let Some(dir) = model_cache_dir() {
            // Ensure the nested cache path exists before the downloader writes
            // into it (it won't always create intermediate directories).
            let _ = std::fs::create_dir_all(&dir);
            opts = opts.with_cache_dir(dir);
        }
        let model = fastembed::TextEmbedding::try_new(opts)
            .map_err(|e| crate::Error::Embedding(e.to_string()))?;
        Ok(Self { model })
    }

    /// Embed a single query string. Call while sources are still fetching.
    pub fn embed_query(&mut self, idea: &str) -> crate::Result<Vec<f32>> {
        let embs = self
            .model
            .embed(vec![idea], None)
            .map_err(|e| crate::Error::Embedding(e.to_string()))?;
        Ok(embs.into_iter().next().unwrap_or_default())
    }

    /// Rank matches against a pre-computed query embedding.
    pub fn rank_with(
        &mut self,
        query_emb: &[f32],
        matches: Vec<Match>,
        limit: usize,
    ) -> crate::Result<Vec<Match>> {
        if matches.is_empty() {
            return Ok(vec![]);
        }

        let texts: Vec<String> = matches
            .iter()
            .map(|m| {
                if m.description.is_empty() {
                    m.name.clone()
                } else {
                    format!("{}: {}", m.name, m.description)
                }
            })
            .collect();
        let descriptions: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        let match_embs = self
            .model
            .embed(descriptions, None)
            .map_err(|e| crate::Error::Embedding(e.to_string()))?;

        Ok(score_sort_limit(query_emb, matches, &match_embs, limit))
    }
}

/// Convenience wrapper: load model, embed, rank in one call.
pub fn rank(query: &Query, matches: Vec<Match>, limit: usize) -> crate::Result<Vec<Match>> {
    if matches.is_empty() {
        return Ok(vec![]);
    }
    let mut ranker = Ranker::new()?;
    let query_emb = ranker.embed_query(&query.idea)?;
    ranker.rank_with(&query_emb, matches, limit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Source;
    use std::sync::OnceLock;

    /// Shared ranker so the ~80 MB model downloads only once across all tests
    /// (parallel test threads race on the download otherwise).
    fn shared_ranker() -> &'static std::sync::Mutex<Ranker> {
        static RANKER: OnceLock<std::sync::Mutex<Ranker>> = OnceLock::new();
        RANKER.get_or_init(|| std::sync::Mutex::new(Ranker::new().unwrap()))
    }

    #[test]
    fn cosine_identical_is_one() {
        let v = [1.0, 2.0, 3.0];
        assert!((cosine(&v, &v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_orthogonal_is_zero() {
        assert!((cosine(&[1.0, 0.0], &[0.0, 1.0])).abs() < 1e-6);
    }

    #[test]
    fn cosine_zero_vector_is_zero() {
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
    }

    fn test_match(name: &str, desc: &str) -> Match {
        Match {
            name: name.to_string(),
            source: Source::CratesIo,
            url: format!("https://example.com/{name}"),
            description: desc.to_string(),
            popularity: None,
            similarity: 0.0,
        }
    }

    // -- score_sort_limit tests (pure logic, no fastembed) --------------------

    #[test]
    fn ssl_empty_input() {
        let result = score_sort_limit(&[1.0, 0.0], vec![], &[], 10);
        assert!(result.is_empty());
    }

    #[test]
    fn ssl_fills_similarity() {
        let q = vec![1.0, 0.0, 0.0];
        let matches = vec![test_match("a", "something")];
        let embs = vec![vec![0.8, 0.1, 0.0]];
        let result = score_sort_limit(&q, matches, &embs, 10);
        assert!(result[0].similarity > 0.0);
    }

    #[test]
    fn ssl_sorts_descending() {
        let q = vec![1.0, 0.0];
        let matches = vec![test_match("low", ""), test_match("high", "")];
        let embs = vec![
            vec![0.1, 0.9], // low similarity to [1, 0]
            vec![0.9, 0.1], // high similarity to [1, 0]
        ];
        let result = score_sort_limit(&q, matches, &embs, 10);
        assert_eq!(result[0].name, "high");
        assert_eq!(result[1].name, "low");
        assert!(result[0].similarity > result[1].similarity);
    }

    #[test]
    fn ssl_truncates_to_limit() {
        let q = vec![1.0, 0.0];
        let matches = vec![
            test_match("a", ""),
            test_match("b", ""),
            test_match("c", ""),
        ];
        let embs = vec![vec![1.0, 0.0], vec![0.5, 0.5], vec![0.0, 1.0]];
        let result = score_sort_limit(&q, matches, &embs, 2);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn ssl_fewer_than_limit_returns_all() {
        let q = vec![1.0, 0.0];
        let matches = vec![test_match("only", "")];
        let embs = vec![vec![1.0, 0.0]];
        let result = score_sort_limit(&q, matches, &embs, 10);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn ssl_preserves_match_fields() {
        let q = vec![1.0, 0.0];
        let mut m = test_match("foo", "bar");
        m.popularity = Some(42);
        let embs = vec![vec![0.9, 0.1]];
        let result = score_sort_limit(&q, vec![m], &embs, 10);
        assert_eq!(result[0].name, "foo");
        assert_eq!(result[0].description, "bar");
        assert_eq!(result[0].popularity, Some(42));
    }

    // -- rank() end-to-end tests (need fastembed model) -----------------------

    fn test_query() -> Query {
        Query {
            idea: "a fast async runtime for Rust".to_string(),
            keywords: vec!["async".to_string(), "runtime".to_string()],
        }
    }

    fn rank_via_shared(query: &Query, matches: Vec<Match>, limit: usize) -> Vec<Match> {
        if matches.is_empty() {
            return vec![];
        }
        let mut ranker = shared_ranker().lock().unwrap();
        let query_emb = ranker.embed_query(&query.idea).unwrap();
        ranker.rank_with(&query_emb, matches, limit).unwrap()
    }

    #[test]
    fn rank_empty_matches_returns_empty() {
        let result = rank_via_shared(&test_query(), vec![], 10);
        assert!(result.is_empty());
    }

    #[test]
    fn rank_fills_positive_similarity_for_related_content() {
        let matches = vec![test_match(
            "tokio",
            "An event-driven async runtime for Rust",
        )];
        let result = rank_via_shared(&test_query(), matches, 10);
        assert!(
            result[0].similarity > 0.0,
            "related content must have positive similarity"
        );
    }

    #[test]
    fn rank_orders_relevant_above_irrelevant() {
        let matches = vec![
            test_match("recipes", "A collection of baking recipes and kitchen tips"),
            test_match(
                "tokio",
                "An event-driven non-blocking I/O platform for async Rust",
            ),
        ];
        let result = rank_via_shared(&test_query(), matches, 10);
        assert_eq!(result[0].name, "tokio");
    }

    #[test]
    fn rank_respects_limit() {
        let matches = vec![
            test_match("a", "async runtime alpha"),
            test_match("b", "async runtime beta"),
            test_match("c", "async runtime gamma"),
            test_match("d", "async runtime delta"),
        ];
        let result = rank_via_shared(&test_query(), matches, 2);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn rank_returns_sorted_descending() {
        let matches = vec![
            test_match("recipes", "Baking sourdough bread at home"),
            test_match("smol", "A small async runtime"),
            test_match("tokio", "An async runtime for Rust applications"),
        ];
        let result = rank_via_shared(&test_query(), matches, 10);
        for pair in result.windows(2) {
            assert!(
                pair[0].similarity >= pair[1].similarity,
                "{} ({}) should be >= {} ({})",
                pair[0].name,
                pair[0].similarity,
                pair[1].name,
                pair[1].similarity,
            );
        }
    }
}
