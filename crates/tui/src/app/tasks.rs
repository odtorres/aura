//! Task runner: resolve configured + auto-detected tasks and dispatch them
//! into the embedded terminal.
//!
//! Extracted from `app/mod.rs` so the monolith file doesn't keep growing.
//! Auto-detection inspects the project root (Cargo.toml, package.json,
//! Makefile, go.mod, mix.exs, pubspec.yaml, build.zig, build.sbt,
//! stack.yaml/cabal.project, requirements.txt/pyproject.toml) and returns
//! a default task set — used only when `[tasks]` is absent from `aura.toml`.

use super::App;

impl App {
    /// Run a named task in the embedded terminal.
    pub fn run_task(&mut self, name: &str) {
        let tasks = self.get_tasks();
        let task = match tasks.get(name) {
            Some(t) => t.clone(),
            None => {
                let available: Vec<&str> = tasks.keys().map(|s| s.as_str()).collect();
                if available.is_empty() {
                    self.set_status("No tasks configured. Add [tasks] to aura.toml");
                } else {
                    self.set_status(format!(
                        "Unknown task '{}'. Available: {}",
                        name,
                        available.join(", ")
                    ));
                }
                return;
            }
        };

        // Show terminal and send the command.
        self.terminal_mut().visible = true;
        self.terminal_focused = true;
        self.terminal_mut().send_bytes(task.command.as_bytes());
        self.terminal_mut().send_enter();
        self.set_status(format!("Running task: {} ({})", name, task.command));
    }

    /// Get all available tasks (configured + auto-detected).
    pub fn get_tasks(&self) -> std::collections::HashMap<String, crate::config::TaskConfig> {
        if !self.config.tasks.is_empty() {
            return self.config.tasks.clone();
        }
        // Auto-detect from project type.
        self.auto_detect_tasks()
    }

    /// Auto-detect tasks based on project files.
    fn auto_detect_tasks(&self) -> std::collections::HashMap<String, crate::config::TaskConfig> {
        use crate::config::TaskConfig;
        let mut tasks = std::collections::HashMap::new();
        let root = self
            .tab()
            .buffer
            .file_path()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| std::path::PathBuf::from("."));

        if root.join("Cargo.toml").exists() {
            tasks.insert(
                "build".into(),
                TaskConfig {
                    command: "cargo build".into(),
                    description: "Build the project".into(),
                },
            );
            tasks.insert(
                "test".into(),
                TaskConfig {
                    command: "cargo test".into(),
                    description: "Run tests".into(),
                },
            );
            tasks.insert(
                "clippy".into(),
                TaskConfig {
                    command: "cargo clippy -- -D warnings".into(),
                    description: "Run lints".into(),
                },
            );
            tasks.insert(
                "fmt".into(),
                TaskConfig {
                    command: "cargo fmt --all".into(),
                    description: "Format code".into(),
                },
            );
        } else if root.join("package.json").exists() {
            // Parse actual scripts from package.json.
            if let Ok(content) = std::fs::read_to_string(root.join("package.json")) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(scripts) = json.get("scripts").and_then(|s| s.as_object()) {
                        for (name, cmd) in scripts {
                            if let Some(cmd_str) = cmd.as_str() {
                                tasks.insert(
                                    name.clone(),
                                    TaskConfig {
                                        command: format!("npm run {name}"),
                                        description: cmd_str.chars().take(60).collect::<String>(),
                                    },
                                );
                            }
                        }
                    }
                }
            }
            // Fallback if no scripts parsed.
            if tasks.is_empty() {
                tasks.insert(
                    "build".into(),
                    TaskConfig {
                        command: "npm run build".into(),
                        description: "Build the project".into(),
                    },
                );
                tasks.insert(
                    "test".into(),
                    TaskConfig {
                        command: "npm test".into(),
                        description: "Run tests".into(),
                    },
                );
            }
        } else if root.join("Makefile").exists() || root.join("makefile").exists() {
            // Parse actual Makefile targets.
            let makefile_path = if root.join("Makefile").exists() {
                root.join("Makefile")
            } else {
                root.join("makefile")
            };
            if let Ok(content) = std::fs::read_to_string(&makefile_path) {
                for line in content.lines() {
                    // Match lines like `target_name:` (not starting with tab/space, not `.PHONY`).
                    if let Some(colon_pos) = line.find(':') {
                        let target = line[..colon_pos].trim();
                        if !target.is_empty()
                            && !target.starts_with('.')
                            && !target.starts_with('#')
                            && !target.starts_with('\t')
                            && !target.contains(' ')
                            && !target.contains('$')
                        {
                            tasks.insert(
                                target.to_string(),
                                TaskConfig {
                                    command: format!("make {target}"),
                                    description: format!("make {target}"),
                                },
                            );
                        }
                    }
                }
            }
            // Fallback if nothing parsed.
            if tasks.is_empty() {
                tasks.insert(
                    "build".into(),
                    TaskConfig {
                        command: "make".into(),
                        description: "Build (default target)".into(),
                    },
                );
            }
        } else if root.join("go.mod").exists() {
            tasks.insert(
                "build".into(),
                TaskConfig {
                    command: "go build ./...".into(),
                    description: "Build the project".into(),
                },
            );
            tasks.insert(
                "test".into(),
                TaskConfig {
                    command: "go test ./...".into(),
                    description: "Run tests".into(),
                },
            );
            tasks.insert(
                "fmt".into(),
                TaskConfig {
                    command: "gofmt -w .".into(),
                    description: "Format code".into(),
                },
            );
        } else if root.join("mix.exs").exists() {
            tasks.insert(
                "build".into(),
                TaskConfig {
                    command: "mix compile".into(),
                    description: "Compile the project".into(),
                },
            );
            tasks.insert(
                "test".into(),
                TaskConfig {
                    command: "mix test".into(),
                    description: "Run tests".into(),
                },
            );
            tasks.insert(
                "fmt".into(),
                TaskConfig {
                    command: "mix format".into(),
                    description: "Format code".into(),
                },
            );
            tasks.insert(
                "deps".into(),
                TaskConfig {
                    command: "mix deps.get".into(),
                    description: "Fetch dependencies".into(),
                },
            );
            tasks.insert(
                "server".into(),
                TaskConfig {
                    command: "mix phx.server".into(),
                    description: "Start Phoenix server".into(),
                },
            );
        } else if root.join("pubspec.yaml").exists() {
            // Dart / Flutter
            tasks.insert(
                "build".into(),
                TaskConfig {
                    command: "dart compile exe lib/main.dart".into(),
                    description: "Compile Dart".into(),
                },
            );
            tasks.insert(
                "test".into(),
                TaskConfig {
                    command: "dart test".into(),
                    description: "Run tests".into(),
                },
            );
            tasks.insert(
                "fmt".into(),
                TaskConfig {
                    command: "dart format .".into(),
                    description: "Format code".into(),
                },
            );
            tasks.insert(
                "deps".into(),
                TaskConfig {
                    command: "dart pub get".into(),
                    description: "Fetch dependencies".into(),
                },
            );
            if root.join("lib").join("main.dart").exists() {
                tasks.insert(
                    "run".into(),
                    TaskConfig {
                        command: "flutter run".into(),
                        description: "Run Flutter app".into(),
                    },
                );
            }
        } else if root.join("build.zig").exists() {
            // Zig
            tasks.insert(
                "build".into(),
                TaskConfig {
                    command: "zig build".into(),
                    description: "Build the project".into(),
                },
            );
            tasks.insert(
                "test".into(),
                TaskConfig {
                    command: "zig build test".into(),
                    description: "Run tests".into(),
                },
            );
        } else if root.join("build.sbt").exists() {
            // Scala / sbt
            tasks.insert(
                "build".into(),
                TaskConfig {
                    command: "sbt compile".into(),
                    description: "Compile the project".into(),
                },
            );
            tasks.insert(
                "test".into(),
                TaskConfig {
                    command: "sbt test".into(),
                    description: "Run tests".into(),
                },
            );
            tasks.insert(
                "run".into(),
                TaskConfig {
                    command: "sbt run".into(),
                    description: "Run the project".into(),
                },
            );
        } else if root.join("stack.yaml").exists() || root.join("cabal.project").exists() {
            // Haskell
            let tool = if root.join("stack.yaml").exists() {
                "stack"
            } else {
                "cabal"
            };
            tasks.insert(
                "build".into(),
                TaskConfig {
                    command: format!("{tool} build"),
                    description: "Build the project".into(),
                },
            );
            tasks.insert(
                "test".into(),
                TaskConfig {
                    command: format!("{tool} test"),
                    description: "Run tests".into(),
                },
            );
            tasks.insert(
                "run".into(),
                TaskConfig {
                    command: format!("{tool} run"),
                    description: "Run the project".into(),
                },
            );
        } else if root.join("requirements.txt").exists() || root.join("pyproject.toml").exists() {
            tasks.insert(
                "test".into(),
                TaskConfig {
                    command: "pytest".into(),
                    description: "Run tests".into(),
                },
            );
            tasks.insert(
                "lint".into(),
                TaskConfig {
                    command: "ruff check .".into(),
                    description: "Run lints".into(),
                },
            );
            tasks.insert(
                "fmt".into(),
                TaskConfig {
                    command: "black .".into(),
                    description: "Format code".into(),
                },
            );
        }

        tasks
    }
}
