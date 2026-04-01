//! Tree-sitter based symbol extraction for the semantic graph.
//!
//! Parses source files using tree-sitter and extracts function definitions,
//! call sites, struct/enum/trait definitions, and test functions. Populates
//! the [`SemanticGraph`] in `aura_core`.

use crate::highlight::Language;
use aura_core::semantic::{Relation, RelationKind, SemanticGraph, Symbol, SymbolKind};
use std::path::Path;
use tree_sitter::{Node, Parser};

/// Builds and maintains a semantic graph from tree-sitter parses.
pub struct SemanticIndexer {
    graph: SemanticGraph,
    parser: Parser,
    /// Unresolved call references: (caller_id, callee_name).
    deferred_calls: Vec<(usize, String)>,
    /// Unresolved test links: (test_id, tested_name).
    deferred_tests: Vec<(usize, String)>,
}

impl SemanticIndexer {
    /// Create a new indexer for the given language.
    pub fn new(language: Language) -> Option<Self> {
        let ts_language: tree_sitter::Language = match language {
            Language::Rust => tree_sitter_rust::LANGUAGE.into(),
            Language::Python => tree_sitter_python::LANGUAGE.into(),
            Language::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Language::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Language::Go => tree_sitter_go::LANGUAGE.into(),
            Language::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Language::Java => tree_sitter_java::LANGUAGE.into(),
            Language::C => tree_sitter_c::LANGUAGE.into(),
            Language::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            Language::Ruby => tree_sitter_ruby::LANGUAGE.into(),
            // Languages without semantic indexing support yet — return None.
            Language::Html
            | Language::Css
            | Language::Json
            | Language::Bash
            | Language::Toml
            | Language::Yaml
            | Language::Markdown
            | Language::Elixir
            | Language::HEEx
            | Language::Dotenv => return None,
        };

        let mut parser = Parser::new();
        parser.set_language(&ts_language).ok()?;

        Some(Self {
            graph: SemanticGraph::new(),
            parser,
            deferred_calls: Vec::new(),
            deferred_tests: Vec::new(),
        })
    }

    /// Get a reference to the current graph.
    pub fn graph(&self) -> &SemanticGraph {
        &self.graph
    }

    /// Re-index a file. Clears old data for this file first.
    pub fn index_file(&mut self, path: &Path, source: &str, language: Language) {
        self.graph.clear_file(path);
        self.deferred_calls.clear();
        self.deferred_tests.clear();

        let tree = match self.parser.parse(source, None) {
            Some(t) => t,
            None => return,
        };

        let root = tree.root_node();

        match language {
            Language::Rust => self.extract_rust(root, source, path),
            Language::Python => self.extract_python(root, source, path),
            Language::TypeScript | Language::Tsx | Language::JavaScript => {
                self.extract_typescript(root, source, path)
            }
            Language::Go => self.extract_go(root, source, path),
            Language::Java | Language::C | Language::Cpp | Language::Ruby => {
                // Basic extraction using generic C-like patterns.
                self.extract_rust(root, source, path)
            }
            _ => {} // No semantic extraction for markup/config languages.
        }

        self.resolve_deferred();
    }

    // ── Rust extraction ───────────────────────────────────────────

    fn extract_rust(&mut self, root: Node, source: &str, path: &Path) {
        let mut cursor = root.walk();
        for node in root.children(&mut cursor) {
            self.extract_rust_node(node, source, path, None);
        }
    }

    fn extract_rust_node(&mut self, node: Node, source: &str, path: &Path, scope: Option<&str>) {
        match node.kind() {
            "function_item" => {
                if let Some(name) = child_field_text(&node, "name", source) {
                    let is_test = has_test_attribute(&node, source);
                    let kind = if is_test {
                        SymbolKind::Test
                    } else if scope.is_some() {
                        SymbolKind::Method
                    } else {
                        SymbolKind::Function
                    };

                    let sym_id = self.graph.add_symbol(Symbol {
                        name: name.clone(),
                        kind,
                        file_path: path.to_path_buf(),
                        line_start: node.start_position().row,
                        line_end: node.end_position().row,
                        scope: scope.map(String::from),
                    });

                    if let Some(body) = node.child_by_field_name("body") {
                        self.extract_calls(body, source, sym_id);
                    }

                    if is_test {
                        if let Some(tested) = name.strip_prefix("test_") {
                            self.deferred_tests.push((sym_id, tested.to_string()));
                        }
                    }
                }
            }
            "struct_item" => {
                if let Some(name) = child_field_text(&node, "name", source) {
                    self.graph.add_symbol(Symbol {
                        name,
                        kind: SymbolKind::Struct,
                        file_path: path.to_path_buf(),
                        line_start: node.start_position().row,
                        line_end: node.end_position().row,
                        scope: scope.map(String::from),
                    });
                }
            }
            "enum_item" => {
                if let Some(name) = child_field_text(&node, "name", source) {
                    self.graph.add_symbol(Symbol {
                        name,
                        kind: SymbolKind::Enum,
                        file_path: path.to_path_buf(),
                        line_start: node.start_position().row,
                        line_end: node.end_position().row,
                        scope: scope.map(String::from),
                    });
                }
            }
            "trait_item" => {
                if let Some(name) = child_field_text(&node, "name", source) {
                    self.graph.add_symbol(Symbol {
                        name,
                        kind: SymbolKind::Trait,
                        file_path: path.to_path_buf(),
                        line_start: node.start_position().row,
                        line_end: node.end_position().row,
                        scope: scope.map(String::from),
                    });
                }
            }
            "const_item" | "static_item" => {
                if let Some(name) = child_field_text(&node, "name", source) {
                    self.graph.add_symbol(Symbol {
                        name,
                        kind: SymbolKind::Constant,
                        file_path: path.to_path_buf(),
                        line_start: node.start_position().row,
                        line_end: node.end_position().row,
                        scope: scope.map(String::from),
                    });
                }
            }
            "impl_item" => {
                let impl_scope = child_field_text(&node, "type", source)
                    .map(|t| format!("impl {t}"))
                    .unwrap_or_else(|| "impl".into());
                if let Some(body) = node.child_by_field_name("body") {
                    let mut child_cursor = body.walk();
                    for child in body.children(&mut child_cursor) {
                        self.extract_rust_node(child, source, path, Some(&impl_scope));
                    }
                }
            }
            "mod_item" => {
                if let Some(name) = child_field_text(&node, "name", source) {
                    let mod_scope = format!("mod {name}");
                    self.graph.add_symbol(Symbol {
                        name,
                        kind: SymbolKind::Module,
                        file_path: path.to_path_buf(),
                        line_start: node.start_position().row,
                        line_end: node.end_position().row,
                        scope: scope.map(String::from),
                    });
                    if let Some(body) = node.child_by_field_name("body") {
                        let mut child_cursor = body.walk();
                        for child in body.children(&mut child_cursor) {
                            self.extract_rust_node(child, source, path, Some(&mod_scope));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // ── Python extraction ─────────────────────────────────────────

    fn extract_python(&mut self, root: Node, source: &str, path: &Path) {
        let mut cursor = root.walk();
        for node in root.children(&mut cursor) {
            match node.kind() {
                "function_definition" => {
                    if let Some(name) = child_field_text(&node, "name", source) {
                        let is_test = name.starts_with("test_");
                        let kind = if is_test {
                            SymbolKind::Test
                        } else {
                            SymbolKind::Function
                        };
                        let sym_id = self.graph.add_symbol(Symbol {
                            name: name.clone(),
                            kind,
                            file_path: path.to_path_buf(),
                            line_start: node.start_position().row,
                            line_end: node.end_position().row,
                            scope: None,
                        });
                        if let Some(body) = node.child_by_field_name("body") {
                            self.extract_calls(body, source, sym_id);
                        }
                        if is_test {
                            if let Some(tested) = name.strip_prefix("test_") {
                                self.deferred_tests.push((sym_id, tested.to_string()));
                            }
                        }
                    }
                }
                "class_definition" => {
                    if let Some(name) = child_field_text(&node, "name", source) {
                        self.graph.add_symbol(Symbol {
                            name,
                            kind: SymbolKind::Struct,
                            file_path: path.to_path_buf(),
                            line_start: node.start_position().row,
                            line_end: node.end_position().row,
                            scope: None,
                        });
                    }
                }
                _ => {}
            }
        }
    }

    // ── TypeScript extraction ─────────────────────────────────────

    fn extract_typescript(&mut self, root: Node, source: &str, path: &Path) {
        self.walk_ts_children(root, source, path, None);
    }

    fn walk_ts_children(&mut self, node: Node, source: &str, path: &Path, scope: Option<&str>) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "function_declaration" => {
                    if let Some(name) = child_field_text(&child, "name", source) {
                        let sym_id = self.graph.add_symbol(Symbol {
                            name,
                            kind: SymbolKind::Function,
                            file_path: path.to_path_buf(),
                            line_start: child.start_position().row,
                            line_end: child.end_position().row,
                            scope: scope.map(String::from),
                        });
                        if let Some(body) = child.child_by_field_name("body") {
                            self.extract_calls(body, source, sym_id);
                        }
                    }
                }
                "class_declaration" => {
                    if let Some(name) = child_field_text(&child, "name", source) {
                        self.graph.add_symbol(Symbol {
                            name: name.clone(),
                            kind: SymbolKind::Struct,
                            file_path: path.to_path_buf(),
                            line_start: child.start_position().row,
                            line_end: child.end_position().row,
                            scope: None,
                        });
                        if let Some(body) = child.child_by_field_name("body") {
                            self.walk_ts_children(body, source, path, Some(&name));
                        }
                    }
                }
                "interface_declaration" => {
                    if let Some(name) = child_field_text(&child, "name", source) {
                        self.graph.add_symbol(Symbol {
                            name,
                            kind: SymbolKind::Trait,
                            file_path: path.to_path_buf(),
                            line_start: child.start_position().row,
                            line_end: child.end_position().row,
                            scope: None,
                        });
                    }
                }
                _ => {
                    self.walk_ts_children(child, source, path, scope);
                }
            }
        }
    }

    // ── Go extraction ─────────────────────────────────────────────

    fn extract_go(&mut self, root: Node, source: &str, path: &Path) {
        let mut cursor = root.walk();
        for node in root.children(&mut cursor) {
            match node.kind() {
                "function_declaration" => {
                    if let Some(name) = child_field_text(&node, "name", source) {
                        let is_test = name.starts_with("Test");
                        let kind = if is_test {
                            SymbolKind::Test
                        } else {
                            SymbolKind::Function
                        };
                        let sym_id = self.graph.add_symbol(Symbol {
                            name: name.clone(),
                            kind,
                            file_path: path.to_path_buf(),
                            line_start: node.start_position().row,
                            line_end: node.end_position().row,
                            scope: None,
                        });
                        if let Some(body) = node.child_by_field_name("body") {
                            self.extract_calls(body, source, sym_id);
                        }
                        if is_test {
                            if let Some(tested) = name.strip_prefix("Test") {
                                self.deferred_tests.push((sym_id, tested.to_string()));
                            }
                        }
                    }
                }
                "method_declaration" => {
                    if let Some(name) = child_field_text(&node, "name", source) {
                        let receiver = node
                            .child_by_field_name("receiver")
                            .map(|r| node_text(&r, source));
                        let sym_id = self.graph.add_symbol(Symbol {
                            name,
                            kind: SymbolKind::Method,
                            file_path: path.to_path_buf(),
                            line_start: node.start_position().row,
                            line_end: node.end_position().row,
                            scope: receiver,
                        });
                        if let Some(body) = node.child_by_field_name("body") {
                            self.extract_calls(body, source, sym_id);
                        }
                    }
                }
                "type_declaration" => {
                    let mut child_cursor = node.walk();
                    for child in node.children(&mut child_cursor) {
                        if child.kind() == "type_spec" {
                            if let Some(name) = child_field_text(&child, "name", source) {
                                self.graph.add_symbol(Symbol {
                                    name,
                                    kind: SymbolKind::Struct,
                                    file_path: path.to_path_buf(),
                                    line_start: child.start_position().row,
                                    line_end: child.end_position().row,
                                    scope: None,
                                });
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // ── Call extraction (language-agnostic) ────────────────────────

    fn extract_calls(&mut self, node: Node, source: &str, caller_id: usize) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "call_expression" => {
                    if let Some(func) = child.child_by_field_name("function") {
                        let call_name = node_text(&func, source);
                        // Strip receiver/path: foo.bar() → bar, foo::bar() → bar
                        let simple = call_name
                            .rsplit('.')
                            .next()
                            .unwrap_or(&call_name)
                            .rsplit("::")
                            .next()
                            .unwrap_or(&call_name);
                        self.deferred_calls.push((caller_id, simple.to_string()));
                    }
                    // Also walk children for nested calls.
                    self.extract_calls(child, source, caller_id);
                }
                "macro_invocation" => {
                    if let Some(name_node) = child.child(0) {
                        let name = node_text(&name_node, source);
                        let name = name.trim_end_matches('!');
                        self.deferred_calls.push((caller_id, name.to_string()));
                    }
                }
                _ => {
                    self.extract_calls(child, source, caller_id);
                }
            }
        }
    }

    // ── Deferred resolution ───────────────────────────────────────

    fn resolve_deferred(&mut self) {
        let calls: Vec<(usize, String)> = self.deferred_calls.drain(..).collect();
        // Collect targets first to avoid borrow conflict.
        let call_relations: Vec<(usize, usize)> = calls
            .iter()
            .flat_map(|(caller_id, callee_name)| {
                self.graph
                    .symbols_named(callee_name)
                    .into_iter()
                    .filter(|(target_id, _)| target_id != caller_id)
                    .map(|(target_id, _)| (*caller_id, target_id))
                    .collect::<Vec<_>>()
            })
            .collect();
        for (source, target) in call_relations {
            self.graph.add_relation(Relation {
                source,
                target,
                kind: RelationKind::Calls,
            });
        }

        let tests: Vec<(usize, String)> = self.deferred_tests.drain(..).collect();
        let test_relations: Vec<(usize, usize)> = tests
            .iter()
            .flat_map(|(test_id, tested_name)| {
                self.graph
                    .symbols_named(tested_name)
                    .into_iter()
                    .map(|(target_id, _)| (*test_id, target_id))
                    .collect::<Vec<_>>()
            })
            .collect();
        for (source, target) in test_relations {
            self.graph.add_relation(Relation {
                source,
                target,
                kind: RelationKind::Tests,
            });
        }
    }
}

// ── Helper functions ──────────────────────────────────────────────

/// Get the text of a node's named field.
fn child_field_text(node: &Node, field: &str, source: &str) -> Option<String> {
    node.child_by_field_name(field)
        .map(|n| node_text(&n, source))
}

/// Get the text content of a node.
fn node_text(node: &Node, source: &str) -> String {
    source[node.start_byte()..node.end_byte()].to_string()
}

/// Check if a Rust function node has a `#[test]` attribute.
fn has_test_attribute(node: &Node, source: &str) -> bool {
    // Check children of the function node.
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "attribute_item" || child.kind() == "attribute" {
            let text = node_text(&child, source);
            if text.contains("test") {
                return true;
            }
        }
    }

    // Also check preceding siblings (tree-sitter Rust puts attributes as siblings).
    let mut prev = node.prev_sibling();
    while let Some(sibling) = prev {
        if sibling.kind() == "attribute_item" {
            let text = node_text(&sibling, source);
            if text.contains("test") {
                return true;
            }
        } else if sibling.kind() != "line_comment" && sibling.kind() != "block_comment" {
            break;
        }
        prev = sibling.prev_sibling();
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_rust_extraction() {
        let source = r#"
fn foo() {
    bar();
}

fn bar() {
    println!("hello");
}

#[test]
fn test_foo() {
    foo();
}
"#;
        let mut indexer = SemanticIndexer::new(Language::Rust).unwrap();
        let path = PathBuf::from("test.rs");
        indexer.index_file(&path, source, Language::Rust);

        let graph = indexer.graph();
        assert!(graph.symbol_count() >= 3); // foo, bar, test_foo

        // foo should be called by test_foo
        let foo_syms = graph.symbols_named("foo");
        assert!(!foo_syms.is_empty());
        let (foo_id, _) = foo_syms[0];
        let callers = graph.callers_of(foo_id);
        assert!(!callers.is_empty());

        // test_foo should test foo
        let tests = graph.tests_for(foo_id);
        assert!(!tests.is_empty());
    }

    #[test]
    fn test_rust_impl_methods() {
        let source = r#"
struct MyStruct;

impl MyStruct {
    fn new() -> Self {
        MyStruct
    }

    fn process(&self) {
        self.helper();
    }

    fn helper(&self) {}
}
"#;
        let mut indexer = SemanticIndexer::new(Language::Rust).unwrap();
        let path = PathBuf::from("test.rs");
        indexer.index_file(&path, source, Language::Rust);

        let graph = indexer.graph();
        let methods = graph.symbols_named("process");
        assert!(!methods.is_empty());
        let (_, sym) = &methods[0];
        assert_eq!(sym.kind, SymbolKind::Method);
        assert!(sym.scope.as_deref().unwrap().contains("MyStruct"));
    }

    #[test]
    fn test_python_extraction() {
        let source = r#"
def foo():
    bar()

def bar():
    print("hello")

def test_foo():
    foo()
"#;
        let mut indexer = SemanticIndexer::new(Language::Python).unwrap();
        let path = PathBuf::from("test.py");
        indexer.index_file(&path, source, Language::Python);

        let graph = indexer.graph();
        assert!(graph.symbol_count() >= 3);
    }

    #[test]
    fn test_context_string() {
        let source = r#"
fn foo() {
    bar();
}

fn bar() {}

#[test]
fn test_foo() {
    foo();
}
"#;
        let mut indexer = SemanticIndexer::new(Language::Rust).unwrap();
        let path = PathBuf::from("test.rs");
        indexer.index_file(&path, source, Language::Rust);

        let graph = indexer.graph();
        let foo_syms = graph.symbols_named("foo");
        let (foo_id, _) = foo_syms[0];
        let ctx = graph.context_string(foo_id).unwrap();
        assert!(ctx.contains("fn foo"));
    }

    #[test]
    fn test_reindex_clears() {
        let source1 = "fn alpha() {}\nfn beta() {}";
        let source2 = "fn gamma() {}";

        let mut indexer = SemanticIndexer::new(Language::Rust).unwrap();
        let path = PathBuf::from("test.rs");

        indexer.index_file(&path, source1, Language::Rust);
        assert!(indexer.graph().symbols_named("alpha").len() == 1);

        indexer.index_file(&path, source2, Language::Rust);
        assert!(indexer.graph().symbols_named("alpha").is_empty());
        assert!(indexer.graph().symbols_named("gamma").len() == 1);
    }
}
