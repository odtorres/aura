//! Tool execution for the AI chat panel.
//!
//! Provides the five core tools that the AI can use:
//! - `read_file` — read file contents
//! - `list_files` — list directory entries
//! - `search_files` — grep for a pattern in files
//! - `edit_file` — edit a file (find & replace)
//! - `run_command` — run a shell command

use std::path::{Path, PathBuf};
use std::process::Command;

/// Maximum size of a tool result to avoid blowing up the context window.
const MAX_RESULT_BYTES: usize = 50_000;

/// Maximum number of tool iterations per user message (safety limit).
pub const MAX_TOOL_ITERATIONS: usize = 25;

/// Execute a tool by name and return the result.
///
/// `project_root` is the working directory for relative paths.
pub fn execute_tool(
    name: &str,
    input: &serde_json::Value,
    project_root: &Path,
) -> Result<String, String> {
    match name {
        "read_file" => tool_read_file(input, project_root),
        "list_files" => tool_list_files(input, project_root),
        "search_files" => tool_search_files(input, project_root),
        "edit_file" => tool_edit_file(input, project_root),
        "run_command" => tool_run_command(input, project_root),
        "create_directory" => tool_create_directory(input, project_root),
        "rename_file" => tool_rename_file(input, project_root),
        // Subagent tools are handled directly by app.rs, not here.
        "spawn_subagent" | "check_subagent" | "cancel_subagent" => {
            Err(format!("{name} is handled by the agent system"))
        }
        _ => Err(format!("Unknown tool: {name}")),
    }
}

/// Read a file's contents.
fn tool_read_file(input: &serde_json::Value, root: &Path) -> Result<String, String> {
    let path_str = input
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or("Missing 'path' parameter")?;

    let path = resolve_path(path_str, root);
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;

    Ok(truncate_result(&content))
}

/// List files in a directory.
fn tool_list_files(input: &serde_json::Value, root: &Path) -> Result<String, String> {
    let path_str = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let recursive = input
        .get("recursive")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let dir = resolve_path(path_str, root);
    if !dir.is_dir() {
        return Err(format!("{} is not a directory", dir.display()));
    }

    let mut entries = Vec::new();
    if recursive {
        list_recursive(&dir, root, &mut entries, 0, 500);
    } else {
        let rd = std::fs::read_dir(&dir).map_err(|e| format!("Failed to read directory: {e}"))?;
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_dir {
                entries.push(format!("{name}/"));
            } else {
                entries.push(name);
            }
        }
        entries.sort();
    }

    Ok(entries.join("\n"))
}

/// Recursively list files up to a limit.
fn list_recursive(dir: &Path, root: &Path, out: &mut Vec<String>, depth: usize, limit: usize) {
    if out.len() >= limit || depth > 10 {
        return;
    }
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return,
    };
    let mut children: Vec<_> = rd.flatten().collect();
    children.sort_by_key(|e| e.file_name());

    for entry in children {
        if out.len() >= limit {
            break;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        // Skip hidden files and common ignores.
        if name.starts_with('.') || name == "node_modules" || name == "target" {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(root)
            .unwrap_or(&entry.path())
            .to_string_lossy()
            .to_string();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            out.push(format!("{rel}/"));
            list_recursive(&entry.path(), root, out, depth + 1, limit);
        } else {
            out.push(rel);
        }
    }
}

/// Search files for a pattern using grep.
fn tool_search_files(input: &serde_json::Value, root: &Path) -> Result<String, String> {
    let pattern = input
        .get("pattern")
        .and_then(|v| v.as_str())
        .ok_or("Missing 'pattern' parameter")?;
    let path_str = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let file_pattern = input.get("file_pattern").and_then(|v| v.as_str());

    let dir = resolve_path(path_str, root);

    // Try to use `rg` (ripgrep) if available, fall back to manual search.
    let mut cmd = Command::new("rg");
    cmd.arg("--no-heading")
        .arg("--line-number")
        .arg("--max-count=50")
        .arg("--max-filesize=1M");

    if let Some(glob) = file_pattern {
        cmd.arg("--glob").arg(glob);
    }

    cmd.arg(pattern).arg(&dir);

    match cmd.output() {
        Ok(output) if output.status.success() || output.status.code() == Some(1) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.is_empty() {
                Ok("No matches found.".to_string())
            } else {
                Ok(truncate_result(&stdout))
            }
        }
        _ => {
            // Fallback: simple grep using grep command.
            let mut cmd = Command::new("grep");
            cmd.arg("-rn").arg("--max-count=50").arg(pattern).arg(&dir);
            match cmd.output() {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    if stdout.is_empty() {
                        Ok("No matches found.".to_string())
                    } else {
                        Ok(truncate_result(&stdout))
                    }
                }
                Err(e) => Err(format!("Search failed: {e}")),
            }
        }
    }
}

/// Edit a file by finding and replacing text.
fn tool_edit_file(input: &serde_json::Value, root: &Path) -> Result<String, String> {
    let path_str = input
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or("Missing 'path' parameter")?;
    let old_text = input
        .get("old_text")
        .and_then(|v| v.as_str())
        .ok_or("Missing 'old_text' parameter")?;
    let new_text = input
        .get("new_text")
        .and_then(|v| v.as_str())
        .ok_or("Missing 'new_text' parameter")?;

    let path = resolve_path(path_str, root);

    if old_text.is_empty() {
        // Create new file.
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directories: {e}"))?;
        }
        std::fs::write(&path, new_text).map_err(|e| format!("Failed to write file: {e}"))?;
        Ok(format!("Created {}", path.display()))
    } else {
        // Read, replace, write.
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;

        if !content.contains(old_text) {
            return Err(format!(
                "old_text not found in {}. Make sure the text matches exactly.",
                path.display()
            ));
        }

        let new_content = content.replacen(old_text, new_text, 1);
        std::fs::write(&path, &new_content)
            .map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
        Ok(format!(
            "Edited {} (replaced {} bytes with {} bytes)",
            path.display(),
            old_text.len(),
            new_text.len()
        ))
    }
}

/// Run a shell command.
fn tool_run_command(input: &serde_json::Value, root: &Path) -> Result<String, String> {
    let command = input
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or("Missing 'command' parameter")?;
    let _timeout_secs = input
        .get("timeout_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(30);

    let output = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(root)
        .output()
        .map_err(|e| format!("Failed to execute command: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let mut result = String::new();
    if !stdout.is_empty() {
        result.push_str(&stdout);
    }
    if !stderr.is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str("STDERR:\n");
        result.push_str(&stderr);
    }

    if output.status.success() {
        if result.is_empty() {
            Ok("Command completed successfully (no output).".to_string())
        } else {
            Ok(truncate_result(&result))
        }
    } else {
        let code = output.status.code().unwrap_or(-1);
        if result.is_empty() {
            Err(format!("Command failed with exit code {code}"))
        } else {
            Err(format!(
                "Command failed (exit code {code}):\n{}",
                truncate_result(&result)
            ))
        }
    }
}

/// Create a directory (and parents).
fn tool_create_directory(input: &serde_json::Value, root: &Path) -> Result<String, String> {
    let path_str = input
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or("Missing 'path' parameter")?;

    let path = resolve_path(path_str, root);
    std::fs::create_dir_all(&path)
        .map_err(|e| format!("Failed to create directory {}: {e}", path.display()))?;

    Ok(format!("Created directory: {}", path.display()))
}

/// Rename or move a file or directory.
fn tool_rename_file(input: &serde_json::Value, root: &Path) -> Result<String, String> {
    let old_str = input
        .get("old_path")
        .and_then(|v| v.as_str())
        .ok_or("Missing 'old_path' parameter")?;
    let new_str = input
        .get("new_path")
        .and_then(|v| v.as_str())
        .ok_or("Missing 'new_path' parameter")?;

    let old_path = resolve_path(old_str, root);
    let new_path = resolve_path(new_str, root);

    // Ensure parent directory exists.
    if let Some(parent) = new_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create parent directory: {e}"))?;
    }

    std::fs::rename(&old_path, &new_path).map_err(|e| {
        format!(
            "Failed to rename {} -> {}: {e}",
            old_path.display(),
            new_path.display()
        )
    })?;

    Ok(format!(
        "Renamed: {} -> {}",
        old_path.display(),
        new_path.display()
    ))
}

/// Resolve a path relative to the project root.
fn resolve_path(path_str: &str, root: &Path) -> PathBuf {
    let p = Path::new(path_str);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        root.join(p)
    }
}

/// Truncate a result string to avoid blowing up the context.
fn truncate_result(s: &str) -> String {
    if s.len() <= MAX_RESULT_BYTES {
        s.to_string()
    } else {
        let truncated = &s[..MAX_RESULT_BYTES];
        format!("{}\n\n... (truncated, {} total bytes)", truncated, s.len())
    }
}
