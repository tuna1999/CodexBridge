# CodexBridge Architecture

## Product shape

CodexBridge is a multi-user, multi-conversation Streamable HTTP MCP daemon with a deliberately fixed native tool surface. It does not load an application config file. Operator-configured upstream MCP servers are an optional additive surface; gateway mode is the default so large upstream catalogues are progressively disclosed instead of injected wholesale.

Normal startup is:

```text
codex-bridge [workspace]
```

where `workspace` defaults to `/workspace`. Environment variables are deployment overrides, not a required configuration layer.

## Main layers

1. **HTTP/MCP server (`server.rs`)** — authentication, request/session limits, health, Streamable HTTP transport, and graceful shutdown.
2. **Conversation project identity (`request_context.rs`, `project.rs`)** — derives project identity from `openai/subject` + `openai/session`, prepares initialization, and atomically commits bindings only after the initialization success boundary.
3. **Compact tool registry (`tools/registry.rs`)** — explicitly registers the fixed public Codex-style native tool set and rejects duplicate/unlisted native routes.
4. **Filesystem/process isolation (`sandbox.rs`)** — no-follow capability I/O for normal project files plus process execution with Bubblewrap/native backend selection.
5. **Agent context (`tools/agent/*`, `runtime_environment.rs`)** — identity-independent pre-init instructions, shared runtime environment facts, home ecosystem discovery, project instruction precedence, skills, symlink-aware agent content, and bounded initialized-project context.
6. **Persistent state (`storage.rs`)** — SQLite WAL with one dedicated serialized writer and four query-only reader connections for aliases/bindings, durable memory, plans, and compatibility data retained from earlier schema versions.
7. **Upstream aggregation (`upstream.rs`)** — opt-in stdio/Streamable-HTTP MCP clients, bounded direct forwarding, or one-tool gateway dispatch with generated progressive skills.
8. **Audit (`audit.rs`)** — bounded asynchronous audit logging, activity tracking, and credential redaction.

## Public tool surface

The public native registry is intentionally fixed at 15 tools:

```text
chatgpt_turn_init
apply_patch
read_file
list_directory
tree
glob
grep
view_image
exec_command
write_stdin
skills_list
skills_read
remember
recall
update_plan
```

The compiled native routers contain only those public routes; historical Git/task/file-mutation compatibility routers are not part of the active tool tree. Server internals therefore do not become model-visible simply because an implementation exists elsewhere in the crate history.

Upstream MCP aggregation is separate from that 15-tool native contract. Without `MCP_UPSTREAM_CONFIG`, there are no upstream routes. With it, `server::run` connects the configured servers and `AgentHandler::new` adds the resulting direct/gateway routes after the native registry is built.

Gateway mode is the default. One `gateway_<server>` dispatcher exposes a bounded function enum, while the full descriptions/schemas live in a generated `gateway_<server>` skill that `skills_list`/`skills_read` disclose only when selected. Direct mode is explicit and exposes `upstream_<server>__<tool>` routes. Both modes require an initialized project and pass through the normal audit/capacity path plus upstream-specific timeout/concurrency limits.

## Initialization lifecycle

All project-scoped tools except `chatgpt_turn_init` require a persisted initialized conversation binding.

Before that binding exists, `ServerInfo.instructions` is deliberately identity-independent. It contains the generic agent brief, exact shell/sandbox facts from the shared `RuntimeEnvironment`, the requirement to call `chatgpt_turn_init`, and global gateway guidance. It does not inspect a project directory, memory, plan, or project skill catalogue because MCP `initialize` has no trustworthy project identity argument of its own.

`chatgpt_turn_init` combines the first project-binding boundary with later per-user-turn synchronization. For each new user message that needs project state, the agent calls it exactly once before any other project tool. On later turns, the nearest preceding CodexBridge `[ref:...]` marker is passed explicitly as `previous_turn_ref`. The successful response says that turn initialization has already run and requires the current final-answer marker.

Turn initialization is two-phase around independently hashed instruction/state builds:

1. `prepare_turn_initialize` computes the proposed effective project and alias relationship without persistence. A same-subject branched conversation may inherit the effective project from `previous_turn_ref`.
2. Capacity is acquired. The instruction context is rebuilt from environment facts, skill/gateway catalogues, and project instructions; saved memory/plan are rendered separately as a bounded state snapshot.
3. SHA-256 is computed independently as `instruction_hash` and `state_hash`. Plan timestamps are excluded from the semantic state snapshot, and the turn protocol/UUIDv7 reference are excluded from both hashes.
4. A compact candidate UUIDv7 `turn_ref` is generated.
5. `commit_initialize_with_turn_ref` atomically verifies binding/alias expectations, validates parent/effective-project/subject scope, stores both hashes, and either inserts the new turn or returns an existing child for the same `(native_key, previous_turn_ref)`.
6. New/branched conversations or instruction changes return the full brief. A memory/plan-only change returns `state_update` with `brief=null`. If neither hash changed, both payloads are null and only a compact turn receipt is returned.

A turn-init request that returns an error must not partially commit a new binding or turn reference.

The result reports one of three lifecycle states: `new` for a newly bound conversation project, `existing` when the same persisted effective project is being reused, and `joined` when a fresh conversation joins an existing alias or same-subject branch reference. It returns `turn_ref`, `previous_turn_ref`, `instruction_hash`, `state_hash`, `instructions_changed`, `state_changed`, and `turn_reused`, plus optional `brief`/`state_update` payloads. Current saved state, skills, gateway metadata, and project instructions are observed each turn without treating routine state churn as an instruction-context change.

For non-root turns, duplicate calls are hard-idempotent: `(native_key, previous_turn_ref)` has at most one child, so a retry returns the existing `turn_ref`. Once a native conversation has a binding, omitting `previous_turn_ref` is rejected instead of creating an ambiguous next turn. `previous_turn_ref` is also scoped to a hash of `openai/subject`; another subject cannot use a leaked ref as a project-join capability. CodexBridge is development-stage and starts directly on the current strict schema-v3 layout rather than migrating older layouts.

MCP request metadata still provides no trustworthy user-message/turn identifier. Therefore the server cannot detect a later user message for which the model never invokes `chatgpt_turn_init` at all. The requirement to invoke it exactly once at the start of each project-bearing user turn remains an agent protocol rule, while duplicate same-parent calls are server-enforced without timing heuristics.

## Workspace and authentication state

The process workspace contains both conversation projects and service metadata:

```text
<workspace>/
  <effective-project-key>/
  .metadata/
    auth-token
    agent.sqlite3
    logs/
    projects/
```

When `MCP_AUTH_TOKEN` is absent, startup creates `.metadata/auth-token` using a random path-safe token. On Unix the metadata directory is mode 0700 and the token file is mode 0600. The token is reused across restarts.

Authentication is mandatory and can use path, bearer, or either transport. `/health` is intentionally unauthenticated.

## Path policies

### Normal project files

Ordinary filesystem operations reject absolute paths, traversal, and symlink components. On Unix, critical reads and mutations walk from an open project-directory descriptor using no-follow `openat`/`mkdirat`/`renameat`/`unlinkat` operations. This avoids validating a pathname and then reopening an attacker-swappable path later.

### Agent content

`AGENTS.md`, rule files, skill packages, Claude plugin skills, and skill resources follow a separate trust policy. Symlinks are allowed, including shared targets outside the project root. This is intentional and must not weaken the normal filesystem resolver.

Agent content is bounded independently by total instruction bytes, file count, skill catalogue size, package traversal size, and per-document/page limits. Project instruction reads truncate at the remaining aggregate budget rather than failing conversation initialization solely because one instruction file is too large. Generated upstream gateway skills use a separate bounded catalogue and are marked as reference metadata rather than higher-priority instructions.

Skill discovery follows the current Codex root model: `.agents/skills` is searched along project-root-to-target ancestry and each skills root is recursively scanned with a depth/directory bound. Because CodexBridge has no persistent process CWD, `skills_list(path=...)` and `skills_read(path=...)` let the caller supply the relevant project path for the equivalent ancestry lookup. `.codex/skills` is accepted at the same levels as a lower-precedence compatibility alias. User roots are `~/.agents/skills` and the deprecated `$CODEX_HOME/skills`; repo `.claude/skills` is not a Codex root, while Claude plugin-cache skills remain a separate namespaced source.

## Process execution

`exec_command` is the single general shell/process primitive. `write_stdin` continues a live process and also handles PTY resize, stdin close, and bounded signals.

The execution backend has three configured modes:

- `auto` — prefer Bubblewrap only when a real probe succeeds; otherwise use native execution.
- `bwrap` — request Bubblewrap, but native fallback remains available when YOLO/native execution is enabled.
- `none` — native execution.

Native execution is enabled by default because CodexBridge is YOLO by design. Operators who need fail-closed Bubblewrap can set `MCP_ALLOW_UNSANDBOXED_EXEC=false`.

### Bubblewrap probe

Availability is not inferred from the existence of `/usr/bin/bwrap`. The process runs a one-time namespace/mount probe. This matters in rootless and nested Podman environments where the binary exists but the kernel/container policy rejects Bubblewrap namespaces.

### Podman execution

Podman follows capability rather than a hard-coded bypass. When Bubblewrap is usable, the first Podman command triggers a bounded, non-mutating `podman info --format json` probe inside the Bubblewrap execution shape. A successful probe keeps subsequent Podman commands inside Bubblewrap. A failed or timed-out probe selects the native backend for Podman commands only. If Bubblewrap itself is unavailable, all shell execution already uses the native backend, including Podman.

CodexBridge does not decide whether a project should use Podman for builds, dependency installation, runtime builds, or container execution. That workflow policy belongs in project/user agent instructions such as `AGENTS.md`.

### Shell selection

Default shell and shell syntax family are computed from the actual effective execution backend. `.exe` suffix classification is case-insensitive so `cmd.Exe`, `PowerShell.Exe`, and `pwsh.EXE` retain the correct Windows invocation semantics.

## Output model

Hard resource ceilings protect the daemon. Smaller presentation budgets protect model context. High-volume tools return structured content plus compact readable text and explicit truncation/continuation state.

`read_file` is line-oriented but its continuation cursor has two dimensions. Normal pagination advances the logical `offset`; when a single line exceeds the presentation byte budget, `next_offset` remains on that line and `next_line_byte_offset` advances at a valid UTF-8 boundary. The next call passes that value as `line_byte_offset`, so even one-line minified/generated files can be consumed without dropping an inaccessible middle/tail segment.

The public registry uses typed output schemas for high-risk structured responses so schema and runtime behavior are derived from the same Rust data types where practical.

## SQLite concurrency

The storage schema remains one SQLite database in WAL mode. The runtime no longer places every operation behind one `Arc<Mutex<Connection>>`. A dedicated writer thread owns the only mutation connection and therefore preserves write ordering and initialization transaction semantics. Four `query_only=ON` reader connections are selected from a bounded pool; independent readers can run concurrently, and WAL allows those readers to coexist with the writer's commits. Read-after-write behavior remains deterministic because a write API returns only after its queued writer operation has completed.

Schema version 3 stores each `turn_ref` with native/effective project keys, a non-null subject-scope hash, parent reference, non-null `instruction_hash` and `state_hash`, and creation time. A partial unique index on `(native_key,parent_turn_ref)` makes later-turn retries idempotent. Older development layouts, including an earlier v3 layout with one `context_hash`, are intentionally unsupported rather than migrated; startup validates the required v3 columns and requires recreation when the layout is stale. Turn-reference insertion is inside the same transaction as binding/alias commit. **TODO(rewind):** attach immutable workspace checkpoint metadata/content to these references and expose compare/rewind semantics later; no rewind operation exists today.

Initialization handover excerpts are byte-bounded at UTF-8 character boundaries and explicitly marked when truncated; character counts are never substituted for byte budgets.

## Agent state

The public state surface intentionally consists of only:

- `remember`
- `recall`
- `update_plan`

The database contains some internal task/memory/plan data structures that are not part of the public tool surface. Those tables do not imply public CRUD tools. Initialization handover exposes bounded plan and memory summaries only.

## Verification

CI verifies:

- formatting;
- all-target/all-feature compilation;
- cross-platform tests on Linux/macOS/Windows;
- Clippy with warnings denied on Linux;
- binary/example builds;
- a real Streamable HTTP smoke test covering generated authentication, exact native tool registry, identity-independent initialize instructions, first-turn full context, unchanged-context compact receipts, duplicate-parent idempotency, AGENTS/skill hash refresh, same-subject branching, cross-subject ref rejection, filesystem/search/patch atomicity, skills and missing-skill recovery, cross-project process-session isolation, image output, normalized continuity inputs, auth rejection, legacy MCP sessions, and deterministic direct/gateway upstream forwarding.

Container verification additionally exercises the runtime image where Bubblewrap may be installed but unusable because the daemon itself is already inside Podman. The expected behavior in that environment is automatic native fallback, not daemon startup failure.
