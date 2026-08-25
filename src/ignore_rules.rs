use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use ignore::gitignore::{Gitignore, GitignoreBuilder};

const DEFAULT_IGNORE: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    ".next",
    ".nuxt",
    ".svelte-kit",
    ".turbo",
    "coverage",
    "__pycache__",
    ".venv",
    "venv",
    ".cache",
];

/// One immutable matcher per tool call. Rebuilding is intentional: changes to
/// `.gitignore` become visible without retaining an unbounded project cache.
pub struct IgnoreMatcher {
    root: PathBuf,
    root_matcher: Gitignore,
    nested: Mutex<HashMap<PathBuf, Arc<Gitignore>>>,
}

impl IgnoreMatcher {
    pub fn for_project(root: &Path) -> Self {
        let mut builder = GitignoreBuilder::new(root);
        for pattern in DEFAULT_IGNORE {
            let _ = builder.add_line(None, pattern);
        }
        // Git gives per-directory .gitignore files higher precedence than
        // repository-local .git/info/exclude. GitignoreBuilder resolves later
        // matching rules last, so add info/exclude before the root .gitignore.
        let _ = builder.add(root.join(".git").join("info").join("exclude"));
        let _ = builder.add(root.join(".gitignore"));
        Self {
            root: root.to_path_buf(),
            root_matcher: builder.build().unwrap_or_else(|_| Gitignore::empty()),
            nested: Mutex::new(HashMap::new()),
        }
    }

    fn matcher_for_directory(&self, directory: &Path) -> Arc<Gitignore> {
        if let Ok(cache) = self.nested.lock()
            && let Some(existing) = cache.get(directory)
        {
            return existing.clone();
        }
        let mut builder = GitignoreBuilder::new(directory);
        let _ = builder.add(directory.join(".gitignore"));
        let matcher = Arc::new(builder.build().unwrap_or_else(|_| Gitignore::empty()));
        if let Ok(mut cache) = self.nested.lock() {
            cache.insert(directory.to_path_buf(), matcher.clone());
        }
        matcher
    }

    pub fn is_ignored(&self, path: &Path, is_dir: bool) -> bool {
        let Ok(relative) = path.strip_prefix(&self.root) else {
            return true;
        };
        if relative.as_os_str().is_empty() {
            return false;
        }
        let mut ignored = self
            .root_matcher
            .matched_path_or_any_parents(relative, is_dir)
            .is_ignore();

        // Gitignore files are scoped to the directory containing them. Walk the
        // ancestors in project-root-to-target order. A nested .gitignore is only
        // reachable if every ancestor directory remains visible under the rules
        // loaded above it; Git cannot re-include a file from inside an excluded
        // directory because traversal never reaches that directory's rules.
        let Some(parent) = path.parent() else {
            return ignored;
        };
        let Ok(parent_relative) = parent.strip_prefix(&self.root) else {
            return true;
        };
        let mut directory = self.root.clone();
        let mut active_nested: Vec<Arc<Gitignore>> = Vec::new();
        for component in parent_relative.components() {
            directory.push(component.as_os_str());

            let Ok(directory_relative) = directory.strip_prefix(&self.root) else {
                return true;
            };
            let mut directory_ignored = self
                .root_matcher
                .matched_path_or_any_parents(directory_relative, true)
                .is_ignore();
            for matcher in &active_nested {
                let matched = matcher.matched_path_or_any_parents(&directory, true);
                if matched.is_ignore() {
                    directory_ignored = true;
                } else if matched.is_whitelist() {
                    directory_ignored = false;
                }
            }
            if directory_ignored {
                return true;
            }

            let matcher = self.matcher_for_directory(&directory);
            let matched = matcher.matched_path_or_any_parents(path, is_dir);
            if matched.is_ignore() {
                ignored = true;
            } else if matched.is_whitelist() {
                ignored = false;
            }
            active_nested.push(matcher);
        }
        ignored
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_gitignore_are_consistent() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join(".gitignore"), "generated/\n*.secret\n").unwrap();
        let matcher = IgnoreMatcher::for_project(temp.path());
        assert!(matcher.is_ignored(&temp.path().join(".git/config"), false));
        assert!(matcher.is_ignored(&temp.path().join("target/debug/app"), false));
        assert!(matcher.is_ignored(&temp.path().join("generated/a.txt"), false));
        assert!(matcher.is_ignored(&temp.path().join("key.secret"), false));
        assert!(!matcher.is_ignored(&temp.path().join("src/main.rs"), false));
    }

    #[test]
    fn every_default_pattern_hides_directory_and_descendants() {
        let temp = tempfile::tempdir().unwrap();
        let matcher = IgnoreMatcher::for_project(temp.path());
        for name in DEFAULT_IGNORE {
            assert!(matcher.is_ignored(&temp.path().join(name), true), "{name}");
            assert!(
                matcher.is_ignored(&temp.path().join(name).join("deep/file.js"), false),
                "{name}"
            );
        }
        assert!(!matcher.is_ignored(&temp.path().join("src/index.rs"), false));
    }

    #[test]
    fn git_info_exclude_is_honored() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(".git/info")).unwrap();
        std::fs::write(temp.path().join(".git/info/exclude"), "local-only.tmp\n").unwrap();
        let matcher = IgnoreMatcher::for_project(temp.path());
        assert!(matcher.is_ignored(&temp.path().join("local-only.tmp"), false));
        assert!(!matcher.is_ignored(&temp.path().join("tracked.tmp"), false));
    }

    #[test]
    fn nested_gitignore_is_scoped_to_its_directory() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("sub")).unwrap();
        std::fs::write(temp.path().join("sub/.gitignore"), "hidden.txt\n").unwrap();
        let matcher = IgnoreMatcher::for_project(temp.path());
        assert!(matcher.is_ignored(&temp.path().join("sub/hidden.txt"), false));
        assert!(!matcher.is_ignored(&temp.path().join("sub/visible.txt"), false));
        assert!(!matcher.is_ignored(&temp.path().join("hidden.txt"), false));
    }

    #[test]
    fn project_root_is_visible_but_outside_paths_fail_closed() {
        let temp = tempfile::tempdir().unwrap();
        let matcher = IgnoreMatcher::for_project(temp.path());
        assert!(!matcher.is_ignored(temp.path(), true));
        let outside = temp.path().parent().unwrap().join("outside-codexbridge");
        assert!(matcher.is_ignored(&outside, true));
    }
}
