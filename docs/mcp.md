# MCP server usage

`mempalace-rust` includes a stdio MCP server for querying and updating the palace from an MCP-capable client.

## Start the server

Run:

```bash
cargo run -- mcp --transport stdio
```

`stdio` is currently the only supported transport.

On startup the server writes a readiness line to **stderr**, not stdout:

```text
mempalace-rust MCP ready (transport=stdio, backend=..., local_provider=...)
```

This keeps stdout available for MCP protocol messages.

## Message framing

The server accepts two input styles on stdin:

- standard MCP `Content-Length` framed messages
- newline-delimited JSON as a simpler fallback

Responses are always written with `Content-Length` framing.

The implementation enforces a maximum incoming message size of 1 MiB.

## Initialization flow

The server handles these protocol methods:

- `initialize`
- `tools/list`
- `tools/call`
- `notifications/initialized`

Unknown methods return JSON-RPC error `-32601`.

## Mutation gating

Mutating tools are disabled by default.

To enable them:

```bash
export MEMPALACE_ENABLE_MUTATIONS=1
```

Accepted truthy values are:

- `1`
- `true`
- `yes`
- `on`

If mutations are disabled:

- mutating tools are removed from `tools/list`
- direct calls to mutating tools fail with an error

Currently gated mutating tools:

- `mempalace_add_drawer`
- `mempalace_delete_drawer`
- `mempalace_kg_add`
- `mempalace_kg_invalidate`
- `mempalace_diary_write`

## Main tools

Core read-oriented tools:

- `mempalace_status` — palace overview, embedding config, protocol guidance, and AAAK spec
- `mempalace_list_wings` — list wings with counts
- `mempalace_list_rooms` — list rooms, optionally within a wing
- `mempalace_get_taxonomy` — full wing/room taxonomy
- `mempalace_get_aaak_spec` — return the AAAK spec text
- `mempalace_get_compact_context` — recent raw and compressed context for a wing or room
- `mempalace_search` — hybrid search with optional wing and room filters
- `mempalace_check_duplicate` — check whether content already exists
- `mempalace_kg_query` — query the temporal knowledge graph
- `mempalace_kg_timeline` — timeline view of facts
- `mempalace_kg_stats` — knowledge graph summary
- `mempalace_traverse` — walk the room graph
- `mempalace_find_tunnels` — find bridge rooms across wings
- `mempalace_graph_stats` — graph summary
- `mempalace_diary_read` — read recent diary entries for an agent

Mutating tools when enabled:

- `mempalace_add_drawer`
- `mempalace_delete_drawer`
- `mempalace_kg_add`
- `mempalace_kg_invalidate`
- `mempalace_diary_write`

## Important tool behavior

### `mempalace_status`

Returns:

- total drawers
- wings and room counts
- `palace_path`
- the built-in MemPalace protocol guidance string
- the AAAK dialect spec
- artifact type counts
- active embedding configuration

This is the best first call after connecting.

### `mempalace_get_compact_context`

Returns recent drawers plus compressed variants when available. Each item can include:

- raw content
- matching compressed content
- wing
- room
- source file
- date

This is useful for compact prompt assembly.

### `mempalace_search`

Runs hybrid search over FTS plus stored vectors. Inputs:

- `query` — required
- `limit` — optional, clamped to `1..=50`
- `wing` — optional
- `room` — optional

Queries that are too long or contain no valid searchable tokens return an error payload.

### `mempalace_check_duplicate`

Checks for likely duplicates using the same hybrid scoring stack. `threshold` is clamped to `0.0..=1.0`.

### Diary tools

`mempalace_diary_write` stores entries in a generated wing named:

```text
wing_<agent_name>
```

with room `diary` and drawer type `diary_entry`.

`mempalace_diary_read` reads from that generated wing.

## Example MCP calls

### Initialize

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
```

### List tools

```json
{"jsonrpc":"2.0","id":2,"method":"tools/list"}
```

### Search

```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "method": "tools/call",
  "params": {
    "name": "mempalace_search",
    "arguments": {
      "query": "why did we switch to GraphQL",
      "wing": "my-app",
      "limit": 5
    }
  }
}
```

### Add a drawer with mutations enabled

```bash
export MEMPALACE_ENABLE_MUTATIONS=1
```

```json
{
  "jsonrpc": "2.0",
  "id": 4,
  "method": "tools/call",
  "params": {
    "name": "mempalace_add_drawer",
    "arguments": {
      "wing": "my-app",
      "room": "architecture",
      "content": "We switched to GraphQL to unify multiple client-specific REST calls.",
      "added_by": "mcp"
    }
  }
}
```

## Storage used by MCP

The MCP server opens the same palace SQLite store used by the CLI. Knowledge graph data is opened from the global MemPalace config directory.

That means MCP sees the same active drawers, vectors, artifacts, and knowledge graph state as CLI commands that point at the same palace path.