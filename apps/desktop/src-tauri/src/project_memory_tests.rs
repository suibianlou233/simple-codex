use super::*;

#[test]
fn memory_location_is_project_and_history_scoped_and_does_not_create_files() {
    let fixture = tempfile::tempdir().expect("fixture");
    let a = fixture.path().join("项目 A");
    let b = fixture.path().join("项目 B");
    std::fs::create_dir(&a).expect("project");
    std::fs::create_dir(&b).expect("project");
    let managed = fixture.path().join("kernel");
    let first = ProjectMemoryLocation::resolve(&managed, &"a".repeat(64), &a).expect("location");
    let reopened = ProjectMemoryLocation::resolve(&managed, &"a".repeat(64), &a.join("."))
        .expect("same project");
    let other =
        ProjectMemoryLocation::resolve(&managed, &"a".repeat(64), &b).expect("other project");
    let legacy =
        ProjectMemoryLocation::resolve(&managed, &"b".repeat(64), &a).expect("old history");
    assert_eq!(first.cache_key, reopened.cache_key);
    assert_ne!(first.cache_key, other.cache_key);
    assert_ne!(first.cache_key, legacy.cache_key);
    assert!(!managed.exists());
    assert!(ProjectMemoryLocation::resolve(&managed, "../escape", &a).is_err());
    std::fs::write(&managed, "not a directory").expect("fixture file");
    assert!(ProjectMemoryLocation::resolve(&managed, &"a".repeat(64), &a).is_err());
}

#[cfg(windows)]
#[test]
fn memory_location_rejects_a_windows_junction() {
    let fixture = tempfile::tempdir().expect("fixture");
    let project = fixture.path().join("project");
    let target = fixture.path().join("target");
    let alias = fixture.path().join("alias");
    std::fs::create_dir(&project).expect("project");
    std::fs::create_dir(&target).expect("target");
    let status = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&alias)
        .arg(&target)
        .output()
        .expect("junction command");
    assert!(status.status.success(), "junction fixture failed");
    assert!(matches!(
        ProjectMemoryLocation::resolve(&alias, &"a".repeat(64), &project),
        Err(DesktopError::UnsafeMemoryPath)
    ));
    assert_eq!(std::fs::read_dir(&target).expect("target").count(), 0);
}
