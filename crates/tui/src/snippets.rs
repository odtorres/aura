//! Tab-triggered code snippet system.
//!
//! Supports VS Code-style snippet format with `${1:placeholder}` syntax.
//! Built-in snippets per language + user-defined snippets from `~/.aura/snippets/`.

use std::collections::HashMap;
use std::path::PathBuf;

/// A code snippet template.
#[derive(Debug, Clone)]
pub struct Snippet {
    /// Trigger word (e.g., "fn").
    pub trigger: String,
    /// Template body with placeholders (e.g., "fn ${1:name}() {\n    $0\n}").
    pub body: String,
    /// Human-readable description.
    pub description: String,
}

/// A parsed placeholder in an expanded snippet.
#[derive(Debug, Clone)]
pub struct Placeholder {
    /// Character offset in the expanded text where this placeholder starts.
    pub offset: usize,
    /// Length of the default text.
    pub length: usize,
    /// The default text.
    pub default: String,
    /// Placeholder number (0 = final cursor position).
    pub number: usize,
}

/// Tracks an active snippet expansion with placeholder navigation.
#[derive(Debug, Clone)]
pub struct ActiveSnippet {
    /// All placeholders sorted by number.
    pub placeholders: Vec<Placeholder>,
    /// Index of the currently focused placeholder.
    pub current: usize,
    /// Character offset where the snippet was inserted in the buffer.
    pub insert_offset: usize,
}

impl ActiveSnippet {
    /// Get the current placeholder (if any remain).
    pub fn current_placeholder(&self) -> Option<&Placeholder> {
        self.placeholders.get(self.current)
    }

    /// Advance to the next placeholder. Returns false if done.
    pub fn next_placeholder(&mut self) -> bool {
        if self.current + 1 < self.placeholders.len() {
            self.current += 1;
            true
        } else {
            false
        }
    }

    /// Check if the snippet is fully navigated.
    pub fn is_done(&self) -> bool {
        self.current >= self.placeholders.len()
    }
}

/// Manages snippets and active snippet state.
pub struct SnippetEngine {
    /// Snippets grouped by language (None key = all languages).
    snippets: HashMap<Option<String>, Vec<Snippet>>,
    /// Currently active snippet (being navigated).
    pub active: Option<ActiveSnippet>,
}

impl SnippetEngine {
    /// Create a new engine with built-in snippets.
    pub fn new() -> Self {
        let mut engine = Self {
            snippets: HashMap::new(),
            active: None,
        };
        engine.load_builtins();
        engine.load_user_snippets();
        engine
    }

    /// Find a snippet matching the trigger for the given language.
    pub fn find(&self, trigger: &str, language: Option<&str>) -> Option<&Snippet> {
        // Check language-specific first.
        if let Some(lang) = language {
            if let Some(snippets) = self.snippets.get(&Some(lang.to_string())) {
                if let Some(s) = snippets.iter().find(|s| s.trigger == trigger) {
                    return Some(s);
                }
            }
        }
        // Check generic snippets.
        if let Some(snippets) = self.snippets.get(&None) {
            if let Some(s) = snippets.iter().find(|s| s.trigger == trigger) {
                return Some(s);
            }
        }
        None
    }

    /// Expand a snippet body, returning (expanded_text, placeholders).
    pub fn expand(body: &str, indent: &str) -> (String, Vec<Placeholder>) {
        let mut result = String::new();
        let mut placeholders = Vec::new();
        let mut chars = body.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '$' {
                if let Some(&next) = chars.peek() {
                    if next == '{' {
                        // ${N:default} format
                        chars.next(); // consume '{'
                        let mut num_str = String::new();
                        while let Some(&c) = chars.peek() {
                            if c == ':' || c == '}' {
                                break;
                            }
                            num_str.push(c);
                            chars.next();
                        }
                        let num: usize = num_str.parse().unwrap_or(0);
                        let mut default = String::new();
                        if chars.peek() == Some(&':') {
                            chars.next(); // consume ':'
                            while let Some(&c) = chars.peek() {
                                if c == '}' {
                                    break;
                                }
                                default.push(c);
                                chars.next();
                            }
                        }
                        if chars.peek() == Some(&'}') {
                            chars.next(); // consume '}'
                        }
                        placeholders.push(Placeholder {
                            offset: result.len(),
                            length: default.len(),
                            default: default.clone(),
                            number: num,
                        });
                        result.push_str(&default);
                    } else if next.is_ascii_digit() {
                        // $N format (no default)
                        chars.next();
                        let num = (next as u8 - b'0') as usize;
                        placeholders.push(Placeholder {
                            offset: result.len(),
                            length: 0,
                            default: String::new(),
                            number: num,
                        });
                    } else {
                        result.push('$');
                    }
                } else {
                    result.push('$');
                }
            } else if ch == '\\' && chars.peek() == Some(&'n') {
                chars.next();
                result.push('\n');
                result.push_str(indent);
            } else {
                result.push(ch);
            }
        }

        // Sort placeholders by number (0 = final position, goes last).
        placeholders.sort_by_key(|p| if p.number == 0 { usize::MAX } else { p.number });

        (result, placeholders)
    }

    /// Register a snippet.
    fn add(&mut self, language: Option<&str>, trigger: &str, body: &str, desc: &str) {
        let key = language.map(String::from);
        self.snippets.entry(key).or_default().push(Snippet {
            trigger: trigger.to_string(),
            body: body.to_string(),
            description: desc.to_string(),
        });
    }

    /// Load built-in snippets.
    fn load_builtins(&mut self) {
        // Rust
        self.add(
            Some("rust"),
            "fn",
            "fn ${1:name}(${2}) ${3:-> ()} {\n    $0\n}",
            "Function",
        );
        self.add(
            Some("rust"),
            "pfn",
            "pub fn ${1:name}(${2}) ${3:-> ()} {\n    $0\n}",
            "Public function",
        );
        self.add(
            Some("rust"),
            "test",
            "#[test]\nfn ${1:test_name}() {\n    $0\n}",
            "Test function",
        );
        self.add(
            Some("rust"),
            "impl",
            "impl ${1:Type} {\n    $0\n}",
            "Impl block",
        );
        self.add(
            Some("rust"),
            "struct",
            "pub struct ${1:Name} {\n    $0\n}",
            "Struct",
        );
        self.add(
            Some("rust"),
            "enum",
            "pub enum ${1:Name} {\n    $0\n}",
            "Enum",
        );
        self.add(
            Some("rust"),
            "match",
            "match ${1:expr} {\n    ${2:pattern} => $0,\n}",
            "Match",
        );
        self.add(Some("rust"), "if", "if ${1:condition} {\n    $0\n}", "If");
        self.add(
            Some("rust"),
            "for",
            "for ${1:item} in ${2:iter} {\n    $0\n}",
            "For loop",
        );
        self.add(Some("rust"), "mod", "mod ${1:name} {\n    $0\n}", "Module");

        // Python
        self.add(
            Some("python"),
            "def",
            "def ${1:name}(${2:self}):\n    $0",
            "Function",
        );
        self.add(
            Some("python"),
            "class",
            "class ${1:Name}:\n    def __init__(self${2}):\n        $0",
            "Class",
        );
        self.add(Some("python"), "if", "if ${1:condition}:\n    $0", "If");
        self.add(
            Some("python"),
            "for",
            "for ${1:item} in ${2:iterable}:\n    $0",
            "For loop",
        );
        self.add(
            Some("python"),
            "with",
            "with ${1:expr} as ${2:var}:\n    $0",
            "With",
        );
        self.add(
            Some("python"),
            "try",
            "try:\n    $0\nexcept ${1:Exception} as ${2:e}:\n    pass",
            "Try/except",
        );

        // TypeScript / JavaScript
        self.add(
            Some("typescript"),
            "fn",
            "function ${1:name}(${2}) {\n    $0\n}",
            "Function",
        );
        self.add(
            Some("typescript"),
            "afn",
            "const ${1:name} = (${2}) => {\n    $0\n}",
            "Arrow function",
        );
        self.add(
            Some("typescript"),
            "class",
            "class ${1:Name} {\n    constructor(${2}) {\n        $0\n    }\n}",
            "Class",
        );
        self.add(
            Some("typescript"),
            "if",
            "if (${1:condition}) {\n    $0\n}",
            "If",
        );
        self.add(
            Some("typescript"),
            "for",
            "for (const ${1:item} of ${2:array}) {\n    $0\n}",
            "For-of loop",
        );
        self.add(
            Some("typescript"),
            "import",
            "import { $0 } from '${1:module}';",
            "Import",
        );
        self.add(
            Some("typescript"),
            "export",
            "export ${1:default }$0",
            "Export",
        );
        self.add(
            Some("typescript"),
            "const",
            "const ${1:name} = $0;",
            "Const",
        );

        // Go
        self.add(
            Some("go"),
            "func",
            "func ${1:name}(${2}) ${3:error} {\n    $0\n}",
            "Function",
        );
        self.add(Some("go"), "if", "if ${1:condition} {\n    $0\n}", "If");
        self.add(Some("go"), "iferr", "if err != nil {\n    $0\n}", "If err");
        self.add(
            Some("go"),
            "for",
            "for ${1:i := 0; i < n; i++} {\n    $0\n}",
            "For loop",
        );
        self.add(
            Some("go"),
            "struct",
            "type ${1:Name} struct {\n    $0\n}",
            "Struct",
        );
        self.add(
            Some("go"),
            "switch",
            "switch ${1:expr} {\ncase ${2:val}:\n    $0\n}",
            "Switch",
        );

        // Generic (all languages)
        self.add(None, "todo", "// TODO: $0", "TODO comment");
        self.add(None, "fixme", "// FIXME: $0", "FIXME comment");
    }

    /// Load user-defined snippets from `~/.aura/snippets/*.json`.
    fn load_user_snippets(&mut self) {
        let dir = match std::env::var("HOME").ok().map(PathBuf::from) {
            Some(h) => h.join(".aura").join("snippets"),
            None => return,
        };
        if !dir.is_dir() {
            return;
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let lang = path.file_stem().and_then(|s| s.to_str()).map(String::from);
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(map) =
                    serde_json::from_str::<HashMap<String, serde_json::Value>>(&content)
                {
                    for val in map.values() {
                        let prefix = val
                            .get("prefix")
                            .and_then(|p| p.as_str())
                            .unwrap_or_default();
                        let body = val.get("body").and_then(|b| b.as_str()).unwrap_or_default();
                        let desc = val
                            .get("description")
                            .and_then(|d| d.as_str())
                            .unwrap_or("");
                        if !prefix.is_empty() && !body.is_empty() {
                            self.add(lang.as_deref(), prefix, body, desc);
                        }
                    }
                }
            }
        }
    }
}

impl Default for SnippetEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_simple() {
        let (text, placeholders) = SnippetEngine::expand("fn ${1:name}() {\n    $0\n}", "");
        assert!(text.contains("fn name()"));
        assert_eq!(placeholders.len(), 2);
        assert_eq!(placeholders[0].number, 1);
        assert_eq!(placeholders[0].default, "name");
        assert_eq!(placeholders[1].number, 0); // $0 = final
    }

    #[test]
    fn test_expand_no_default() {
        let (text, placeholders) = SnippetEngine::expand("hello $1 world", "");
        assert_eq!(text, "hello  world");
        assert_eq!(placeholders.len(), 1);
        assert_eq!(placeholders[0].number, 1);
        assert_eq!(placeholders[0].default, "");
    }

    #[test]
    fn test_find_snippet() {
        let engine = SnippetEngine::new();
        assert!(engine.find("fn", Some("rust")).is_some());
        assert!(engine.find("def", Some("python")).is_some());
        assert!(engine.find("todo", None).is_some());
        assert!(engine.find("nonexistent", Some("rust")).is_none());
    }

    #[test]
    fn test_active_snippet_navigation() {
        let active = ActiveSnippet {
            placeholders: vec![
                Placeholder {
                    offset: 3,
                    length: 4,
                    default: "name".into(),
                    number: 1,
                },
                Placeholder {
                    offset: 10,
                    length: 0,
                    default: "".into(),
                    number: 0,
                },
            ],
            current: 0,
            insert_offset: 0,
        };
        assert_eq!(active.current_placeholder().unwrap().number, 1);
        assert!(!active.is_done());
    }
}
