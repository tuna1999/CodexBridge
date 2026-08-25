use rmcp::{ErrorData, model::CallToolResult};
use serde_json::json;

use super::{AgentHandler, PatchUpdate, capability_patch_transaction};
use crate::{error::AppError, request_context::ProjectRequestContext};

const BEGIN: &str = "*** Begin Patch";
const END: &str = "*** End Patch";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Add {
        path: String,
        content: String,
    },
    Delete {
        path: String,
    },
    Update {
        path: String,
        destination: Option<String>,
        chunks: Vec<Chunk>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub anchor: Option<String>,
    pub old: Vec<String>,
    pub new: Vec<String>,
    /// Exact old/new indexes for lines authored as patch context (` ` or a
    /// bare empty context line). Content equality cannot reconstruct this after
    /// deletions or duplicate lines, so preserve the parser's intent just like
    /// Codex `UpdateFileChunk::context_line_indices` does.
    pub context_line_indices: Vec<(usize, usize)>,
    pub eof: bool,
}

fn header(line: &str) -> bool {
    line.starts_with("*** Add File: ")
        || line.starts_with("*** Delete File: ")
        || line.starts_with("*** Update File: ")
        || line == END
}

pub fn parse(input: &str) -> Result<Vec<Action>, AppError> {
    let normalized = input.replace("\r\n", "\n");
    let all_lines: Vec<&str> = normalized.lines().collect();
    let first = all_lines.iter().position(|line| !line.trim().is_empty());
    let last = all_lines.iter().rposition(|line| !line.trim().is_empty());
    let lines: &[&str] = match (first, last) {
        (Some(first), Some(last)) if first <= last => &all_lines[first..=last],
        _ => &[],
    };
    if lines.first().copied() != Some(BEGIN) || lines.last().copied() != Some(END) {
        return Err(AppError::new(
            "INVALID_PATCH",
            "Codex patch must start with *** Begin Patch and end with *** End Patch",
        ));
    }
    let mut actions = Vec::new();
    let mut index = 1usize;
    while index + 1 < lines.len() {
        let line = lines[index];
        if let Some(path) = line.strip_prefix("*** Add File: ") {
            index += 1;
            let mut added = Vec::new();
            while index < lines.len() && !header(lines[index]) {
                let body = lines[index].strip_prefix('+').ok_or_else(|| {
                    AppError::new(
                        "INVALID_PATCH",
                        "every Add File body line must start with +",
                    )
                })?;
                added.push(body);
                index += 1;
            }
            actions.push(Action::Add {
                path: path.to_owned(),
                content: if added.is_empty() {
                    String::new()
                } else {
                    format!("{}\n", added.join("\n"))
                },
            });
            continue;
        }
        if let Some(path) = line.strip_prefix("*** Delete File: ") {
            actions.push(Action::Delete {
                path: path.to_owned(),
            });
            index += 1;
            continue;
        }
        if let Some(path) = line.strip_prefix("*** Update File: ") {
            index += 1;
            let destination = lines
                .get(index)
                .and_then(|line| line.strip_prefix("*** Move to: "))
                .map(str::to_owned);
            if destination.is_some() {
                index += 1;
            }
            let mut chunks = Vec::new();
            while index < lines.len() && !header(lines[index]) {
                // Codex accepts the first update chunk without an explicit @@
                // header. Subsequent chunks still need @@ so the boundary is
                // unambiguous.
                let anchor = if let Some(marker) = lines[index].strip_prefix("@@") {
                    index += 1;
                    (!marker.trim().is_empty()).then(|| marker.trim().to_owned())
                } else if chunks.is_empty() {
                    None
                } else {
                    return Err(AppError::new(
                        "INVALID_PATCH",
                        "subsequent Update File chunks must start with @@",
                    ));
                };
                let mut old = Vec::new();
                let mut new = Vec::new();
                let mut context_line_indices = Vec::new();
                let mut eof = false;
                while index < lines.len()
                    && !header(lines[index])
                    && !lines[index].starts_with("@@")
                {
                    let body = lines[index];
                    if body == "*** End of File" {
                        eof = true;
                        index += 1;
                        continue;
                    }
                    match body.as_bytes().first().copied() {
                        Some(b'+') => new.push(body[1..].to_owned()),
                        Some(b'-') => old.push(body[1..].to_owned()),
                        Some(b' ') => {
                            let old_index = old.len();
                            let new_index = new.len();
                            old.push(body[1..].to_owned());
                            new.push(body[1..].to_owned());
                            context_line_indices.push((old_index, new_index));
                        }
                        None => {
                            let old_index = old.len();
                            let new_index = new.len();
                            old.push(String::new());
                            new.push(String::new());
                            context_line_indices.push((old_index, new_index));
                        }
                        _ => {
                            return Err(AppError::new(
                                "INVALID_PATCH",
                                "update lines must start with space, +, or -",
                            ));
                        }
                    }
                    index += 1;
                }
                chunks.push(Chunk {
                    anchor,
                    old,
                    new,
                    context_line_indices,
                    eof,
                });
            }
            if chunks.is_empty() {
                return Err(AppError::new(
                    "INVALID_PATCH",
                    "Update File contains no chunks",
                ));
            }
            actions.push(Action::Update {
                path: path.to_owned(),
                destination,
                chunks,
            });
            continue;
        }
        return Err(AppError::new(
            "INVALID_PATCH",
            format!("unexpected patch line: {line}"),
        ));
    }
    if actions.is_empty() {
        return Err(AppError::new("INVALID_PATCH", "patch contains no actions"));
    }
    Ok(actions)
}

fn normalized(value: &str) -> String {
    value
        .trim()
        .chars()
        .map(|character| match character {
            '\u{2010}'..='\u{2015}' | '\u{2212}' => '-',
            '\u{2018}'..='\u{201b}' => '\'',
            '\u{201c}'..='\u{201f}' => '"',
            '\u{00a0}' | '\u{2000}'..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}' => ' ',
            other => other,
        })
        .collect()
}

fn find_sequence(
    lines: &[(String, bool)],
    wanted: &[String],
    start: usize,
    eof: bool,
) -> Option<usize> {
    if wanted.is_empty() {
        return Some(lines.len());
    }
    if wanted.len() > lines.len() {
        return None;
    }
    let last = lines.len() - wanted.len();
    let first = if eof { last } else { start.min(last) };
    for mode in 0..4 {
        for index in first..=last {
            let equal = wanted.iter().enumerate().all(|(offset, right)| match mode {
                0 => lines[index + offset].0 == *right,
                1 => lines[index + offset].0.trim_end() == right.trim_end(),
                2 => lines[index + offset].0.trim() == right.trim(),
                _ => normalized(&lines[index + offset].0) == normalized(right),
            });
            if equal {
                return Some(index);
            }
        }
    }
    None
}

pub fn apply(original: &str, chunks: &[Chunk], path: &str) -> Result<String, AppError> {
    // Split into (content, eol) pairs once. Matching runs on EOL-stripped
    // content, so a patch authored with LF applies cleanly to CRLF files and
    // to files with mixed line endings. Each replacement line inherits the
    // EOL of the source line it replaces, keeping mixed endings intact;
    // purely inserted lines use the surrounding region's ending.
    let trailing_newline = original.ends_with('\n');
    // A zero-byte file has no lines at all: treating its single empty split
    // fragment as a real line would invent a blank first line when appending
    // via an empty-old chunk.
    let mut lines: Vec<(String, bool)> = if original.is_empty() {
        Vec::new()
    } else {
        original
            .split('\n')
            .map(|raw| {
                (
                    raw.strip_suffix('\r').unwrap_or(raw).to_owned(),
                    raw.ends_with('\r'),
                )
            })
            .collect()
    };
    if trailing_newline {
        lines.pop();
    }

    fn eol(is_crlf: bool) -> &'static str {
        if is_crlf { "\r\n" } else { "\n" }
    }

    let dominant_crlf = |lines: &[(String, bool)]| {
        let crlf = lines.iter().filter(|(_, is_crlf)| *is_crlf).count();
        crlf * 2 > lines.len()
    };

    let mut cursor = 0usize;
    for chunk in chunks {
        if let Some(anchor) = &chunk.anchor {
            cursor = find_sequence(&lines, std::slice::from_ref(anchor), cursor, false)
                .ok_or_else(|| {
                    AppError::new(
                        "INVALID_PATCH",
                        format!("failed to find anchor in {path}: {anchor}"),
                    )
                })?
                + 1;
        }
        // Patches authored against a trailing newline often end the matched
        // region with an empty line that has no counterpart in this split's
        // line list. Retry once without that sentinel (codex apply-patch does
        // the same) before giving up.
        let mut pattern: &[String] = &chunk.old;
        let mut replacement_source: &[String] = &chunk.new;
        let mut at = find_sequence(&lines, pattern, cursor, chunk.eof);
        if at.is_none() && chunk.old.last().is_some_and(String::is_empty) && !chunk.eof {
            pattern = &chunk.old[..chunk.old.len() - 1];
            if chunk.new.last().is_some_and(String::is_empty) {
                replacement_source = &chunk.new[..chunk.new.len() - 1];
            }
            at = find_sequence(&lines, pattern, cursor, chunk.eof);
        }
        let at = at.ok_or_else(|| {
            AppError::new(
                "INVALID_PATCH",
                format!(
                    "failed to find expected lines in {path}:\n{}",
                    chunk.old.join("\n")
                ),
            )
        })?;
        // Replacement EOLs are decided before splicing: positionally replaced
        // lines keep their source ending, extra inserted lines use the last
        // removed line's ending (then the preceding line's, then the file
        // majority).
        let removed_eols: Vec<bool> = lines[at..at + pattern.len()]
            .iter()
            .map(|(_, is_crlf)| *is_crlf)
            .collect();
        let fallback_eol = removed_eols
            .last()
            .copied()
            .or_else(|| lines.get(at.wrapping_sub(1)).map(|(_, is_crlf)| *is_crlf))
            .or_else(|| lines.get(at).map(|(_, is_crlf)| *is_crlf))
            .unwrap_or_else(|| dominant_crlf(&lines));

        // Context lines keep their exact source text *and* original EOL. The
        // parser records their identity explicitly; inferring context from
        // equal old/new text is wrong after deletions and with duplicates.
        let replacement: Vec<(String, bool)> = replacement_source
            .iter()
            .enumerate()
            .map(|(offset, content)| {
                if let Some((old_index, _)) =
                    chunk
                        .context_line_indices
                        .iter()
                        .find(|(old_index, new_index)| {
                            *new_index == offset
                                && *old_index < pattern.len()
                                && *new_index < replacement_source.len()
                        })
                {
                    lines
                        .get(at + old_index)
                        .map(|(source, source_eol)| (source.clone(), *source_eol))
                        .unwrap_or_else(|| (content.clone(), fallback_eol))
                } else {
                    (
                        content.clone(),
                        removed_eols.get(offset).copied().unwrap_or(fallback_eol),
                    )
                }
            })
            .collect();
        lines.splice(at..at + pattern.len(), replacement);
        cursor = at + replacement_source.len();
    }

    let mut output = String::with_capacity(original.len());
    for (index, (content, is_crlf)) in lines.iter().enumerate() {
        output.push_str(content);
        if index + 1 < lines.len() || trailing_newline {
            output.push_str(eol(*is_crlf));
        }
    }
    Ok(output)
}

impl AgentHandler {
    pub(super) async fn apply_codex_patch(
        &self,
        context: ProjectRequestContext,
        input: String,
    ) -> Result<CallToolResult, ErrorData> {
        if input.len() > self.shared.config.limits.patch_bytes {
            return Ok(super::error_result(&AppError::new(
                "INPUT_TOO_LARGE",
                "patch exceeds MAX_PATCH_BYTES",
            )));
        }
        let actions = match parse(&input) {
            Ok(actions) => actions,
            Err(error) => return Ok(super::error_result(&error)),
        };
        if actions.len() > self.shared.config.limits.multi_path_count {
            return Ok(super::error_result(&AppError::new(
                "RESOURCE_LIMIT_EXCEEDED",
                "too many patch targets",
            )));
        }
        let shared = self.shared.clone();
        let params = json!({"input": input});
        self.run(context.0, "apply_patch", params, move |project| async move {
            let _patch = shared.permit(shared.patches.clone()).await?;
            let _cpu = shared.permit(shared.cpu.clone()).await?;
            let (_, _, project_patch) = shared
                .project_permits
                .get(project.effective_project_key.as_str())?;
            let _project_patch = shared.permit(project_patch).await?;
            let mut instruction_notices = Vec::new();
            for action in &actions {
                let (path, destination) = match action {
                    Action::Add { path, .. } | Action::Delete { path } => (path.as_str(), None),
                    Action::Update { path, destination, .. } => {
                        (path.as_str(), destination.as_deref())
                    }
                };
                if let Some(notice) = shared.scoped_instruction_notice(&project, path)? {
                    instruction_notices.push(notice);
                }
                if let Some(destination) = destination
                    && let Some(notice) =
                        shared.scoped_instruction_notice(&project, destination)?
                {
                    instruction_notices.push(notice);
                }
            }
            if !instruction_notices.is_empty() {
                return Err(AppError::new(
                    "AGENTS_SCOPE_REQUIRED",
                    format!(
                        "nested project instructions were disclosed before this mutation; consume them and retry the same patch if it still complies:\n\n{}",
                        instruction_notices.join("\n")
                    ),
                ));
            }
            let mut updates = Vec::new();
            let mut results = Vec::new();
            let mut targets = std::collections::HashSet::new();
            for action in actions {
                match action {
                    Action::Add { path, content } => {
                        if !targets.insert(path.clone()) { return Err(AppError::new("INVALID_PATCH", "duplicate patch target")); }
                        let old = match shared.paths.read_file_bounded(&project.project_root, &path, shared.config.limits.write_bytes) {
                            Ok(_) => return Err(AppError::new("INVALID_PATCH", format!("Add File already exists: {path}"))),
                            Err(error) if error.code() == "FILE_NOT_FOUND" => None,
                            Err(error) => return Err(error),
                        };
                        updates.push(PatchUpdate { path: path.clone(), old, new: Some(content.into_bytes()) });
                        results.push(json!({"path":path,"operation":"create"}));
                    }
                    Action::Delete { path } => {
                        if !targets.insert(path.clone()) { return Err(AppError::new("INVALID_PATCH", "duplicate patch target")); }
                        let old = Some(shared.paths.read_file_bounded(&project.project_root, &path, shared.config.limits.write_bytes)?);
                        updates.push(PatchUpdate { path: path.clone(), old, new: None });
                        results.push(json!({"path":path,"operation":"delete"}));
                    }
                    Action::Update { path, destination, chunks } => {
                        if !targets.insert(path.clone()) { return Err(AppError::new("INVALID_PATCH", "duplicate patch target")); }
                        let old = shared.paths.read_file_bounded(&project.project_root, &path, shared.config.limits.write_bytes)?;
                        let original = String::from_utf8(old.clone()).map_err(|error| AppError::new("INVALID_PATCH", format!("target is not UTF-8: {error}")))?;
                        let updated = apply(&original, &chunks, &path)?.into_bytes();
                        if updated.len() > shared.config.limits.write_bytes { return Err(AppError::new("RESOURCE_LIMIT_EXCEEDED", "patched file exceeds MAX_WRITE_BYTES")); }
                        if let Some(destination) = destination {
                            if !targets.insert(destination.clone()) { return Err(AppError::new("INVALID_PATCH", "duplicate move destination")); }
                            match shared.paths.read_file_bounded(&project.project_root, &destination, shared.config.limits.write_bytes) {
                                Ok(_) => return Err(AppError::new("INVALID_PATCH", format!("Move destination already exists: {destination}"))),
                                Err(error) if error.code() == "FILE_NOT_FOUND" => {}
                                Err(error) => return Err(error),
                            }
                            updates.push(PatchUpdate { path: destination.clone(), old: None, new: Some(updated) });
                            updates.push(PatchUpdate { path: path.clone(), old: Some(old), new: None });
                            results.push(json!({"path":path,"destination":destination,"operation":"move"}));
                        } else {
                            updates.push(PatchUpdate { path: path.clone(), old: Some(old), new: Some(updated) });
                            results.push(json!({"path":path,"operation":"update"}));
                        }
                    }
                }
            }
            let paths = shared.paths.clone();
            let root = project.project_root.clone();
            tokio::task::spawn_blocking(move || capability_patch_transaction(&paths, &root, &updates)).await.map_err(|error| AppError::new("PROCESS_FAILED", error.to_string()))??;
            Ok(json!({"files":results,"count":results.len(),"applied":true,"format":"codex","transaction":"all_or_rollback"}))
        }).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_and_applies_multi_file_codex_patch() {
        let actions = parse("*** Begin Patch\n*** Add File: a.txt\n+hello\n*** Update File: b.txt\n@@\n-old\n+new\n*** Delete File: c.txt\n*** End Patch").unwrap();
        assert_eq!(actions.len(), 3);
        let Action::Update { chunks, .. } = &actions[1] else {
            panic!()
        };
        assert_eq!(apply("old\n", chunks, "b.txt").unwrap(), "new\n");
    }

    #[test]
    fn supports_move_and_crlf() {
        let actions = parse("*** Begin Patch\n*** Update File: a.txt\n*** Move to: b.txt\n@@\n-one\n+two\n*** End Patch").unwrap();
        let Action::Update {
            destination,
            chunks,
            ..
        } = &actions[0]
        else {
            panic!()
        };
        assert_eq!(destination.as_deref(), Some("b.txt"));
        assert_eq!(apply("one\r\n", chunks, "a.txt").unwrap(), "two\r\n");
    }

    #[test]
    fn update_matching_falls_back_to_trimmed_whitespace() {
        let actions =
            parse("*** Begin Patch\n*** Update File: a.txt\n@@\n-value\n+updated\n*** End Patch")
                .unwrap();
        let Action::Update { chunks, .. } = &actions[0] else {
            panic!()
        };
        assert_eq!(apply("value   \n", chunks, "a.txt").unwrap(), "updated\n");
    }

    #[test]
    fn update_matching_normalizes_smart_quotes_and_dashes() {
        let actions = parse(
            "*** Begin Patch\n*** Update File: a.txt\n@@\n-let x = \"a-b\";\n+let x = \"updated\";\n*** End Patch",
        )
        .unwrap();
        let Action::Update { chunks, .. } = &actions[0] else {
            panic!()
        };
        assert_eq!(
            apply("let x = “a–b”;\n", chunks, "a.txt").unwrap(),
            "let x = \"updated\";\n"
        );
    }

    #[test]
    fn eof_chunks_match_only_at_the_end() {
        let actions = parse(
            "*** Begin Patch\n*** Update File: a.txt\n@@\n-tail\n+done\n*** End of File\n*** End Patch",
        )
        .unwrap();
        let Action::Update { chunks, .. } = &actions[0] else {
            panic!()
        };
        assert_eq!(
            apply("tail\nmiddle\ntail\n", chunks, "a.txt").unwrap(),
            "tail\nmiddle\ndone\n"
        );
    }

    #[test]
    fn rejects_missing_envelope() {
        assert!(parse("*** Add File: a\n+x").is_err());
    }

    #[test]
    fn tolerates_blank_lines_around_patch_envelope() {
        let actions =
            parse("\n  \n*** Begin Patch\n*** Delete File: x\n*** End Patch\n\n").unwrap();
        assert_eq!(
            actions,
            vec![Action::Delete {
                path: "x".to_owned()
            }]
        );
    }

    #[test]
    fn parses_add_and_delete_hunks_exactly() {
        let actions = parse("*** Begin Patch\n*** Add File: a.txt\n+one\n+two\n*** Delete File: gone.txt\n*** End Patch").unwrap();
        assert_eq!(
            actions,
            vec![
                Action::Add {
                    path: "a.txt".to_owned(),
                    content: "one\ntwo\n".to_owned()
                },
                Action::Delete {
                    path: "gone.txt".to_owned()
                },
            ]
        );
    }

    #[test]
    fn parses_context_anchor_move_and_eof() {
        let actions = parse("*** Begin Patch\n*** Update File: src/old.rs\n*** Move to: src/new.rs\n@@ function main\n ctx\n-old\n+new\n*** End of File\n*** End Patch").unwrap();
        let Action::Update {
            path,
            destination,
            chunks,
        } = &actions[0]
        else {
            panic!()
        };
        assert_eq!(path, "src/old.rs");
        assert_eq!(destination.as_deref(), Some("src/new.rs"));
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].anchor.as_deref(), Some("function main"));
        assert_eq!(chunks[0].old, vec!["ctx", "old"]);
        assert_eq!(chunks[0].new, vec!["ctx", "new"]);
        assert!(chunks[0].eof);
    }

    #[test]
    fn parses_multiple_update_chunks_and_bare_empty_context() {
        let actions = parse("*** Begin Patch\n*** Update File: f.txt\n@@\n before\n\n after\n@@\n-old\n+new\n*** End Patch").unwrap();
        let Action::Update { chunks, .. } = &actions[0] else {
            panic!()
        };
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].old, vec!["before", "", "after"]);
        assert_eq!(chunks[0].new, vec!["before", "", "after"]);
        assert_eq!(chunks[1].old, vec!["old"]);
        assert_eq!(chunks[1].new, vec!["new"]);
    }

    #[test]
    fn codex_parity_first_update_chunk_does_not_require_explicit_at_at_header() {
        // OpenAI Codex parser.rs explicitly tests this strict-mode grammar:
        // the first update chunk may begin directly with diff lines.
        let actions =
            parse("*** Begin Patch\n*** Update File: file2.py\n import foo\n+bar\n*** End Patch")
                .expect("Codex-compatible grammar accepts a first chunk without @@");
        let Action::Update { chunks, .. } = &actions[0] else {
            panic!()
        };
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].old, vec!["import foo"]);
        assert_eq!(chunks[0].new, vec!["import foo", "bar"]);
    }

    #[test]
    fn parses_crlf_patch_text() {
        let actions =
            parse("*** Begin Patch\r\n*** Add File: a.txt\r\n+hi\r\n*** End Patch\r\n").unwrap();
        assert_eq!(
            actions[0],
            Action::Add {
                path: "a.txt".to_owned(),
                content: "hi\n".to_owned()
            }
        );
    }

    #[test]
    fn rejects_missing_end_marker_and_update_without_chunk() {
        assert!(parse("*** Begin Patch\n*** Add File: a.txt\n+hi").is_err());
        let error = parse("*** Begin Patch\n*** Update File: a.txt\n*** End Patch").unwrap_err();
        assert_eq!(error.code(), "INVALID_PATCH");
        assert!(error.message().contains("no chunks"));
    }

    #[test]
    fn rejects_unknown_update_line_prefix() {
        let error =
            parse("*** Begin Patch\n*** Update File: a.txt\n@@\n?nope\n*** End Patch").unwrap_err();
        assert_eq!(error.code(), "INVALID_PATCH");
        assert!(error.message().contains("space, +, or -"));
    }

    #[test]
    fn sequence_matching_covers_exact_trim_normalized_and_edge_cases() {
        fn pair(content: &str) -> (String, bool) {
            (content.to_owned(), false)
        }
        let exact = vec![pair("alpha"), pair("beta"), pair("gamma")];
        assert_eq!(
            find_sequence(&exact, &["beta".to_owned(), "gamma".to_owned()], 0, false),
            Some(1)
        );
        assert_eq!(
            find_sequence(&[pair(" foo   ")], &["foo".to_owned()], 0, false),
            Some(0)
        );
        assert_eq!(
            find_sequence(
                &[pair("say “hello” — now")],
                &["say \"hello\" - now".to_owned()],
                0,
                false
            ),
            Some(0)
        );
        assert_eq!(
            find_sequence(
                &[pair("only")],
                &["too".to_owned(), "many".to_owned()],
                0,
                false
            ),
            None
        );
        assert_eq!(find_sequence(&exact, &[], 2, false), Some(exact.len()));
        assert_eq!(
            find_sequence(
                &[pair("x"), pair("x"), pair("x")],
                &["x".to_owned()],
                0,
                true
            ),
            Some(2)
        );
    }

    #[test]
    fn apply_uses_context_and_appends_empty_old_chunk() {
        let actions =
            parse("*** Begin Patch\n*** Update File: f\n@@\n a\n-b\n+B\n@@\n+d\n*** End Patch")
                .unwrap();
        let Action::Update { chunks, .. } = &actions[0] else {
            panic!()
        };
        assert_eq!(
            apply("z\nb\na\nb\nc\n", chunks, "f").unwrap(),
            "z\nb\na\nB\nc\nd\n"
        );
    }

    #[test]
    fn audit_bare_insert_chunk_appends_at_tail_like_codex() {
        let actions = parse("*** Begin Patch\n*** Update File: f\n@@\n+X\n*** End Patch").unwrap();
        let Action::Update { chunks, .. } = &actions[0] else {
            panic!()
        };
        assert_eq!(apply("a\nb\n", chunks, "f").unwrap(), "a\nb\nX\n");
    }

    #[test]
    fn apply_reports_missing_anchor_and_expected_lines() {
        let missing_anchor =
            parse("*** Begin Patch\n*** Update File: f\n@@ nowhere\n-a\n+A\n*** End Patch")
                .unwrap();
        let Action::Update { chunks, .. } = &missing_anchor[0] else {
            panic!()
        };
        assert!(
            apply("a\nb\n", chunks, "f")
                .unwrap_err()
                .message()
                .contains("failed to find anchor")
        );

        let missing_old =
            parse("*** Begin Patch\n*** Update File: f\n@@\n-missing\n+new\n*** End Patch")
                .unwrap();
        let Action::Update { chunks, .. } = &missing_old[0] else {
            panic!()
        };
        assert!(
            apply("a\nb\n", chunks, "f")
                .unwrap_err()
                .message()
                .contains("failed to find expected lines")
        );
    }

    #[test]
    fn trailing_empty_context_line_retries_as_sentinel() {
        // A patch authored with a trailing blank context line (representing
        // the final newline) still applies when the file's last real line is
        // the match target.
        let actions =
            parse("*** Begin Patch\n*** Update File: f\n@@\n-b\n+B\n\n*** End Patch").unwrap();
        let Action::Update { chunks, .. } = &actions[0] else {
            panic!()
        };
        assert_eq!(apply("a\nb\nc\n", chunks, "f").unwrap(), "a\nB\nc\n");
    }

    #[test]
    fn lenient_match_keeps_original_context_bytes() {
        // The context line matches only after trimming; its original trailing
        // whitespace must survive in the output instead of taking the patch's
        // trimmed text.
        let actions = parse(
            "*** Begin Patch\n*** Update File: f\n@@\n keep me   \n-old\n+new\n*** End Patch",
        )
        .unwrap();
        let Action::Update { chunks, .. } = &actions[0] else {
            panic!()
        };
        assert_eq!(
            apply("keep me   \nold\n", chunks, "f").unwrap(),
            "keep me   \nnew\n"
        );
    }

    #[test]
    fn regression_context_after_deletion_keeps_original_lenient_bytes() {
        let actions =
            parse("*** Begin Patch\n*** Update File: f\n@@\n-remove\n keep\n*** End Patch")
                .unwrap();
        let Action::Update { chunks, .. } = &actions[0] else {
            panic!()
        };

        // `keep` is a context line, so a lenient trim-only match must preserve
        // the exact source bytes rather than replacing them with patch text.
        assert_eq!(
            apply("remove\nkeep   \n", chunks, "f").unwrap(),
            "keep   \n"
        );
    }

    #[test]
    fn regression_context_after_deletion_keeps_original_line_ending() {
        let actions =
            parse("*** Begin Patch\n*** Update File: f\n@@\n-remove\n keep\n*** End Patch")
                .unwrap();
        let Action::Update { chunks, .. } = &actions[0] else {
            panic!()
        };

        // The deleted line is CRLF while the untouched context line is LF.
        // Removing the first line must not transfer its CRLF terminator to the
        // surviving context line.
        assert_eq!(apply("remove\r\nkeep\n", chunks, "f").unwrap(), "keep\n");
    }

    #[tokio::test]
    async fn safety_delete_file_action_never_recursively_deletes_directory_target() {
        use std::{collections::BTreeMap, sync::Arc};

        use crate::{
            audit::AuditLogger,
            config::ConfigBuilder,
            project::{ProjectContext, ProjectKey, ProjectResolver},
            request_context::{ProjectRequestContext, TransportMode},
            storage::Storage,
            tools::SharedState,
            upstream::Aggregator,
        };

        let directory = tempfile::tempdir().unwrap();
        let workspace = directory.path().join("workspace");
        let project_root = workspace.join("project");
        let target = project_root.join("target-dir");
        std::fs::create_dir_all(target.join("nested")).unwrap();
        std::fs::write(target.join("nested/keep.txt"), b"keep").unwrap();

        let config = Arc::new(
            ConfigBuilder::from_map(BTreeMap::from([
                ("MCP_AUTH_TOKEN".to_owned(), "1234567890abcdef".to_owned()),
                ("WORKSPACE_ROOT".to_owned(), workspace.display().to_string()),
            ]))
            .build()
            .unwrap(),
        );
        let storage = Storage::open(&directory.path().join("state.sqlite3")).unwrap();
        let resolver = ProjectResolver::new(workspace, storage.clone()).unwrap();
        let audit = AuditLogger::new(config.logs.clone(), config.auth_token.clone())
            .await
            .unwrap();
        let handler = AgentHandler::new(SharedState::new(
            config,
            resolver,
            storage,
            audit,
            Aggregator::default(),
        ));
        let project = ProjectContext {
            native_project_key: ProjectKey::new("native".to_owned()).unwrap(),
            effective_project_key: ProjectKey::new("effective".to_owned()).unwrap(),
            project_alias: None,
            project_root,
            metadata_root: directory.path().join("metadata"),
            transport_mode: TransportMode::Stateless,
            mcp_session_present: false,
        };

        let response = handler
            .apply_codex_patch(
                ProjectRequestContext(Ok(project)),
                "*** Begin Patch\n*** Delete File: target-dir\n*** End Patch".to_owned(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.is_error,
            Some(true),
            "Delete File must reject a directory target"
        );
        assert!(
            target.is_dir(),
            "patch recursively removed the directory target"
        );
        assert_eq!(
            std::fs::read(target.join("nested/keep.txt")).unwrap(),
            b"keep"
        );
    }

    #[test]
    fn apply_preserves_absence_of_trailing_newline() {
        let actions =
            parse("*** Begin Patch\n*** Update File: f\n@@\n-b\n+B\n*** End Patch").unwrap();
        let Action::Update { chunks, .. } = &actions[0] else {
            panic!()
        };
        assert_eq!(apply("a\nb", chunks, "f").unwrap(), "a\nB");
    }

    #[test]
    fn crlf_file_keeps_crlf_and_lf_lines_intact() {
        let actions =
            parse("*** Begin Patch\n*** Update File: f\n@@\n-second\n+replaced2\n*** End Patch")
                .unwrap();
        let Action::Update { chunks, .. } = &actions[0] else {
            panic!()
        };
        // Dominantly CRLF file with one stray LF line: the patched line keeps
        // its CRLF, untouched lines keep their original endings.
        assert_eq!(
            apply("first\r\nsecond\r\nthird\r\nfourth\n", chunks, "f").unwrap(),
            "first\r\nreplaced2\r\nthird\r\nfourth\n"
        );
    }

    #[test]
    fn lf_patch_applies_to_crlf_file_and_inserted_lines_use_crlf() {
        let actions =
            parse("*** Begin Patch\n*** Update File: f\n@@\n-two\n+two\n+inserted\n*** End Patch")
                .unwrap();
        let Action::Update { chunks, .. } = &actions[0] else {
            panic!()
        };
        assert_eq!(
            apply("one\r\ntwo\r\nthree\r\n", chunks, "f").unwrap(),
            "one\r\ntwo\r\ninserted\r\nthree\r\n"
        );
    }

    #[test]
    fn mixed_eol_chunk_preserves_per_line_endings() {
        let actions =
            parse("*** Begin Patch\n*** Update File: f\n@@\n-a\n-b\n+A\n+B\n*** End Patch")
                .unwrap();
        let Action::Update { chunks, .. } = &actions[0] else {
            panic!()
        };
        // Each replaced line keeps the ending of the line it replaces.
        assert_eq!(apply("a\r\nb\n", chunks, "f").unwrap(), "A\r\nB\n");
    }

    #[test]
    fn pure_insertion_uses_replaced_line_ending() {
        let actions = parse(
            "*** Begin Patch\n*** Update File: f\n@@\n-tail\n+tail\n+appended\n*** End of File\n*** End Patch",
        )
        .unwrap();
        let Action::Update { chunks, .. } = &actions[0] else {
            panic!()
        };
        // Without a trailing newline the inserted line still gets one so the
        // file stays newline-terminated; without trailing_newline the last
        // inserted line stays open.
        assert_eq!(
            apply("x\r\ntail", chunks, "f").unwrap(),
            "x\r\ntail\nappended"
        );
        assert_eq!(
            apply("x\r\ntail\r\n", chunks, "f").unwrap(),
            "x\r\ntail\r\nappended\r\n"
        );
    }

    #[test]
    fn empty_old_chunk_inherits_region_ending() {
        let actions =
            parse("*** Begin Patch\n*** Update File: f\n@@\n+a\n+b\n*** End Patch").unwrap();
        let Action::Update { chunks, .. } = &actions[0] else {
            panic!()
        };
        // Empty old matches at end-of-file (at = lines.len()); the fallback
        // is the preceding line's ending. This holds for any file: an
        // empty-old chunk always APPENDS, never prepends.
        assert_eq!(apply("x\r\n", chunks, "f").unwrap(), "x\r\na\r\nb\r\n");
        let head = parse("*** Begin Patch\n*** Update File: f\n@@\n+head\n*** End Patch").unwrap();
        let Action::Update {
            chunks: head_chunks,
            ..
        } = &head[0]
        else {
            panic!()
        };
        assert_eq!(apply("x\r\n", head_chunks, "f").unwrap(), "x\r\nhead\r\n");
    }

    #[test]
    fn repeated_identical_lines_require_anchor_or_match_first() {
        // Without an anchor, a chunk matches the FIRST occurrence from the
        // cursor; agents must anchor to target later duplicates.
        let actions =
            parse("*** Begin Patch\n*** Update File: f\n@@\n-dup\n+FIRST\n*** End Patch").unwrap();
        let Action::Update { chunks, .. } = &actions[0] else {
            panic!()
        };
        assert_eq!(
            apply("dup\nmid\ndup\n", chunks, "f").unwrap(),
            "FIRST\nmid\ndup\n"
        );
    }
    #[test]
    fn anchored_chunk_targets_second_occurrence() {
        // The anchor moves the cursor past the first `dup`; the context line
        // `mid` then pins the match to the second occurrence.
        let actions = parse(
            "*** Begin Patch\n*** Update File: f\n@@ mid\n mid\n-dup\n+SECOND\n*** End Patch",
        )
        .unwrap();
        let Action::Update { chunks, .. } = &actions[0] else {
            panic!()
        };
        assert_eq!(
            apply("dup\nmid\ndup\n", chunks, "f").unwrap(),
            "dup\nmid\nSECOND\n"
        );
    }

    #[test]
    fn second_chunk_continues_after_first_replacement() {
        let actions =
            parse("*** Begin Patch\n*** Update File: f\n@@\n-a\n+A1\n@@\n-b\n+B2\n*** End Patch")
                .unwrap();
        let Action::Update { chunks, .. } = &actions[0] else {
            panic!()
        };
        assert_eq!(apply("a\nb\n", chunks, "f").unwrap(), "A1\nB2\n");
    }

    #[test]
    fn crlf_file_with_lf_authored_context_and_eof_chunk() {
        // The EOF flag restricts matching to the file tail; the LF-authored
        // patch keeps every CRLF ending intact.
        let actions = parse(
            "*** Begin Patch\n*** Update File: f\n@@\n-HDR\n+HEADER\n*** End of File\n*** End Patch",
        )
        .unwrap();
        let Action::Update { chunks, .. } = &actions[0] else {
            panic!()
        };
        // Positive: HDR at file tail is replaced in place.
        assert_eq!(
            apply("header\r\nbody\r\nHDR\r\n", chunks, "f").unwrap(),
            "header\r\nbody\r\nHEADER\r\n"
        );
        // Negative: the same chunk must REFUSE to match when HDR sits before
        // other lines — EOF pins matching to the last window only.
        let mid = apply("header\r\nHDR\r\nbody\r\n", chunks, "f");
        assert!(mid.is_err(), "EOF chunk must not match a non-tail line");
    }

    #[test]
    fn deletion_of_last_line_keeps_prior_line_ending_discipline() {
        // A chunk that replaces two lines with one keeps line count coherent
        // and the trailing-newline state of the original file.
        let actions =
            parse("*** Begin Patch\n*** Update File: f\n@@\n-x\n-y\n+z\n*** End Patch").unwrap();
        let Action::Update { chunks, .. } = &actions[0] else {
            panic!()
        };
        assert_eq!(apply("x\r\ny\r\n", chunks, "f").unwrap(), "z\r\n");
        assert_eq!(apply("x\r\ny", chunks, "f").unwrap(), "z");
    }

    #[test]
    fn context_only_noop_chunk_is_byte_stable() {
        let actions =
            parse("*** Begin Patch\n*** Update File: f\n@@\n keep\n keep2\n*** End Patch").unwrap();
        let Action::Update { chunks, .. } = &actions[0] else {
            panic!()
        };
        assert_eq!(
            apply("keep\r\nkeep2\n", chunks, "f").unwrap(),
            "keep\r\nkeep2\n"
        );
    }

    #[test]
    fn empty_file_add_via_update_with_empty_old_appends() {
        let actions =
            parse("*** Begin Patch\n*** Update File: f\n@@\n+seeded\n*** End Patch").unwrap();
        let Action::Update { chunks, .. } = &actions[0] else {
            panic!()
        };
        // Empty file: old=[] matches at end; appending yields exactly the
        // inserted content — no phantom leading blank line.
        assert_eq!(apply("", chunks, "f").unwrap(), "seeded");
    }
}
