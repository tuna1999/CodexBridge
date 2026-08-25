# Changelog

All notable changes to this project are documented here. The format follows Keep a Changelog and the project intends to use Semantic Versioning once the public compatibility policy is finalized.

## [Unreleased]

### Added
- Byte-cursored process output: `exec_command`/`write_stdin` responses carry `output_offset`/`output_next_offset` ranges, deliver each chunk exactly once, and accept `since_output_offset` to re-render buffered output after a lost response instead of re-running the command. Finished sessions stay briefly pollable for output recovery.
- Line-ending-preserving patch application: LF-authored hunks apply to CRLF/mixed-EOL files, each replaced line keeps its original ending, and inserted lines inherit the replaced region's ending instead of renormalizing whole files.
- Separate presentation budgets for file, multi-file, search, listing, and tree outputs.
- Persistent automatically generated endpoint authentication token when `MCP_AUTH_TOKEN` is absent.
- Derived output schemas for high-risk structured tool responses.
- Integration regression suites for project initialization and filesystem confinement.
- `MCP_AUTH_MODE=path|bearer|either` while keeping authentication mandatory.
- Runtime Bubblewrap usability probing and automatic native fallback for restricted/nested-container environments.
- Podman capability probing inside Bubblewrap, with Podman-only native fallback when the probe fails; Docker/Buildah are not special-cased.
- Architecture documentation for the compact CodexBridge runtime model.
- Opt-in upstream MCP aggregation through `MCP_UPSTREAM_CONFIG`, with stdio/Streamable-HTTP transports, explicit direct mode, gateway-by-default dispatch, bounded generated gateway skills, upstream call limits, and end-to-end smoke coverage.
- Shared identity-independent `RuntimeEnvironment` facts reused by MCP initialize instructions, initialized project briefs, exec descriptions, and diagnostics without adding another public environment tool.
- Outside-in lifecycle regression coverage for new/existing/joined project initialization plus expanded MCP smoke coverage for patch atomicity, cross-project process-session isolation, skill recovery, and normalized continuity inputs.
- Broad behavioral regression coverage adapted from the codex-free test matrix across patch parsing/apply, ignore/search pagination, UTF-8 file windows, project-doc/skill precedence and budgets, shell/process contracts, SQLite state invariants, upstream failure handling, schema contracts, and outside-in filesystem/state suites.
- Persistent compact UUIDv7 `turn_ref` records with explicit parent links, subject scope, and stable project-context hashes, stored atomically with project binding as anchors for future checkpoint/rewind support.

### Changed
- Renamed the package/binary and MCP implementation to `codex-bridge` / `CodexBridge`.
- Simplified startup to `codex-bridge [workspace]`, defaulting to `/workspace`; application config files and config subcommands are no longer part of the product surface.
- Reduced the public native MCP registry to 15 Codex-style tools and removed duplicate CRUD/compatibility/Git/clock/download exposure. The compiled native registry is now explicit and rejects duplicate or unlisted native routes instead of composing historical routers and filtering afterward.
- Removed unreachable historical Git/task/filesystem-mutation tool routers from the compiled tool tree and kept only the three public continuity tools (`remember`, `recall`, `update_plan`) from the old compatibility module.
- Upstream mode now defaults to `gateway`; `direct` remains an explicit operator choice when individual upstream tools should be model-visible.
- Search pagination now rejects zero-sized result windows and oversized single matches instead of returning a continuation that cannot make progress; empty optional search/list/tree paths consistently mean the project root.
- Removed `get_agent_brief` (project context is synchronized by `chatgpt_turn_init`) and `get_environment` (runtime shell/backend details are in the `exec_command` description and startup diagnostics).
- Split agent instruction budgets into a 128 KiB daemon-home/global budget and an independent 256 KiB project budget.
- Restricted public `apply_patch` input to Codex's `*** Begin Patch` grammar; legacy path/files patch forms are no longer model-visible.
- YOLO/native execution is enabled by default; Bubblewrap remains preferred when it actually works.
- `read_file`, `read_files`, `glob`, `search_files`, `grep`, `list_directory`, and `tree` now use compact agent-facing text renderers alongside structured content.
- `list_directory` paginates before statting file metadata.
- Oversized project instructions now consume the remaining bounded instruction budget instead of making initialization fail solely because the file is too large.
- Skill identity lookup is case-insensitive while preserving declared names.
- SQLite state access now uses WAL with one dedicated serialized writer and four query-only reader connections instead of one application-wide connection mutex, allowing independent reads and WAL reader/writer overlap while retaining ordered writes.
- `ServerInfo.instructions` now carries the full identity-independent coding-agent brief and exact runtime shell/sandbox guidance; project-specific state/skills/instructions remain gated behind `chatgpt_turn_init`.
- Renamed `chatgpt_remote_init` to `chatgpt_turn_init` to match its real lifecycle: first project binding plus synchronization at the start of every project-bearing user turn.
- Later `chatgpt_turn_init` calls now take `previous_turn_ref`. Duplicate calls with the same parent are SQLite-idempotent and return the same child reference; an already-bound conversation omitting the parent is rejected instead of creating an ambiguous extra turn.
- Turn synchronization now hashes instruction context and saved project state independently. AGENTS/skills/gateway/environment changes update `instruction_hash` and can return a full refreshed brief; memory/plan-only changes update `state_hash` and return only `state_update`, so routine plan churn does not re-inject the full coding brief. Plan timestamps are excluded from the semantic state hash.
- Same-subject branched conversations can inherit the referenced effective project without repeating `project_key`. Turn references are subject-scoped and cannot be used by another OpenAI subject as a project-join capability.
- SQLite now starts directly on strict schema v3: turn refs store non-null `instruction_hash`, `state_hash`, subject scope, and a unique native-parent index. Pre-v3 databases and the earlier development v3 `context_hash` layout are rejected instead of migrated.
- Skill discovery now follows current OpenAI Codex semantics: recursive `.agents/skills` roots are selected along project-root-to-target ancestry, `~/.agents/skills` is the canonical user root, and deprecated `$CODEX_HOME/skills` remains compatible. CodexBridge additionally accepts `.codex/skills` as a lower-precedence repo alias; repo `.claude/skills` is no longer treated as a Codex skill root, while namespaced Claude plugin-cache skills remain supported. `skills_list`/`skills_read` accept `path` for nested-scope discovery.
- Agent context internals are split into dedicated instruction, skill, project-doc, plugin, home, and bounded-content modules instead of keeping discovery/rendering logic in one monolithic file.
- `remember` trims memory keys and rejects whitespace-only keys; blank `update_plan.explanation` values normalize to `null`.
- Skill package resources accept an explicit `./` current-directory prefix while continuing to reject parent traversal, absolute forms, and backslash paths.

### Fixed
- Windows shell classification now strips `.exe` case-insensitively (`cmd.Exe`, `PowerShell.Exe`, `pwsh.EXE`).
- `read_file` no longer loses the remainder of a logical line that exceeds the presentation byte budget; oversized lines continue with a UTF-8-safe `line_byte_offset` cursor on the same line.
- Project handover truncation now enforces its 48 KiB limit in UTF-8 bytes rather than Unicode scalar count and marks truncated excerpts explicitly.
- `skills_read` now reports a bounded list of available skill names when lookup fails, reducing recovery round trips without exposing skill package paths.
- `apply_patch` accepts harmless blank lines surrounding the `*** Begin Patch` / `*** End Patch` envelope.
- Whitespace-only project instruction files are ignored without consuming the bounded instruction byte/file budget.
