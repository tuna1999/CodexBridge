use std::{cmp::Ordering, fs, path::PathBuf};

#[derive(Debug)]
pub(super) struct PluginSkillCandidate {
    pub(super) plugin: String,
    pub(super) root: PathBuf,
    pub(super) path: PathBuf,
    pub(super) source: String,
}

fn safe_directories(root: &std::path::Path, maximum: usize) -> Vec<fs::DirEntry> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut values = entries
        .filter_map(Result::ok)
        .filter(|entry| fs::metadata(entry.path()).is_ok_and(|kind| kind.is_dir()))
        .collect::<Vec<_>>();
    values.sort_by_key(|entry| entry.file_name().to_string_lossy().into_owned());
    values.truncate(maximum);
    values
}

fn version_parts(value: &str) -> Vec<VersionPart<'_>> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let bytes = value.as_bytes();
    while start < bytes.len() {
        let numeric = bytes[start].is_ascii_digit();
        let mut end = start + 1;
        while end < bytes.len() && bytes[end].is_ascii_digit() == numeric {
            end += 1;
        }
        let piece = &value[start..end];
        if numeric {
            parts.push(VersionPart::Number(
                piece.parse::<u64>().unwrap_or(u64::MAX),
            ));
        } else {
            parts.push(VersionPart::Text(piece));
        }
        start = end;
    }
    parts
}

#[derive(Debug, Clone, Copy)]
enum VersionPart<'a> {
    Number(u64),
    Text(&'a str),
}

fn compare_versions(left: &str, right: &str) -> Ordering {
    let left = version_parts(left);
    let right = version_parts(right);
    for (a, b) in left.iter().zip(right.iter()) {
        let ordering = match (a, b) {
            (VersionPart::Number(a), VersionPart::Number(b)) => a.cmp(b),
            (VersionPart::Text(a), VersionPart::Text(b)) => a.cmp(b),
            (VersionPart::Number(_), VersionPart::Text(_)) => Ordering::Greater,
            (VersionPart::Text(_), VersionPart::Number(_)) => Ordering::Less,
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    left.len().cmp(&right.len())
}

pub(super) fn discover_plugin_skills(roots: &[PathBuf]) -> Vec<PluginSkillCandidate> {
    const LEVEL_LIMIT: usize = 128;
    const TOTAL_LIMIT: usize = 256;

    let mut candidates = Vec::new();
    for (root_index, root) in roots.iter().enumerate() {
        for marketplace in safe_directories(root, LEVEL_LIMIT) {
            let marketplace_name = marketplace.file_name().to_string_lossy().into_owned();
            for plugin in safe_directories(&marketplace.path(), LEVEL_LIMIT) {
                let plugin_name = plugin.file_name().to_string_lossy().into_owned();
                let mut versions = safe_directories(&plugin.path(), LEVEL_LIMIT);
                versions.sort_by(|left, right| {
                    compare_versions(
                        &left.file_name().to_string_lossy(),
                        &right.file_name().to_string_lossy(),
                    )
                });
                let Some(version) = versions.pop() else {
                    continue;
                };
                let version_name = version.file_name().to_string_lossy().into_owned();
                let skills_root = version.path().join("skills");
                for package in safe_directories(&skills_root, LEVEL_LIMIT) {
                    let skill_path = package.path().join("SKILL.md");
                    let Ok(metadata) = fs::metadata(&skill_path) else {
                        continue;
                    };
                    if !metadata.is_file() {
                        continue;
                    }
                    candidates.push(PluginSkillCandidate {
                        plugin: plugin_name.clone(),
                        root: skills_root.clone(),
                        path: skill_path,
                        source: format!(
                            "home-plugin-root-{root_index}/{marketplace_name}/{plugin_name}/{version_name}"
                        ),
                    });
                    if candidates.len() == TOTAL_LIMIT {
                        return candidates;
                    }
                }
            }
        }
    }
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_version_order_prefers_ten_over_two() {
        assert_eq!(compare_versions("1.10.0", "1.2.0"), Ordering::Greater);
        assert_eq!(compare_versions("2.0.0", "10.0.0"), Ordering::Less);
    }
}
