# Embedding backends

MemPalace stores vectors in SQLite and uses them for semantic and hybrid retrieval. At runtime you can choose between local embeddings and OpenAI embeddings.

## Backends

`mempalace-rust` supports three user-facing backend modes:

- `auto` — prefer OpenAI when an API key is configured, otherwise use a local backend
- `local` — force local embeddings
- `openai` — force OpenAI embeddings

The configured backend affects indexing and search because vectors are generated when drawers are added or refreshed.

## Local providers

When the backend is `local`, MemPalace chooses a local provider:

- `auto` — use ONNX if the binary was built with ONNX support and the model can be loaded, otherwise use the built-in local vectorizer
- `builtin` — force the built-in embedding implementation
- `onnx` — require the ONNX path if available; if ONNX cannot be used, the current implementation falls back to the built-in local vectorizer unless `openai` was explicitly required

The built-in provider is always available. It is an in-process vectorizer based on normalization, stemming, weighted n-grams, and feature hashing.

## CLI, environment, and config precedence

Embedding settings are resolved in this order:

1. CLI flags
2. Environment variables
3. `~/.mempalace/config.json`
4. Built-in defaults

Relevant CLI flags:

```bash
--embedding-backend auto|local|openai
--local-embedding-provider auto|builtin|onnx
--openai-embedding-model <model>
--openai-api-key <key>
--openai-base-url <url>
```

Relevant environment variables:

```bash
MEMPALACE_EMBEDDING_BACKEND
MEMPALACE_LOCAL_EMBEDDING_PROVIDER
OPENAI_API_KEY
MEMPALACE_OPENAI_API_KEY
MEMPALACE_OPENAI_EMBEDDING_MODEL
MEMPALACE_OPENAI_BASE_URL
MEMPALACE_EMBEDDING_MODEL_DIR
```

Relevant global config fields in `~/.mempalace/config.json`:

```json
{
  "embedding_backend": "auto",
  "local_embedding_provider": "auto",
  "openai_embedding_model": "text-embedding-3-small",
  "openai_base_url": "https://api.openai.com/v1"
}
```

Defaults:

- `embedding_backend`: `auto`
- `local_embedding_provider`: `auto`
- `openai_embedding_model`: `text-embedding-3-small`
- `openai_base_url`: `https://api.openai.com/v1`

## Backend selection behavior

### `auto`

`auto` tries OpenAI first when an API key is present. If OpenAI is not configured or cannot be used, MemPalace tries the local provider path. If no ONNX provider is available, it uses the built-in local vectorizer.

This is the default mode.

### `local`

`local` skips OpenAI and uses the configured local provider selection.

Examples:

```bash
cargo run -- search "GraphQL schema decisions" \
  --embedding-backend local \
  --local-embedding-provider builtin
```

```bash
cargo run -- search "GraphQL schema decisions" \
  --embedding-backend local \
  --local-embedding-provider onnx
```

### `openai`

`openai` requires a configured API key. If the key is missing, startup validation fails. If requests fail, the command fails instead of silently switching backends.

Example:

```bash
export OPENAI_API_KEY=sk-...
cargo run -- search "GraphQL schema decisions" --embedding-backend openai
```

## ONNX embeddings

The ONNX backend is optional at build time.

Build with:

```bash
cargo build --features onnx-embeddings
```

When enabled, MemPalace uses `fastembed` with the `AllMiniLML6V2` text embedding model. Vectors are normalized and resized to the internal embedding dimension before storage.

Optional model cache directory:

```bash
export MEMPALACE_EMBEDDING_MODEL_DIR=/path/to/cache
```

If the binary was not built with `onnx-embeddings`, the ONNX provider is unavailable and `auto` falls back to the built-in provider.

## OpenAI usage

OpenAI embeddings are fetched from:

```text
<base-url>/embeddings
```

The default base URL is:

```text
https://api.openai.com/v1
```

Example using the default OpenAI endpoint:

```bash
export OPENAI_API_KEY=sk-...
cargo run -- mine . --mode projects --embedding-backend openai
cargo run -- search "typed GraphQL queries" --embedding-backend openai
```

Example with a custom compatible endpoint:

```bash
export OPENAI_API_KEY=token
export MEMPALACE_OPENAI_BASE_URL=https://example.com/v1
cargo run -- search "deployment incident" --embedding-backend openai
```

You can also pass settings on the command line:

```bash
cargo run -- search "deployment incident" \
  --embedding-backend openai \
  --openai-api-key "$OPENAI_API_KEY" \
  --openai-embedding-model text-embedding-3-small \
  --openai-base-url https://api.openai.com/v1
```

## Notes about stored vectors

Vectors are written when drawers are added or refreshed. Changing embedding settings does not automatically re-embed existing drawers. To repopulate vectors with a different backend, re-mine or otherwise refresh the relevant sources.

## Search result backend labels

Search results report which semantic backend was used for the query vector:

- `openai`
- `local_onnx`
- `local_builtin`
- `lexical_fallback`

These labels appear in CLI search output and in stored search hit metadata.