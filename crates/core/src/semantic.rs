//! Semantic graph: tracks symbols and their relationships.
//!
//! Provides a lightweight dependency graph of code symbols (functions,
//! structs, imports, tests) and their call/containment relationships.
//! Language-agnostic — the extraction layer populates this graph.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Unique identifier for a symbol in the graph.
pub type SymbolId = usize;

/// The kind of code symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Function,
    Method,
    Struct,
    Enum,
    Trait,
    Module,
    Import,
    Test,
    Constant,
}

impl std::fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SymbolKind::Function => write!(f, "fn"),
            SymbolKind::Method => write!(f, "method"),
            SymbolKind::Struct => write!(f, "struct"),
            SymbolKind::Enum => write!(f, "enum"),
            SymbolKind::Trait => write!(f, "trait"),
            SymbolKind::Module => write!(f, "mod"),
            SymbolKind::Import => write!(f, "use"),
            SymbolKind::Test => write!(f, "test"),
            SymbolKind::Constant => write!(f, "const"),
        }
    }
}

/// A code symbol (function, struct, etc.).
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub file_path: PathBuf,
    pub line_start: usize,
    pub line_end: usize,
    /// Parent scope (e.g. "impl Foo" for a method).
    pub scope: Option<String>,
}

/// The kind of relationship between two symbols.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationKind {
    /// Source calls target.
    Calls,
    /// Source tests target (test → function).
    Tests,
    /// Source contains target (struct → method).
    Contains,
}

/// A directed relationship between two symbols.
#[derive(Debug, Clone)]
pub struct Relation {
    pub source: SymbolId,
    pub target: SymbolId,
    pub kind: RelationKind,
}

/// Impact analysis result for a proposed change.
#[derive(Debug, Clone, Default)]
pub struct ImpactReport {
    /// Symbols that directly call or reference the changed symbol.
    pub direct_callers: Vec<SymbolId>,
    /// Symbols called by the changed symbol.
    pub callees: Vec<SymbolId>,
    /// Test functions that cover the changed symbol.
    pub affected_tests: Vec<SymbolId>,
}

/// In-memory graph of symbols and their relationships.
#[derive(Debug, Default)]
pub struct SemanticGraph {
    symbols: Vec<Symbol>,
    relations: Vec<Relation>,
    /// Name → list of symbol IDs for fast lookup.
    name_index: HashMap<String, Vec<SymbolId>>,
}

impl SemanticGraph {
    /// Create an empty graph.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a symbol to the graph. Returns its ID.
    pub fn add_symbol(&mut self, symbol: Symbol) -> SymbolId {
        let id = self.symbols.len();
        self.name_index
            .entry(symbol.name.clone())
            .or_default()
            .push(id);
        self.symbols.push(symbol);
        id
    }

    /// Add a relation between two symbols.
    pub fn add_relation(&mut self, relation: Relation) {
        self.relations.push(relation);
    }

    /// Remove all symbols and relations from a given file.
    pub fn clear_file(&mut self, path: &Path) {
        // Collect IDs to remove.
        let removed: Vec<SymbolId> = self
            .symbols
            .iter()
            .enumerate()
            .filter(|(_, s)| s.file_path == path)
            .map(|(id, _)| id)
            .collect();

        if removed.is_empty() {
            return;
        }

        // Remove relations involving removed symbols.
        self.relations
            .retain(|r| !removed.contains(&r.source) && !removed.contains(&r.target));

        // Mark removed symbols (we don't compact to preserve IDs).
        // Instead, clear their names from the index.
        for &id in &removed {
            let name = self.symbols[id].name.clone();
            if let Some(ids) = self.name_index.get_mut(&name) {
                ids.retain(|i| *i != id);
                if ids.is_empty() {
                    self.name_index.remove(&name);
                }
            }
            // Mark as removed by clearing the name.
            self.symbols[id].name.clear();
        }
    }

    /// Find the symbol enclosing a given line in a file.
    pub fn symbol_at(&self, file: &Path, line: usize) -> Option<(SymbolId, &Symbol)> {
        self.symbols
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                !s.name.is_empty()
                    && s.file_path == file
                    && line >= s.line_start
                    && line <= s.line_end
            })
            // Prefer the innermost (smallest span) symbol.
            .min_by_key(|(_, s)| s.line_end - s.line_start)
    }

    /// Look up symbols by name.
    pub fn symbols_named(&self, name: &str) -> Vec<(SymbolId, &Symbol)> {
        self.name_index
            .get(name)
            .map(|ids| {
                ids.iter()
                    .filter_map(|&id| {
                        let s = &self.symbols[id];
                        if s.name.is_empty() {
                            None
                        } else {
                            Some((id, s))
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get a symbol by ID.
    pub fn symbol(&self, id: SymbolId) -> Option<&Symbol> {
        self.symbols.get(id).filter(|s| !s.name.is_empty())
    }

    /// Find all callers of a symbol.
    pub fn callers_of(&self, id: SymbolId) -> Vec<SymbolId> {
        self.relations
            .iter()
            .filter(|r| r.target == id && r.kind == RelationKind::Calls)
            .map(|r| r.source)
            .collect()
    }

    /// Find all symbols called by a symbol.
    pub fn callees_of(&self, id: SymbolId) -> Vec<SymbolId> {
        self.relations
            .iter()
            .filter(|r| r.source == id && r.kind == RelationKind::Calls)
            .map(|r| r.target)
            .collect()
    }

    /// Find test functions that test a symbol.
    pub fn tests_for(&self, id: SymbolId) -> Vec<SymbolId> {
        self.relations
            .iter()
            .filter(|r| r.target == id && r.kind == RelationKind::Tests)
            .map(|r| r.source)
            .collect()
    }

    /// Compute impact report for a symbol.
    pub fn impact_of(&self, id: SymbolId) -> ImpactReport {
        ImpactReport {
            direct_callers: self.callers_of(id),
            callees: self.callees_of(id),
            affected_tests: self.tests_for(id),
        }
    }

    /// Generate a human/AI-readable context string for a symbol.
    pub fn context_string(&self, id: SymbolId) -> Option<String> {
        let sym = self.symbol(id)?;
        let mut parts = Vec::new();

        parts.push(format!(
            "{} {} (lines {}-{})",
            sym.kind,
            sym.name,
            sym.line_start + 1,
            sym.line_end + 1
        ));

        if let Some(scope) = &sym.scope {
            parts.push(format!("  Scope: {scope}"));
        }

        let callers = self.callers_of(id);
        if !callers.is_empty() {
            let names: Vec<&str> = callers
                .iter()
                .filter_map(|&cid| self.symbol(cid).map(|s| s.name.as_str()))
                .collect();
            parts.push(format!(
                "  Called by: {} ({})",
                names.join(", "),
                callers.len()
            ));
        }

        let callees = self.callees_of(id);
        if !callees.is_empty() {
            let names: Vec<&str> = callees
                .iter()
                .filter_map(|&cid| self.symbol(cid).map(|s| s.name.as_str()))
                .collect();
            parts.push(format!("  Calls: {}", names.join(", ")));
        }

        let tests = self.tests_for(id);
        if !tests.is_empty() {
            let names: Vec<&str> = tests
                .iter()
                .filter_map(|&tid| self.symbol(tid).map(|s| s.name.as_str()))
                .collect();
            parts.push(format!("  Tests: {}", names.join(", ")));
        }

        Some(parts.join("\n"))
    }

    /// Generate impact summary text for a set of changed lines.
    pub fn impact_summary(
        &self,
        file: &Path,
        start_line: usize,
        end_line: usize,
    ) -> Option<String> {
        // Find all symbols in the changed range.
        let affected_symbols: Vec<SymbolId> = self
            .symbols
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                !s.name.is_empty()
                    && s.file_path == file
                    && s.line_start <= end_line
                    && s.line_end >= start_line
            })
            .map(|(id, _)| id)
            .collect();

        if affected_symbols.is_empty() {
            return None;
        }

        let mut all_callers: Vec<String> = Vec::new();
        let mut all_tests: Vec<String> = Vec::new();

        for &sid in &affected_symbols {
            for cid in self.callers_of(sid) {
                if let Some(s) = self.symbol(cid) {
                    if !all_callers.contains(&s.name) {
                        all_callers.push(s.name.clone());
                    }
                }
            }
            for tid in self.tests_for(sid) {
                if let Some(s) = self.symbol(tid) {
                    if !all_tests.contains(&s.name) {
                        all_tests.push(s.name.clone());
                    }
                }
            }
        }

        let sym_names: Vec<&str> = affected_symbols
            .iter()
            .filter_map(|&id| self.symbol(id).map(|s| s.name.as_str()))
            .collect();

        let mut parts = Vec::new();
        parts.push(format!("Changed: {}", sym_names.join(", ")));

        if !all_callers.is_empty() {
            parts.push(format!("Affected: {}", all_callers.join(", ")));
        }
        if !all_tests.is_empty() {
            parts.push(format!("Tests: {}", all_tests.join(", ")));
        }

        Some(parts.join(" | "))
    }

    /// Total number of live symbols.
    pub fn symbol_count(&self) -> usize {
        self.symbols.iter().filter(|s| !s.name.is_empty()).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_graph() -> SemanticGraph {
        let mut g = SemanticGraph::new();
        let path = PathBuf::from("test.rs");

        let foo = g.add_symbol(Symbol {
            name: "foo".into(),
            kind: SymbolKind::Function,
            file_path: path.clone(),
            line_start: 0,
            line_end: 10,
            scope: None,
        });
        let bar = g.add_symbol(Symbol {
            name: "bar".into(),
            kind: SymbolKind::Function,
            file_path: path.clone(),
            line_start: 12,
            line_end: 20,
            scope: None,
        });
        let test_foo = g.add_symbol(Symbol {
            name: "test_foo".into(),
            kind: SymbolKind::Test,
            file_path: path.clone(),
            line_start: 22,
            line_end: 30,
            scope: None,
        });

        g.add_relation(Relation {
            source: bar,
            target: foo,
            kind: RelationKind::Calls,
        });
        g.add_relation(Relation {
            source: test_foo,
            target: foo,
            kind: RelationKind::Tests,
        });
        g.add_relation(Relation {
            source: test_foo,
            target: foo,
            kind: RelationKind::Calls,
        });

        g
    }

    #[test]
    fn test_callers() {
        let g = test_graph();
        let foo_ids = g.symbols_named("foo");
        assert_eq!(foo_ids.len(), 1);
        let callers = g.callers_of(foo_ids[0].0);
        assert_eq!(callers.len(), 2); // bar and test_foo
    }

    #[test]
    fn test_tests_for() {
        let g = test_graph();
        let foo_ids = g.symbols_named("foo");
        let tests = g.tests_for(foo_ids[0].0);
        assert_eq!(tests.len(), 1);
        assert_eq!(g.symbol(tests[0]).unwrap().name, "test_foo");
    }

    #[test]
    fn test_symbol_at() {
        let g = test_graph();
        let path = PathBuf::from("test.rs");
        let (id, sym) = g.symbol_at(&path, 5).unwrap();
        assert_eq!(sym.name, "foo");
        assert_eq!(id, 0);
    }

    #[test]
    fn test_impact_of() {
        let g = test_graph();
        let impact = g.impact_of(0); // foo
        assert_eq!(impact.direct_callers.len(), 2);
        assert_eq!(impact.affected_tests.len(), 1);
    }

    #[test]
    fn test_context_string() {
        let g = test_graph();
        let ctx = g.context_string(0).unwrap();
        assert!(ctx.contains("fn foo"));
        assert!(ctx.contains("Called by"));
        assert!(ctx.contains("Tests: test_foo"));
    }

    #[test]
    fn test_clear_file() {
        let mut g = test_graph();
        let path = PathBuf::from("test.rs");
        assert!(g.symbol_count() > 0);
        g.clear_file(&path);
        assert_eq!(g.symbol_count(), 0);
    }

    #[test]
    fn test_impact_summary() {
        let g = test_graph();
        let path = PathBuf::from("test.rs");
        let summary = g.impact_summary(&path, 0, 10).unwrap();
        assert!(summary.contains("foo"));
        assert!(summary.contains("Affected"));
    }
}
