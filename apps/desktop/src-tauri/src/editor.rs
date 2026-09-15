//! User-operated editor. Main-webview IPC only; mutations share the agent's project lease.
use super::*;
use std::path::Component;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry { path: String, name: String, directory: bool }

fn checked(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let mut path = fs::canonicalize(root).map_err(command_error)?;
    if relative.contains(':') || relative.contains('\0') { return Err("无效的项目路径".into()); }
    for part in Path::new(relative).components() {
        let Component::Normal(name) = part else { return Err("只允许项目内的相对路径".into()); };
        path.push(name);
        match fs::symlink_metadata(&path) {
            Ok(meta) => {
                #[cfg(windows)]
                { use std::os::windows::fs::MetadataExt;
                  if meta.file_attributes() & 0x400 != 0 { return Err("编辑器暂不支持链接目录或文件".into()); }
                }
                if meta.file_type().is_symlink() { return Err("编辑器暂不支持链接目录或文件".into()); }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(command_error(e)),
        }
    }
    Ok(path)
}

#[tauri::command]
pub fn editor_list(project_id: String, path: String, state: State<'_, DesktopState>) -> Result<Vec<Entry>, String> {
    let runtime = state.lock().map_err(command_error)?;
    let root = runtime.project_root(&project_id).map_err(command_error)?;
    let directory = checked(&root, &path)?;
    let mut entries = Vec::new();
    for item in fs::read_dir(directory).map_err(command_error)?.take(2001) {
        if entries.len() == 2000 { return Err("目录超过 2000 项，请缩小范围".into()); }
        let item = item.map_err(command_error)?;
        let name = item.file_name().to_string_lossy().into_owned();
        if name == ".git" { continue; }
        let relative = if path.is_empty() {name.clone()} else {format!("{path}/{name}")};
        if checked(&root, &relative).is_err() { continue; }
        entries.push(Entry {path:relative, name, directory:item.file_type().map_err(command_error)?.is_dir()});
    }
    entries.sort_by(|a,b| b.directory.cmp(&a.directory).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(entries)
}

#[tauri::command]
pub fn editor_search(project_id: String, query: String, state: State<'_, DesktopState>) -> Result<Vec<String>, String> {
    let runtime = state.lock().map_err(command_error)?;
    let root = runtime.project_root(&project_id).map_err(command_error)?;
    let workspace = Workspace::open(&root).map_err(command_error)?;
    let query = query.to_lowercase();
    Ok(workspace.list_files(10000).map_err(command_error)?.into_iter()
        .filter(|path| path.to_lowercase().contains(&query) && checked(&root,path).is_ok()).take(100).collect())
}

fn read(root: &Path, path: &str) -> Result<BackendInspectorFile, String> {
    checked(root,path)?;
    let file = Workspace::open(root).map_err(command_error)?.read_file(path).map_err(command_error)?;
    if file.content.contains('\0') { return Err("暂不支持编辑二进制文件".into()); }
    Ok(BackendInspectorFile {path:file.path, content:file.content, sha256:file.sha256, truncated:false})
}

#[tauri::command]
pub fn editor_read(project_id: String, path: String, state: State<'_, DesktopState>) -> Result<BackendInspectorFile, String> {
    let runtime = state.lock().map_err(command_error)?;
    read(&runtime.project_root(&project_id).map_err(command_error)?, &path)
}

fn save(root: &Path, path: &str, content: String, expected: &str) -> Result<BackendInspectorFile, String> {
    let target = checked(root,path)?;
    let permissions = fs::metadata(&target).map_err(command_error)?.permissions();
    let workspace = Workspace::open(root).map_err(command_error)?;
    let preview = workspace.preview_write(path,content,Some(expected)).map_err(command_error)?;
    workspace.apply_write(&preview).map_err(command_error)?;
    fs::set_permissions(target,permissions).map_err(command_error)?;
    read(root,path)
}

#[tauri::command]
pub fn editor_save(project_id: String, path: String, content: String, expected: String, state: State<'_, DesktopState>) -> Result<BackendInspectorFile, String> {
    let runtime = state.lock().map_err(command_error)?;
    let root = runtime.project_root(&project_id).map_err(command_error)?;
    let _lease = project_lease::ProjectLease::acquire(&root, runtime.database_path.parent().ok_or("数据目录不可用")?).map_err(command_error)?;
    save(&root,&path,content,&expected)
}

fn create(root: &Path, path: &str, directory: bool) -> Result<(), String> {
    if path.is_empty() { return Err("请输入名称".into()); }
    let target = checked(root,path)?;
    if directory { fs::create_dir(target).map_err(command_error) }
    else { fs::OpenOptions::new().write(true).create_new(true).open(target).map(|_|()).map_err(command_error) }
}

#[tauri::command]
pub fn editor_create(project_id: String, path: String, directory: bool, state: State<'_, DesktopState>) -> Result<(), String> {
    let runtime = state.lock().map_err(command_error)?;
    let root = runtime.project_root(&project_id).map_err(command_error)?;
    let _lease = project_lease::ProjectLease::acquire(&root, runtime.database_path.parent().ok_or("数据目录不可用")?).map_err(command_error)?;
    create(&root,&path,directory)
}

#[tauri::command]
pub fn editor_rename(project_id: String, path: String, destination: String, state: State<'_, DesktopState>) -> Result<(), String> {
    let runtime = state.lock().map_err(command_error)?;
    let root = runtime.project_root(&project_id).map_err(command_error)?;
    let _lease = project_lease::ProjectLease::acquire(&root, runtime.database_path.parent().ok_or("数据目录不可用")?).map_err(command_error)?;
    if path.is_empty() || destination.is_empty() { return Err("不能重命名项目根目录".into()); }
    let from = checked(&root,&path)?;
    let to = checked(&root,&destination)?;
    if to.exists() { return Err("目标已存在，请使用其他名称".into()); }
    fs::rename(from,to).map_err(command_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editor_rejects_stale_save_and_keeps_disk() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"),"before").unwrap();
        let first = read(dir.path(),"a.txt").unwrap();
        fs::write(dir.path().join("a.txt"),"AI changed").unwrap();
        assert!(save(dir.path(),"a.txt","my edit".into(),&first.sha256).is_err());
        assert_eq!(read(dir.path(),"a.txt").unwrap().content,"AI changed");
        let current = read(dir.path(),"a.txt").unwrap();
        assert_eq!(save(dir.path(),"a.txt","merged".into(),&current.sha256).unwrap().content,"merged");
    }
    #[test]
    fn editor_paths_and_create_do_not_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        assert!(checked(dir.path(),"../outside").is_err());
        assert!(checked(dir.path(),"/outside").is_err());
        assert!(checked(dir.path(),"a:stream").is_err());
        create(dir.path(),"new.txt",false).unwrap();
        fs::write(dir.path().join("new.txt"),"keep").unwrap();
        assert!(create(dir.path(),"new.txt",false).is_err());
        assert_eq!(read(dir.path(),"new.txt").unwrap().content,"keep");
    }
}
