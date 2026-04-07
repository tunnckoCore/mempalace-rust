# mempalace-rust

_This is Rust port/re-implementation (not a direct fork) of the [milla-jovovich/mempalace](https://github.com/milla-jovovich/mempalace) project, which might have been inspired by [SaraBrain](https://github.com/LunarFawn/SaraBrain). The port was largely done using [Pi Coding Agent](https://github.com/badlogic/pi-mono) and OpenAI GPT-5.4 - it's a work in progress and may not be as good as the original._

MemPalace is a local memory system for projects, conversations, and agent workflows. This Rust implementation stores everything in SQLite, supports hybrid retrieval, and exposes both a CLI and an MCP server.

## What it does

MemPalace ingests source material into a global, structured "palace" of memories:

- **wings** separate projects, agents, or source domains
- **rooms** organize memories by topic
- **drawers** store raw chunks and derived artifacts
- **vectors** support semantic retrieval
- **FTS** supports lexical retrieval

The result is a searchable local memory store that can be mined from codebases and conversations, queried from the CLI, and connected to MCP clients.

## Core architecture

### Storage

The palace lives in SQLite at:

```text
<palace-path>/mempalace.sqlite3
```

Key persisted data:

- raw drawers and derived artifacts
- source revision tracking for incremental refresh
- stored vectors for semantic search
- SQLite FTS5 index for lexical search

### Retrieval

Search is hybrid by default:

- lexical retrieval via SQLite FTS5
- semantic retrieval via stored vectors
- heuristic reranking and fused scoring

### Memory layers

Wake-up uses a layered memory model:

- **L0** — identity text from `~/.mempalace/identity.txt`
- **L1** — essential story built from recent important drawers
- **L2** — scoped recall
- **L3** — deep search

### Derived memory features

The project also includes:

- AAAK compression and compact artifacts
- general extraction from conversations
- a temporal knowledge graph
- a room graph for traversal and tunnel finding
- an MCP server for external clients

## Build and install

### Requirements

- Rust toolchain
- SQLite is bundled through `rusqlite`'s bundled feature

### Build

```bash
cargo build
```

Run the CLI:

```bash
cargo run -- --help
```

### Optional ONNX embeddings

To enable the ONNX local embedding backend:

```bash
cargo build --features onnx-embeddings
```

## Quick start

Initialize MemPalace for a project:

```bash
cargo run -- init .
```

Mine the current project:

```bash
cargo run -- mine . --mode projects
```

Search the palace:

```bash
cargo run -- search "how openai embedding backend works"
```

Show palace status:

```bash
cargo run -- status
```

Render the wake-up summary:

```bash
cargo run -- wake-up
```

Start the MCP server:

```bash
cargo run -- mcp --transport stdio
```

## Configuration model

MemPalace uses both global and per-project config.

It stores all memories into a "global store". If you run it from a project that has "local config" it would know how to narrow the search to be scoped only for that project.

### Global config

Global state lives under:

```
~/.mempalace
```

Important files:

- `~/.mempalace/config.json`
- `~/.mempalace/identity.txt`
- `~/.mempalace/palace/`

### Per-project config

Projects can define `mempalace.yaml`:

```yaml
wing: my-project
rooms:
  - name: general
    keywords: []
  - name: src
    keywords: []
```

The `init` command creates this file automatically if it does not already exist.

## Key commands

### `init`

Create global config if needed and create a project `mempalace.yaml`.

```bash
cargo run -- init /path/to/project
```

### `mine` (import)

Mine/import a project or a conversation directory into the palace.

```bash
cargo run -- mine /path/to/project --mode projects
cargo run -- mine /path/to/chats --mode convos --extract exchange
cargo run -- mine /path/to/chats --mode convos --extract general
```

Useful flags:

- `--wing <name>`
- `--limit <n>`
- `--dry-run`
- `--agent <name>`

### `search`

Hybrid search with optional wing and room filtering.

```bash
cargo run -- search "typed GraphQL queries"
cargo run -- search "typed GraphQL queries" --wing my-app --room architecture --results 10
```

### `status`

Show total drawer counts grouped by wing and room.

```bash
cargo run -- status
```

### `wake-up`

Render the L0 + L1 summary, optionally scoped to a wing.

```bash
cargo run -- wake-up
cargo run -- wake-up --wing my-app
```

### `compress`

Generate and store AAAK compressed artifacts.

```bash
cargo run -- compress
cargo run -- compress --wing my-app
```

### `mcp`

Run the stdio MCP server.

```bash
cargo run -- mcp --transport stdio
```

### `benchmark`

Evaluate retrieval quality on a benchmark dataset.

```bash
cargo run -- benchmark ./bench.json --backend hybrid --k 5
```

## Embedding choices

MemPalace supports three backend modes:

- `auto`
- `local`
- `openai`

Local provider choices:

- `auto`
- `builtin`
- `onnx`

Examples:

Use the default automatic selection:

```bash
cargo run -- search "deployment incident"
```

Force the built-in local provider:

```bash
cargo run -- search "deployment incident" \
  --embedding-backend local \
  --local-embedding-provider builtin
```

### Use ONNX embeddings:

```bash
cargo run --features onnx-embeddings -- search "deployment incident" \
  --embedding-backend local \
  --local-embedding-provider onnx
```

### Use OpenAI embeddings:

```bash
export OPENAI_API_KEY=sk-...
cargo run -- search "deployment incident" --embedding-backend openai
```

Embedding configuration precedence is:

1. CLI flags
2. environment variables
3. `~/.mempalace/config.json`
4. defaults

## MCP usage notes

The MCP server:

- supports `stdio` transport
- accepts `Content-Length` framed input and newline-delimited JSON input
- writes startup logging to stderr
- gates mutating tools behind `MEMPALACE_ENABLE_MUTATIONS=1`

## Project layout

Important source files:

- `src/storage.rs` — SQLite storage, vectors, refresh logic, hybrid search
- `src/embedding.rs` — backend selection and embedding providers
- `src/project.rs` — project init and mining
- `src/convo.rs` — conversation mining
- `src/layers.rs` — layered memory stack
- `src/dialect.rs` — AAAK compression dialect
- `src/kg.rs` — temporal knowledge graph
- `src/graph.rs` — room graph traversal and tunnel logic
- `src/mcp.rs` — MCP server
- `src/bench.rs` — benchmark runner

## More documentation

See the focused docs in `docs/`:

- [`docs/embeddings.md`](docs/embeddings.md)
- [`docs/init-config-storage.md`](docs/init-config-storage.md)
- [`docs/mcp.md`](docs/mcp.md)
- [`docs/benchmarking.md`](docs/benchmarking.md)

## License

GPL-3.0-or-later, 2026.

It's not a direct fork of [milla-jovovich/mempalace](https://github.com/milla-jovovich/mempalace), but spiritual re-implementation in Rust.
