# Notebook / REPL Mode

Execute code cells inline, Jupyter-style. Write code with cell markers, run cells, and see output in the terminal.

## Cell Markers

### Python / VS Code style

```python
# %% [python]
print("Hello from cell 1")
x = 42

# %%
print(f"x = {x}")
```

### JavaScript

```javascript
// %% [javascript]
console.log("Hello from JS");

// %%
const result = 2 + 2;
console.log(result);
```

### Markdown fenced blocks

In `.md` files, fenced code blocks are treated as cells:

````markdown
```python
print("Hello")
```
````

## Commands

| Command | Action |
|---------|--------|
| `:cell run` | Run the cell at the cursor |
| `:cell run-all` | Run all cells in the file |

## Supported Languages

| Language | Runtime |
|----------|---------|
| Python | `python3 -c` |
| JavaScript | `node -e` |
| TypeScript | `npx tsx -e` |
| Ruby | `ruby -e` |
| Bash / Shell | `bash -c` |
| Zsh | `zsh -c` |

## Output

Cell output (stdout and stderr) is displayed in the embedded terminal pane, which opens automatically when a cell is executed.

## Language Hints

Specify the language in the cell marker:

```
# %% [python]     → runs with python3
# %% [javascript] → runs with node
// %%              → defaults to javascript
# %%               → defaults to python
```
