# Claude Code project instructions

See `AGENTS.md` first — it holds the tool-agnostic engineering rules for this
repo (security posture, change workflow, verification commands). The sections
below are Claude-Code-specific mechanics for the two policies AGENTS.md states
abstractly: memory reuse and model selection.

## Codebase search: prefer codebase_memory MCP

For "where is X implemented," definitions, callers, architecture, or impact
analysis, prefer the `codebase_memory` MCP tools over blind `Grep`/`Glob`/file
reads — they return graph-enriched, token-efficient results instead of raw
file dumps.

Preference order:
1. `mcp__codebase_memory__search_graph` — definitions, classes, routes,
   "where is X" (`query` for natural language, `name_pattern` for exact
   regex, `semantic_query` for vocabulary-bridging search)
2. `mcp__codebase_memory__search_code` — literal string/regex search,
   graph-enriched
3. `mcp__codebase_memory__trace_path` — callers/callees, dependencies,
   data flow, impact analysis
4. `mcp__codebase_memory__get_code_snippet` — read one symbol's source via
   its `qualified_name`, instead of opening the whole file
5. `mcp__codebase_memory__get_architecture` / `query_graph` — module
   boundaries, complexity/hot-path queries

Confirm the repo is indexed first with `mcp__codebase_memory__index_status`
(project name = this path with `/`, `:` replaced by `-`, e.g.
`E:/Home/Documents/Programming/tz_combinator` → `E-Home-Documents-Programming-tz_combinator`).
Index with `mcp__codebase_memory__index_repository` (mode `fast` unless a
semantic query is actually needed) if it isn't indexed yet.

Fall back to `Grep`/`Glob`/`Read` when the repo isn't indexed and indexing
isn't practical, when searching non-code content (docs, config, generated
files), when a `codebase_memory` call errors, or when the path is already
known.

## Model economy per task

This repo is security-sensitive; reasoning quality matters more than cost on
threat modeling, security analysis, and ambiguous-requirement calls — but
most of the token spend in a session is mechanical (search, inventory,
running the same test command repeatedly). Route work to match:

**Subagent dispatch (`Agent` tool, `model` param) — the primary lever:**
- `haiku` — mechanical search, file inventory, repetitive/narrowly-specified
  test runs, simple formatting or transformation tasks
- omit (inherits session default) — standard implementation, ordinary bug
  fixes, routine refactors
- `opus` — threat modeling, security analysis, architecture/API design,
  complex debugging, ambiguous requirements, implementation planning
- Escalate mid-task (re-dispatch with a higher-tier model) if a cheap-model
  subagent hits ambiguity, conflicting requirements, security-sensitive
  judgment calls, or unexplained failing tests — don't let it guess.

**Main session model — I can't switch it myself; only `/model` and `/fast`
do that. When the mismatch is clear, say so once, briefly, rather than
grinding on the wrong tier:**
- Suggest `/model opus` (or `/fast`) when the task turns out to be a
  security-sensitive design call, a large ambiguous-requirements piece of
  work, or repeatedly hits judgment calls a cheaper tier keeps punting on.
- Suggest dropping to a cheaper model when a long session has shifted to
  extended mechanical work (bulk renames, repetitive verification runs)
  with no complex judgment left.
- Don't suggest a switch for every task — only when staying on the current
  tier is a clear mismatch, not a marginal one.

Model selection is advisory, per AGENTS.md — don't claim a specific model
ran unless the host exposes that information.
