# Init, config, and storage model

This document explains how MemPalace initializes a project, where configuration lives, where data is stored, and how wings are assigned.

## Global config directory

MemPalace keeps global state under:

```text
~/.mempalace
```

Important files and directories:

- `~/.mempalace/config.json` — global configuration
- `~/.mempalace/identity.txt` — L0 identity text used by `wake-up`
- `~/.mempalace/palace/` — default palace directory

You can override the palace location with:

- `--palace <path>`
- `MEMPALACE_PALACE_PATH`
- `MEMPAL_PALACE_PATH`
- `palace_path` in `~/.mempalace/config.json`

Precedence for palace path is:

1. `--palace`
2. `MEMPALACE_PALACE_PATH`
3. `MEMPAL_PALACE_PATH`
4. `palace_path` in global config
5. default `~/.mempalace/palace`

## What `init` does

Run:

```bash
cargo run -- init /path/to/project
```

`init` does two things:

1. Creates `~/.mempalace/config.json` if it does not already exist
2. Creates `/path/to/project/mempalace.yaml` if it does not already exist

If the project already has a `mempalace.yaml`, it is preserved.

## Project config: `mempalace.yaml`

Each indexed project can define a local config file:

```yaml
wing: my-project
rooms:
  - name: general
    keywords: []
  - name: src
    keywords: []
  - name: tests
    keywords: []
```

Fields:

- `wing` — the default wing name for this project
- `rooms` — room definitions used during project mining
  - `name` — room name
  - `keywords` — additional terms that help file-to-room classification

The miner also accepts a legacy `mempal.yaml`, but `init` writes `mempalace.yaml`.

If `general` is missing from the file, it is added automatically when the config is loaded.

## How the default project config is generated

When `init` creates `mempalace.yaml`, it:

- infers a wing name from the project directory name
- creates a `general` room
- adds one room for each top-level subdirectory that is not in the skip list

Directories skipped during inference include common generated or dependency folders such as `.git`, `node_modules`, `.venv`, `.next`, `dist`, `build`, `.mempalace`, and `target`.

## Wing behavior

A wing is the top-level namespace for memories. It is how MemPalace separates one project, agent, or source context from another.

Examples:

- a codebase might use wing `my-app`
- a conversation import might use wing `support-history`
- diary entries for an MCP agent use a generated wing like `wing_claude`

Wing selection during project mining is resolved in this order:

1. `--wing <name>` on the `mine` command
2. `wing` from `mempalace.yaml`
3. an inferred wing name based on the project directory

The inferred name is slugified. Generic directory names such as `project`, `projects`, `repo`, `repos`, `src`, `code`, `chat`, or `chats` cause MemPalace to look at the parent directory name instead.

## Rooms and halls

Rooms are the primary organization unit inside a wing. During project mining, files are assigned to rooms by matching:

- directory names in the relative path
- filename stems
- configured room keywords in file content

If no room matches, MemPalace uses `general`.

MemPalace also assigns a derived `hall` value such as:

- `hall_facts`
- `hall_events`
- `hall_preferences`
- `hall_advice`
- `hall_discoveries`
- `hall_diary`

Halls are inferred from room names, file paths, and content heuristics.

## Storage model

The palace uses SQLite as its source of truth. The main database file is:

```text
<palace-path>/mempalace.sqlite3
```

Core persisted tables include:

- `drawers` — raw drawers and derived artifacts
- `source_revisions` — source hash tracking for incremental refresh
- `vectors` — stored embeddings for semantic retrieval
- `drawers_fts` — SQLite FTS5 index for lexical search

Stored drawer fields include:

- `id`
- `wing`
- `room`
- `source_file`
- `chunk_index`
- `added_by`
- `filed_at`
- `content`
- `hall`
- `date`
- `drawer_type`
- `source_hash`
- `active`
- optional scoring fields such as `importance`, `emotional_weight`, and `weight`

## Active vs inactive drawers

MemPalace refreshes sources incrementally. When a source file changes:

- new drawers are inserted for the new source hash
- older drawers for the same source are marked inactive
- the latest hash is written to `source_revisions`

Search and status only use active drawers.

This lets MemPalace preserve refresh semantics without duplicating live results from outdated revisions.

## Source revision behavior

Project mining computes an MD5 hash of each source file's contents.

If the file's current hash matches the stored `source_revisions` record, mining skips that file.

If the hash changed, MemPalace refreshes the source transactionally:

- writes new raw drawers
- writes new derived artifacts
- updates vectors
- retires old active drawers for that source
- records the new source hash

## Derived artifacts

Not every stored item is a raw chunk. MemPalace also stores derived records using `drawer_type`, including compressed artifacts and project-derived artifacts.

Hybrid search can surface these artifacts, then resolve them back to a parent raw drawer for display.

## Identity and wake-up

`wake-up` uses the layered memory stack:

- L0 comes from `~/.mempalace/identity.txt`
- L1 is generated from the most important recent drawers, optionally preferring compressed artifacts

If `identity.txt` does not exist, `wake-up` prints a placeholder telling you where to create it.

To prefer compressed snippets in wake-up summaries:

```bash
export MEMPALACE_PREFER_COMPRESSED=1
```

## Example setup

Initialize a project and inspect the resulting files:

```bash
cargo run -- init .
```

Mine the project into the default palace:

```bash
cargo run -- mine . --mode projects
```

Mine it into a custom palace path:

```bash
cargo run -- --palace /tmp/my-palace mine . --mode projects
```

Override the wing at ingest time:

```bash
cargo run -- mine . --mode projects --wing docs-site
```