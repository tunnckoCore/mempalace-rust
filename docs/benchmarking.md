# Benchmarking

`mempalace-rust` includes a benchmark runner for evaluating retrieval quality across different backends.

## What the benchmark runner does

The benchmark command:

1. loads a benchmark dataset from JSON or JSONL
2. indexes the dataset into the current palace storage using synthetic source files
3. runs each benchmark query against the selected backend
4. computes retrieval metrics
5. prints a JSON result summary

The runner uses the same storage and retrieval code paths as the main application, so it is useful for comparing lexical, local semantic, ONNX, OpenAI, and hybrid retrieval behavior.

## Command

```bash
cargo run -- benchmark ./bench.json --backend hybrid --k 5
```

Arguments:

- `dataset` — path to the dataset file
- `--backend` — one of `fts`, `local`, `onnx`, `openai`, `hybrid`
- `--k` — result cutoff for metrics such as Recall@k and NDCG@k

## Input formats

The runner accepts two dataset formats.

### JSON format

```json
{
  "documents": [
    {
      "id": "doc1",
      "wing": "app",
      "room": "architecture",
      "content": "We switched to GraphQL to unify client data fetching.",
      "source_file": "bench/doc1.md"
    }
  ],
  "queries": [
    {
      "id": "q1",
      "query": "why did we switch to GraphQL",
      "relevant_ids": ["doc1"]
    }
  ]
}
```

### JSONL format

Each line is a JSON object with `kind` set to `document` or `query`.

```json
{"kind":"document","id":"doc1","wing":"app","room":"architecture","content":"We switched to GraphQL to unify client data fetching.","source_file":"bench/doc1.md"}
{"kind":"query","id":"q1","query":"why did we switch to GraphQL","relevant_ids":["doc1"]}
```

## Dataset fields

### Documents

Document fields:

- `id` — unique drawer ID used for relevance matching
- `wing` — wing to index into
- `room` — room to index into
- `content` — indexed text
- `source_file` — optional source file label

If `source_file` is omitted or empty, the runner generates one like:

```text
bench/<id>.md
```

Documents are grouped by `source_file` before indexing, then written using source refresh semantics.

### Queries

Query fields:

- `id` — query identifier
- `query` — search text
- `relevant_ids` — document IDs considered relevant

## Output

The benchmark command prints a JSON object such as:

```json
{
  "queries": 10,
  "recall_at_k": 0.8,
  "mrr": 0.67,
  "ndcg_at_k": 0.74,
  "backend": "hybrid"
}
```

Metrics:

- `queries` — number of benchmark queries evaluated
- `recall_at_k` — fraction of queries with at least one relevant result in the top `k`
- `mrr` — mean reciprocal rank of the first relevant result
- `ndcg_at_k` — normalized discounted cumulative gain at `k`
- `backend` — the backend used for evaluation

After the JSON result, the CLI also prints a one-line help summary describing the accepted dataset format.

## Backend behavior

### `fts`

Uses lexical FTS search only.

### `local`

Uses semantic search with local embeddings and forces:

```text
MEMPALACE_LOCAL_EMBEDDING_PROVIDER=builtin
```

### `onnx`

Uses semantic search with local embeddings and forces:

```text
MEMPALACE_LOCAL_EMBEDDING_PROVIDER=onnx
```

If the binary was not built with ONNX support, semantic behavior falls back according to the embedding implementation.

### `openai`

Uses semantic search with OpenAI embeddings. You must configure an API key and any non-default model or base URL you need.

### `hybrid`

Uses the full hybrid search path, combining lexical and semantic scoring.

## Limits

The runner enforces these safeguards:

- maximum dataset size: 2 MiB
- maximum document count for JSON datasets: 5,000 documents

If a dataset exceeds these limits, the command fails with an error.

## Notes about indexing

The benchmark runner writes benchmark documents into the current palace storage using normal refresh logic. Running a benchmark can therefore populate or update benchmark-labeled sources in that palace.

If you want isolated benchmark runs, point MemPalace at a separate palace path:

```bash
cargo run -- --palace /tmp/mempalace-bench benchmark ./bench.json --backend hybrid --k 5
```

## Examples

Hybrid benchmark:

```bash
cargo run -- benchmark ./bench.json --backend hybrid --k 5
```

Built-in local embeddings only:

```bash
cargo run -- benchmark ./bench.json --backend local --k 10
```

ONNX benchmark:

```bash
cargo build --features onnx-embeddings
cargo run --features onnx-embeddings -- benchmark ./bench.json --backend onnx --k 5
```

OpenAI benchmark:

```bash
export OPENAI_API_KEY=sk-...
cargo run -- benchmark ./bench.json --backend openai --k 5
```