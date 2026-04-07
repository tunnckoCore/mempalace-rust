# mempalace-rust

Parity-oriented Rust migration of `./mempalace`, implemented only from the reference Python project and this Rust directory.

## Architecture

This pass moves the Rust version closer to the Python MemPalace architecture rather than a stripped-down CLI.

### Storage core

Primary storage remains SQLite under the palace directory:

- `mempalace.sqlite3`
- active/inactive drawer lifecycle tracking
- source revision tracking for incremental re-mining
- FTS5 lexical index
- persisted vector table for semantic retrieval
- derived artifact support via `drawer_type`

### Retrieval model

Search is now **hybrid** instead of FTS-only:

- lexical retrieval via SQLite FTS5
- semantic retrieval via vectors persisted in SQLite
- reciprocal-rank-style fusion plus heuristic reranking

Embedding backends:

- **default / fallback path**: strong local in-process vectorizer (stemming + weighted n-grams + signed feature hashing)
- **optional real backend**: ONNX/local model embedding via `fastembed` behind the `onnx-embeddings` Cargo feature
- **optional OpenAI backend**: live OpenAI embeddings over HTTPS using `OPENAI_API_KEY`

This keeps SQLite as source of truth while allowing a stronger semantic backend when enabled.

### Incremental mining semantics

Mining no longer skips only by `source_file` existence.

For both project and conversation mining:

- normalized/read content is hashed
- source revision is compared against stored hash
- unchanged sources are skipped
- changed sources are refreshed transactionally
- new drawers/artifacts are inserted before prior revisions for that source are retired
- source revision metadata is updated only after the refresh succeeds

### Memory architecture

Rust now includes a layered memory model inspired by Python:

- **L0** identity from `~/.mempalace/identity.txt`
- **L1** essential story built from top recent drawers grouped by room
- **L2** filtered recall
- **L3** deep hybrid search

`wake-up` now uses the layered stack rather than an ad hoc summary.

### AAAK

A practical Rust AAAK dialect port is included:

- entity/topic/emotion/flag detection
- compression stats
- `compress` CLI command
- compressed artifacts can be stored back into the palace as derived records
- AAAK spec is exposed through MCP/status flows

### Extractors

Conversation mining supports:

- `exchange` mode
- `general` mode with heuristic extraction of:
  - decisions
  - preferences
  - milestones
  - problems
  - emotional memories

General extraction outputs are stored as derived retrievable artifacts.

### Knowledge graph

A Rust SQLite-backed temporal KG is included:

- entities
- triples
- valid_from / valid_to
- invalidate semantics
- entity query
- timeline
- stats

### Palace graph

Metadata-derived room graph support is included:

- graph build
- traversal
- tunnel finding
- graph stats
- basic hall metadata generation during mining for better graph usefulness

### MCP coverage

The stdio MCP server now exposes a much larger Python-aligned tool inventory.

Transport handling now supports:

- standard `Content-Length` framed stdio messages
- newline-delimited JSON as a simpler fallback input mode

Responses are emitted with `Content-Length` framing for better MCP compatibility.

The tool inventory includes:

- `mempalace_status`
- `mempalace_list_wings`
- `mempalace_list_rooms`
- `mempalace_get_taxonomy`
- `mempalace_get_aaak_spec`
- `mempalace_search`
- `mempalace_check_duplicate`
- `mempalace_add_drawer`
- `mempalace_delete_drawer`
- `mempalace_kg_query`
- `mempalace_kg_add`
- `mempalace_kg_invalidate`
- `mempalace_kg_timeline`
- `mempalace_kg_stats`
- `mempalace_traverse`
- `mempalace_find_tunnels`
- `mempalace_graph_stats`
- `mempalace_diary_write`
- `mempalace_diary_read`

## Embedding backend selection

Runtime/backend selection is controlled by:

- CLI: `--embedding-backend auto|strong_local|onnx|openai`, `--openai-embedding-model`, `--openai-api-key`, `--openai-base-url`
- env: `MEMPALACE_EMBEDDING_BACKEND`, `OPENAI_API_KEY`, `MEMPALACE_OPENAI_EMBEDDING_MODEL`, `MEMPALACE_OPENAI_BASE_URL`
- config: `~/.mempalace/config.json` → `embedding_backend`, `openai_embedding_model`, `openai_base_url`

Behavior:

- `auto` (default): try OpenAI first when configured, then ONNX if compiled and available, otherwise fall back cleanly
- `openai`: require OpenAI embeddings; missing key/model or request failures are surfaced clearly
- `onnx`: prefer ONNX backend, but still fall back if model/backend initialization fails at runtime
- `strong_local`: force the built-in local vectorizer

Precedence:
- CLI flags
- environment variables
- config file
- defaults

OpenAI examples:

```bash
export OPENAI_API_KEY=sk-...
cargo run -- search "why did we switch to GraphQL" --embedding-backend openai
cargo run -- benchmark ./bench.json --backend openai --openai-embedding-model text-embedding-3-small
```

Optional ONNX build:

```bash
cargo build --features onnx-embeddings
```

Optional model cache dir:

```bash
export MEMPALACE_EMBEDDING_MODEL_DIR=/path/to/model-cache
```

## CLI

Examples:

```bash
cargo run -- init /path/to/project
cargo run -- mine /path/to/project --mode projects
cargo run -- mine /path/to/chats --mode convos --extract exchange
cargo run -- mine /path/to/chats --mode convos --extract general
cargo run -- search "why did we switch to GraphQL"
cargo run -- status
cargo run -- wake-up
cargo run -- compress --wing myapp
cargo run -- mcp --transport stdio
cargo run -- benchmark ./bench.json --backend hybrid --k 5
```

## Benchmark runner

A benchmark/eval runner is included via the `benchmark` CLI command.

Supported dataset formats:

JSON:
```json
{
  "documents": [
    {"id": "doc1", "wing": "app", "room": "architecture", "content": "...", "source_file": "bench/doc1.md"}
  ],
  "queries": [
    {"id": "q1", "query": "typed GraphQL queries", "relevant_ids": ["doc1"]}
  ]
}
```

JSONL:
```json
{"kind":"document","id":"doc1","wing":"app","room":"architecture","content":"..."}
{"kind":"query","id":"q1","query":"typed GraphQL queries","relevant_ids":["doc1"]}
```

Output metrics:
- Recall@k
- MRR
- NDCG@k

Backends:
- `fts`
- `local`
- `onnx`
- `openai`
- `hybrid`

## What is now implemented

- Python-compatible config location/env handling
- project mining
- conversation mining
- general extraction mode
- incremental source revision tracking
- hybrid retrieval
- optional ONNX/local pretrained embedding backend
- layered memory stack for wake-up
- AAAK compression path
- knowledge graph tables and APIs
- palace graph utilities
- expanded MCP tool surface
- derived artifact storage (`drawer_type`)


### Retrieval caveats

- Default semantic retrieval is still not a true neural embedding model unless built with the optional `onnx-embeddings` feature.
- The optional ONNX path improves semantic retrieval materially, but still does not guarantee parity with the Python stack's exact embedding behavior.
- Fusion/reranking is stronger now, but still heuristic.

### AAAK caveats

- The Rust dialect is a practical port of the plain-text compression behavior, not a complete port of every legacy zettel-oriented path in `dialect.py`.

### Layering caveats

- L1 scoring is still simpler than Python's full importance metadata interpretation.
- L2 currently uses hybrid retrieval over a generic query seed rather than a dedicated retrieval-only path identical to Python.

### Mining / refresh caveats

- Runtime ingestion/compression/benchmark refresh paths now use owned refresh plans instead of `Box::leak`-driven planning.

## File layout

- `src/storage.rs` — SQLite storage, vectors, revisions, hybrid retrieval
- `src/embedding.rs` — embedding backend selection, optional ONNX path, local fallback vectorization
- `src/layers.rs` — L0/L1/L2/L3 stack
- `src/dialect.rs` — AAAK compression
- `src/extractor.rs` — general memory extraction
- `src/kg.rs` — temporal knowledge graph
- `src/graph.rs` — palace graph traversal and tunnel logic
- `src/mcp.rs` — expanded MCP server
- `src/project.rs` — project mining with updates
- `src/convo.rs` — convo mining with extract modes and updates
