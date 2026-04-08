//! Codebase RAG (Retrieval-Augmented Generation) indexing.
//!
//! Creates vector embeddings of code chunks across the entire codebase
//! for semantic search. When the AI needs context, it retrieves the most
//! relevant code snippets based on similarity to the query.
//!
//! Uses a simple TF-IDF approach (no external embedding model required)
//! with optional upgrade path to AI-generated embeddings.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A code chunk with its embedding and metadata.
#[derive(Debug, Clone)]
pub struct CodeChunk {
    /// File path relative to project root.
    pub file: PathBuf,
    /// Start line (0-indexed).
    pub start_line: usize,
    /// End line (0-indexed, exclusive).
    pub end_line: usize,
    /// The actual code text.
    pub text: String,
    /// TF-IDF term frequency vector (sparse).
    pub terms: HashMap<String, f32>,
}

/// The RAG index for a codebase.
pub struct RagIndex {
    /// All indexed code chunks.
    pub chunks: Vec<CodeChunk>,
    /// Inverse document frequency for each term.
    idf: HashMap<String, f32>,
    /// Project root directory.
    root: PathBuf,
    /// Total files indexed.
    pub file_count: usize,
}

impl RagIndex {
    /// Create an empty RAG index.
    pub fn new(root: PathBuf) -> Self {
        Self {
            chunks: Vec::new(),
            idf: HashMap::new(),
            root,
            file_count: 0,
        }
    }

    /// Index the entire codebase from the project root.
    ///
    /// Walks the directory tree, reads source files, splits into chunks,
    /// and computes TF-IDF embeddings.
    pub fn build(&mut self) {
        self.chunks.clear();
        self.idf.clear();
        self.file_count = 0;

        let files = collect_source_files(&self.root);
        self.file_count = files.len();

        // Phase 1: Build chunks from all files.
        for file in &files {
            if let Ok(content) = std::fs::read_to_string(file) {
                let rel_path = file.strip_prefix(&self.root).unwrap_or(file).to_path_buf();
                let file_chunks = split_into_chunks(&content, &rel_path);
                self.chunks.extend(file_chunks);
            }
        }

        // Phase 2: Compute IDF across all chunks.
        let n = self.chunks.len() as f32;
        let mut doc_freq: HashMap<String, usize> = HashMap::new();
        for chunk in &self.chunks {
            for term in chunk.terms.keys() {
                *doc_freq.entry(term.clone()).or_insert(0) += 1;
            }
        }
        for (term, df) in &doc_freq {
            self.idf.insert(term.clone(), (n / *df as f32).ln());
        }

        tracing::info!(
            "RAG index built: {} chunks from {} files",
            self.chunks.len(),
            self.file_count
        );
    }

    /// Incrementally update the index for a single file.
    pub fn update_file(&mut self, file: &Path) {
        let rel_path = file.strip_prefix(&self.root).unwrap_or(file).to_path_buf();

        // Remove old chunks for this file.
        self.chunks.retain(|c| c.file != rel_path);

        // Re-index the file.
        if let Ok(content) = std::fs::read_to_string(file) {
            let new_chunks = split_into_chunks(&content, &rel_path);
            self.chunks.extend(new_chunks);
        }

        // Recompute IDF (lightweight for incremental updates).
        self.recompute_idf();
    }

    /// Search the index for chunks most relevant to a query.
    ///
    /// Returns the top `limit` chunks sorted by TF-IDF cosine similarity.
    pub fn search(&self, query: &str, limit: usize) -> Vec<&CodeChunk> {
        let query_terms = tokenize(query);
        let mut query_vec: HashMap<String, f32> = HashMap::new();
        for term in &query_terms {
            *query_vec.entry(term.clone()).or_insert(0.0) += 1.0;
        }
        // Apply IDF to query.
        for (term, tf) in &mut query_vec {
            if let Some(idf) = self.idf.get(term) {
                *tf *= idf;
            }
        }

        // Score each chunk by cosine similarity.
        let mut scores: Vec<(usize, f32)> = self
            .chunks
            .iter()
            .enumerate()
            .map(|(i, chunk)| {
                let score = cosine_similarity(&query_vec, &chunk.terms, &self.idf);
                (i, score)
            })
            .filter(|(_, s)| *s > 0.0)
            .collect();

        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scores
            .iter()
            .take(limit)
            .map(|(i, _)| &self.chunks[*i])
            .collect()
    }

    /// Get a formatted context string from the top search results.
    pub fn retrieve_context(&self, query: &str, max_chunks: usize) -> String {
        let results = self.search(query, max_chunks);
        if results.is_empty() {
            return String::new();
        }
        let mut context = String::new();
        for chunk in results {
            context.push_str(&format!(
                "--- {} (lines {}-{}) ---\n{}\n\n",
                chunk.file.display(),
                chunk.start_line + 1,
                chunk.end_line,
                chunk.text
            ));
        }
        context
    }

    /// Recompute IDF values.
    fn recompute_idf(&mut self) {
        let n = self.chunks.len() as f32;
        if n == 0.0 {
            return;
        }
        let mut doc_freq: HashMap<String, usize> = HashMap::new();
        for chunk in &self.chunks {
            for term in chunk.terms.keys() {
                *doc_freq.entry(term.clone()).or_insert(0) += 1;
            }
        }
        self.idf.clear();
        for (term, df) in &doc_freq {
            self.idf.insert(term.clone(), (n / *df as f32).ln());
        }
    }
}

// ── Helpers ────────────────────────────────────────────────────

/// Source file extensions to index.
const SOURCE_EXTENSIONS: &[&str] = &[
    "rs", "py", "js", "ts", "tsx", "jsx", "go", "rb", "lua", "c", "cpp", "h", "hpp", "java", "kt",
    "swift", "zig", "ex", "exs", "erl", "hs", "ml", "scala", "sh", "bash", "zsh", "toml", "yaml",
    "yml", "json", "md", "sql", "html", "css", "scss", "vue", "svelte", "dart", "php",
];

/// Collect all source files under a directory.
fn collect_source_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_recursive(root, &mut files);
    files
}

fn collect_recursive(dir: &Path, files: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // Skip noise directories.
        if path.is_dir() {
            if matches!(
                name,
                ".git"
                    | "target"
                    | "node_modules"
                    | ".next"
                    | "dist"
                    | "build"
                    | "__pycache__"
                    | ".venv"
                    | "venv"
            ) {
                continue;
            }
            collect_recursive(&path, files);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| SOURCE_EXTENSIONS.contains(&ext))
        {
            files.push(path);
        }
    }
}

/// Split file content into chunks (~50 lines each, at function boundaries).
fn split_into_chunks(content: &str, rel_path: &Path) -> Vec<CodeChunk> {
    let lines: Vec<&str> = content.lines().collect();
    let mut chunks = Vec::new();
    let chunk_size = 50;

    let mut i = 0;
    while i < lines.len() {
        let end = (i + chunk_size).min(lines.len());
        let text: String = lines[i..end].join("\n");
        let terms = compute_tf(&text);
        chunks.push(CodeChunk {
            file: rel_path.to_path_buf(),
            start_line: i,
            end_line: end,
            text,
            terms,
        });
        i = end;
    }

    chunks
}

/// Tokenize text into lowercase terms.
fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|s| s.len() >= 2)
        .map(|s| s.to_lowercase())
        .collect()
}

/// Compute term frequency for a text chunk.
fn compute_tf(text: &str) -> HashMap<String, f32> {
    let tokens = tokenize(text);
    let total = tokens.len() as f32;
    if total == 0.0 {
        return HashMap::new();
    }
    let mut freq: HashMap<String, f32> = HashMap::new();
    for token in tokens {
        *freq.entry(token).or_insert(0.0) += 1.0;
    }
    for val in freq.values_mut() {
        *val /= total;
    }
    freq
}

/// Cosine similarity between a query vector and a document vector.
fn cosine_similarity(
    query: &HashMap<String, f32>,
    doc: &HashMap<String, f32>,
    idf: &HashMap<String, f32>,
) -> f32 {
    let mut dot = 0.0f32;
    let mut q_norm = 0.0f32;
    let mut d_norm = 0.0f32;

    for (term, q_val) in query {
        q_norm += q_val * q_val;
        if let Some(d_val) = doc.get(term) {
            let d_weighted = d_val * idf.get(term).unwrap_or(&1.0);
            dot += q_val * d_weighted;
        }
    }

    for (term, d_val) in doc {
        let d_weighted = d_val * idf.get(term).unwrap_or(&1.0);
        d_norm += d_weighted * d_weighted;
    }

    let denom = q_norm.sqrt() * d_norm.sqrt();
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize() {
        let tokens = tokenize("fn hello_world(x: i32) -> bool");
        assert!(tokens.contains(&"fn".to_string()));
        assert!(tokens.contains(&"hello_world".to_string()));
        assert!(tokens.contains(&"i32".to_string()));
        assert!(tokens.contains(&"bool".to_string()));
    }

    #[test]
    fn test_compute_tf() {
        let tf = compute_tf("hello world hello");
        assert!(tf.get("hello").unwrap() > tf.get("world").unwrap());
    }

    #[test]
    fn test_search_empty_index() {
        let idx = RagIndex::new(PathBuf::from("/tmp"));
        let results = idx.search("test query", 5);
        assert!(results.is_empty());
    }

    #[test]
    fn test_split_into_chunks() {
        let content = (0..120)
            .map(|i| format!("line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let chunks = split_into_chunks(&content, Path::new("test.rs"));
        assert_eq!(chunks.len(), 3); // 120 lines / 50 = 2.4 → 3 chunks
        assert_eq!(chunks[0].start_line, 0);
        assert_eq!(chunks[0].end_line, 50);
    }
}
