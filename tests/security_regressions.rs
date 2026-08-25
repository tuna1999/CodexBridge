use codex_bridge::sandbox::{PathOperation, SecurePathResolver};

#[test]
fn normal_filesystem_paths_reject_parent_traversal() {
    let temp = tempfile::tempdir().unwrap();
    let resolver = SecurePathResolver;
    let error = resolver
        .resolve_project_path(temp.path(), "../escape.txt", PathOperation::Create)
        .unwrap_err();
    assert_eq!(error.code(), "PATH_OUTSIDE_WORKSPACE");
}

#[cfg(unix)]
#[test]
fn normal_filesystem_paths_reject_symlink_components() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.txt"), "secret").unwrap();
    symlink(outside.path(), temp.path().join("linked")).unwrap();

    let resolver = SecurePathResolver;
    let error = resolver
        .resolve_project_path(temp.path(), "linked/secret.txt", PathOperation::Existing)
        .unwrap_err();
    assert_eq!(error.code(), "SYMLINK_ESCAPE");
}

#[cfg(unix)]
#[test]
fn capability_reads_reject_a_symlink_as_the_final_component() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.txt"), "secret").unwrap();
    symlink(
        outside.path().join("secret.txt"),
        temp.path().join("linked.txt"),
    )
    .unwrap();

    let resolver = SecurePathResolver;
    let direct = resolver
        .read_file_bounded(temp.path(), "linked.txt", 1024)
        .unwrap_err();
    assert_eq!(direct.code(), "SYMLINK_ESCAPE");
    let ranged = resolver
        .read_file_range(temp.path(), "linked.txt", 0, 16)
        .unwrap_err();
    assert_eq!(ranged.code(), "SYMLINK_ESCAPE");
}

#[cfg(unix)]
#[test]
fn capability_writes_reject_symlinked_parent_directories() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), temp.path().join("linked")).unwrap();

    let resolver = SecurePathResolver;
    let error = resolver
        .write_file_atomic(temp.path(), "linked/escape.txt", b"nope")
        .unwrap_err();
    assert_eq!(error.code(), "SYMLINK_ESCAPE");
    assert!(!outside.path().join("escape.txt").exists());
}

#[test]
fn capability_copy_and_move_reject_parent_traversal() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("source.txt"), "source").unwrap();
    let resolver = SecurePathResolver;

    assert!(
        resolver
            .copy_file_secure(temp.path(), "source.txt", "../copy.txt", 1024)
            .is_err()
    );
    assert!(
        resolver
            .move_path_secure(temp.path(), "source.txt", "../move.txt")
            .is_err()
    );
    assert!(temp.path().join("source.txt").is_file());
}
