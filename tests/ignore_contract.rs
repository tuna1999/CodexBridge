use codex_bridge::ignore_rules::IgnoreMatcher;

#[test]
fn root_gitignore_whitelist_overrides_broader_root_pattern() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join(".gitignore"), "*.log\n!important.log\n").unwrap();
    let matcher = IgnoreMatcher::for_project(temp.path());
    assert!(matcher.is_ignored(&temp.path().join("debug.log"), false));
    assert!(!matcher.is_ignored(&temp.path().join("important.log"), false));
}

#[test]
fn nested_gitignore_whitelist_overrides_broader_root_file_pattern() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("sub")).unwrap();
    std::fs::write(temp.path().join(".gitignore"), "*.tmp\n").unwrap();
    std::fs::write(temp.path().join("sub/.gitignore"), "!keep.tmp\n").unwrap();
    let matcher = IgnoreMatcher::for_project(temp.path());
    assert!(matcher.is_ignored(&temp.path().join("drop.tmp"), false));
    assert!(matcher.is_ignored(&temp.path().join("sub/drop.tmp"), false));
    assert!(!matcher.is_ignored(&temp.path().join("sub/keep.tmp"), false));
}

#[test]
fn nested_rules_do_not_leak_to_sibling_directories() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("left")).unwrap();
    std::fs::create_dir_all(temp.path().join("right")).unwrap();
    std::fs::write(temp.path().join("left/.gitignore"), "local.txt\n").unwrap();
    let matcher = IgnoreMatcher::for_project(temp.path());
    assert!(matcher.is_ignored(&temp.path().join("left/local.txt"), false));
    assert!(!matcher.is_ignored(&temp.path().join("right/local.txt"), false));
}

#[test]
fn anchored_root_pattern_only_matches_at_project_root() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("nested")).unwrap();
    std::fs::write(temp.path().join(".gitignore"), "/root-only.txt\n").unwrap();
    let matcher = IgnoreMatcher::for_project(temp.path());
    assert!(matcher.is_ignored(&temp.path().join("root-only.txt"), false));
    assert!(!matcher.is_ignored(&temp.path().join("nested/root-only.txt"), false));
}

#[test]
fn directory_pattern_hides_directory_and_descendants_but_not_similar_name() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join(".gitignore"), "generated/\n").unwrap();
    let matcher = IgnoreMatcher::for_project(temp.path());
    assert!(matcher.is_ignored(&temp.path().join("generated"), true));
    assert!(matcher.is_ignored(&temp.path().join("generated/deep/file.rs"), false));
    assert!(!matcher.is_ignored(&temp.path().join("generated.txt"), false));
}

#[test]
fn root_gitignore_has_higher_precedence_than_git_info_exclude() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(".git/info")).unwrap();
    std::fs::write(temp.path().join(".git/info/exclude"), "local.tmp\n").unwrap();
    std::fs::write(temp.path().join(".gitignore"), "!local.tmp\n").unwrap();
    let matcher = IgnoreMatcher::for_project(temp.path());
    assert!(
        !matcher.is_ignored(&temp.path().join("local.tmp"), false),
        "per-repository .gitignore should outrank .git/info/exclude like Git"
    );
}

#[test]
fn ignored_parent_directory_cannot_be_reincluded_only_by_nested_gitignore() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("sub")).unwrap();
    std::fs::write(temp.path().join(".gitignore"), "sub/\n").unwrap();
    std::fs::write(temp.path().join("sub/.gitignore"), "!keep.txt\n").unwrap();
    let matcher = IgnoreMatcher::for_project(temp.path());
    assert!(matcher.is_ignored(&temp.path().join("sub"), true));
    assert!(
        matcher.is_ignored(&temp.path().join("sub/keep.txt"), false),
        "Git cannot re-include a file when an ancestor directory is still excluded"
    );
}
