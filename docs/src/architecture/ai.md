# AI Crate (`aura-ai`)

The AI crate handles communication with the Anthropic API and assembles editor context for AI requests.

## Modules

| Module | Purpose |
|--------|---------|
| `client` | Streaming Anthropic API client |
| `context` | Editor context assembly and token management |

## Configuration

`AiConfig` holds the AI connection settings:

| Field | Default | Description |
|-------|---------|-------------|
| `api_key` | (from `ANTHROPIC_API_KEY`) | Anthropic API key |
| `base_url` | `https://api.anthropic.com` | API base URL |
| `model` | `claude-sonnet-4-20250514` | Model identifier |
| `max_tokens` | `4096` | Maximum response tokens |
| `max_context_tokens` | `100,000` | Context window budget |

## Anthropic Client

`AnthropicClient` implements streaming communication with Claude:

- **Streaming responses**: Uses Server-Sent Events (SSE) for real-time token delivery
- **Retry logic**: Exponential backoff on transient failures
- **Rate limiting**: Respects API rate limits
- **Token counting**: Estimates token usage before sending

The client emits `AiEvent`s:

- `AiEvent::Token(String)` — a new token from the stream
- `AiEvent::Done` — stream complete
- `AiEvent::Error(String)` — error occurred

## Context Assembly

`EditorContext` assembles the information sent with each AI request:

- **Buffer content**: Current file text (truncated to fit context window)
- **Cursor position**: Row and column
- **Selection**: Visual mode selection range (if any)
- **File metadata**: Path, language, project structure
- **Syntax context**: Tree-sitter node at cursor (type, parent, scope)
- **Edit history**: Recent changes with authorship
- **Diagnostics**: LSP errors/warnings near the cursor
- **Semantic info**: Function call graph, test coverage

### Truncation Strategy

When the context exceeds `max_context_tokens`, the assembler prioritizes:

1. Code near the cursor (highest priority)
2. Diagnostics and syntax context
3. Recent edit history
4. Distant code (summarized rather than included verbatim)

## Token Estimation

`estimate_tokens(text)` provides a fast approximation of token count without calling the API. Used to budget context assembly.

## API Reference

See the [rustdoc for `aura-ai`](/api/aura_ai/).
