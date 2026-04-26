// CodeCell tracks `start_line` / `end_line` for future "jump to cell"
// navigation that hasn't shipped yet. The fields ride along on every
// parsed cell so we don't have to recompute them later.
#![allow(dead_code)]

//! Notebook/REPL mode — Jupyter-like code cells with inline execution.
//!
//! Supports `# %%` cell markers (Python/VS Code style) and fenced
//! code blocks in markdown. Cells are executed via the appropriate
//! runtime (python, node, cargo, etc.).

/// A code cell parsed from the buffer.
#[derive(Debug, Clone)]
pub struct CodeCell {
    /// Language/runtime to use for execution.
    pub language: String,
    /// The cell source code.
    pub code: String,
    /// Start line in the buffer (0-indexed).
    pub start_line: usize,
    /// End line in the buffer (0-indexed, exclusive).
    pub end_line: usize,
}

/// Find the code cell containing the cursor line.
///
/// Recognizes:
/// - `# %%` markers (Python cells, used by VS Code)
/// - `// %%` markers (JS/TS cells)
/// - Fenced code blocks in markdown (` ```language ... ``` `)
pub fn find_cell_at_cursor(content: &str, cursor_line: usize) -> Option<CodeCell> {
    let lines: Vec<&str> = content.lines().collect();

    // Try `# %%` or `// %%` style markers first.
    let mut cell_start = None;
    let mut cell_lang = "python".to_string();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("# %%") || trimmed.starts_with("// %%") {
            if i <= cursor_line {
                cell_start = Some(i);
                // Check for language hint: # %% [python]
                if let Some(lang) = trimmed.split('[').nth(1).and_then(|s| s.strip_suffix(']')) {
                    cell_lang = lang.to_string();
                } else if trimmed.starts_with("// %%") {
                    cell_lang = "javascript".to_string();
                }
            } else if cell_start.is_some() {
                // Found the end of the cell.
                let start = cell_start.unwrap() + 1;
                let code: String = lines[start..i].join("\n");
                return Some(CodeCell {
                    language: cell_lang,
                    code,
                    start_line: start,
                    end_line: i,
                });
            }
        }
    }

    // If we found a start but no end, the cell goes to the end of the file.
    if let Some(start_idx) = cell_start {
        let start = start_idx + 1;
        let code: String = lines[start..].join("\n");
        return Some(CodeCell {
            language: cell_lang,
            code,
            start_line: start,
            end_line: lines.len(),
        });
    }

    // Try fenced code blocks (markdown).
    let mut in_block = false;
    let mut block_start = 0;
    let mut block_lang = String::new();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") && !in_block {
            in_block = true;
            block_start = i + 1;
            block_lang = trimmed[3..].trim().to_string();
            if block_lang.is_empty() {
                block_lang = "python".to_string();
            }
        } else if trimmed == "```" && in_block {
            if cursor_line >= block_start.saturating_sub(1) && cursor_line <= i {
                let code: String = lines[block_start..i].join("\n");
                return Some(CodeCell {
                    language: block_lang,
                    code,
                    start_line: block_start,
                    end_line: i,
                });
            }
            in_block = false;
        }
    }

    None
}

/// Find all code cells in the content.
pub fn find_all_cells(content: &str) -> Vec<CodeCell> {
    let lines: Vec<&str> = content.lines().collect();
    let mut cells = Vec::new();
    let mut cell_start = None;
    let mut cell_lang = "python".to_string();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("# %%") || trimmed.starts_with("// %%") {
            // Close previous cell if any.
            if let Some(start_idx) = cell_start {
                let start = start_idx + 1;
                let code: String = lines[start..i].join("\n");
                if !code.trim().is_empty() {
                    cells.push(CodeCell {
                        language: cell_lang.clone(),
                        code,
                        start_line: start,
                        end_line: i,
                    });
                }
            }
            cell_start = Some(i);
            if let Some(lang) = trimmed.split('[').nth(1).and_then(|s| s.strip_suffix(']')) {
                cell_lang = lang.to_string();
            } else if trimmed.starts_with("// %%") {
                cell_lang = "javascript".to_string();
            } else {
                cell_lang = "python".to_string();
            }
        }
    }

    // Close the last cell.
    if let Some(start_idx) = cell_start {
        let start = start_idx + 1;
        let code: String = lines[start..].join("\n");
        if !code.trim().is_empty() {
            cells.push(CodeCell {
                language: cell_lang,
                code,
                start_line: start,
                end_line: lines.len(),
            });
        }
    }

    cells
}

/// Execute a code cell and return the output.
pub fn execute_cell(cell: &CodeCell) -> anyhow::Result<String> {
    let (cmd, args) = match cell.language.as_str() {
        "python" | "py" | "python3" => ("python3", vec!["-c", &cell.code]),
        "javascript" | "js" | "node" => ("node", vec!["-e", &cell.code]),
        "typescript" | "ts" => ("npx", vec!["tsx", "-e", &cell.code]),
        "ruby" | "rb" => ("ruby", vec!["-e", &cell.code]),
        "bash" | "sh" | "shell" => ("bash", vec!["-c", &cell.code]),
        "zsh" => ("zsh", vec!["-c", &cell.code]),
        lang => anyhow::bail!("Unsupported language: {lang}"),
    };

    let output = std::process::Command::new(cmd)
        .args(&args)
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run {cmd}: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !stderr.is_empty() {
        Ok(format!("{}{}", stdout, stderr))
    } else {
        Ok(stdout.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_cell_python() {
        let content = "# some header\n# %%\nx = 1\nprint(x)\n# %%\ny = 2\n";
        // Cursor on line 2 (x = 1) should find the first cell.
        let cell = find_cell_at_cursor(content, 2).unwrap();
        assert_eq!(cell.language, "python");
        assert!(cell.code.contains("x = 1"));
        assert_eq!(cell.start_line, 2);
        assert_eq!(cell.end_line, 4);
    }

    #[test]
    fn test_find_cell_fenced() {
        let content = "Some text\n```python\nprint('hello')\n```\nMore text\n";
        // Cursor on line 2 (inside the fenced block).
        let cell = find_cell_at_cursor(content, 2).unwrap();
        assert_eq!(cell.language, "python");
        assert!(cell.code.contains("print('hello')"));
    }

    #[test]
    fn test_find_all_cells() {
        let content = "# %% [python]\nx = 1\n# %% [rust]\nlet y = 2;\n# %%\nz = 3\n";
        let cells = find_all_cells(content);
        assert_eq!(cells.len(), 3);
        assert_eq!(cells[0].language, "python");
        assert_eq!(cells[1].language, "rust");
        assert_eq!(cells[2].language, "python"); // default
    }
}
