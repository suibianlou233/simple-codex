//! Simple-owned skill library. Import is inert; scripts are never executed here.
use super::*;
use serde_yaml::Value as Yaml;
use std::io::Read;

fn bounded_read(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    check_ancestors(path)?;
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("技能文件超出大小上限".into());
    }
    Ok(bytes)
}

#[derive(Default)]
pub(crate) struct SkillState(Mutex<()>);
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkillEntry {
    id: String,
    name: String,
    description: String,
    source: PathBuf,
    revision: String,
    enabled: bool,
    issues: Vec<String>,
    files: usize,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Candidate {
    id: String,
    name: String,
    description: String,
}
#[derive(Default, Serialize, Deserialize)]
struct Library {
    entries: Vec<SkillEntry>,
}

fn library_root(app: &AppHandle) -> Result<PathBuf, String> {
    let root = app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("skills");
    check_ancestors(&root)?;
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    Ok(root)
}
fn check_ancestors(path: &Path) -> Result<(), String> {
    for part in path.ancestors() {
        let meta = match fs::symlink_metadata(part) {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.to_string()),
        };
        {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if meta.file_attributes() & 0x400 != 0 {
                    return Err("技能路径不能经过链接或重解析目录".into());
                }
            }
            if meta.file_type().is_symlink() {
                return Err("技能路径不能经过符号链接".into());
            }
        }
    }
    Ok(())
}
fn read_library(root: &Path) -> Result<Library, String> {
    let path = root.join("library.json");
    check_ancestors(&path)?;
    if !path.exists() {
        return Ok(Library::default());
    }
    let data = bounded_read(&path, 1024 * 1024)?;
    if data.len() > 1024 * 1024 {
        return Err("技能索引过大".into());
    }
    let library: Library = serde_json::from_slice(&data).map_err(|_| "技能索引损坏")?;
    for entry in &library.entries {
        Uuid::parse_str(&entry.revision).map_err(|_| "技能版本无效")?;
        if entry.id != entry.name || !valid_name(&entry.id) {
            return Err("技能索引名称无效".into());
        }
    }
    Ok(library)
}
fn save_library(root: &Path, library: &Library) -> Result<(), String> {
    let temporary = root.join(format!(".library-{}.tmp", Uuid::new_v4()));
    fs::write(
        &temporary,
        serde_json::to_vec_pretty(library).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::rename(temporary, root.join("library.json")).map_err(|e| e.to_string())
}
/// A completed read is evidence of loading instructions, not proof of following them.
pub(super) fn read_skill_name(item: &CodexThreadItem) -> Option<String> {
    if item.kind != "commandExecution" || !item.execution_succeeded() {
        return None;
    }
    let command = item.value["command"].as_str()?;
    if !command.contains("SKILL.md")
        || !["Get-Content", "cat ", "sed ", "type ", "read_text"]
            .iter()
            .any(|word| command.contains(word))
    {
        return None;
    }
    let output = item.value["aggregatedOutput"]
        .as_str()?
        .replace("\r\n", "\n");
    let start = if output.starts_with("---\n") {
        0
    } else {
        output.find("\n---\n")? + 1
    };
    metadata(&output[start..]).ok().map(|(name, _)| name)
}
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}
fn metadata(text: &str) -> Result<(String, String), String> {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let yaml = text
        .strip_prefix("---\n")
        .and_then(|s| s.split_once("\n---").map(|p| p.0))
        .ok_or("SKILL.md 缺少 YAML 名称和描述")?;
    let value: Yaml = serde_yaml::from_str(yaml).map_err(|_| "技能头部 YAML 无效")?;
    let name = value["name"]
        .as_str()
        .ok_or("技能缺少 name")?
        .trim()
        .to_owned();
    let description = value["description"]
        .as_str()
        .ok_or("技能缺少 description")?
        .trim()
        .to_owned();
    if !valid_name(&name) || description.is_empty() || description.chars().count() > 4000 {
        return Err("技能名称或描述不符合要求".into());
    }
    Ok((name, description))
}
fn collect_files(root: &Path) -> Result<Vec<(PathBuf, Vec<u8>)>, String> {
    check_ancestors(root)?;
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    let mut total = 0;
    let mut directories = 0;
    while let Some(directory) = pending.pop() {
        directories += 1;
        if directories > 2000 {
            return Err("技能目录过多".into());
        }
        for item in fs::read_dir(directory).map_err(|e| e.to_string())? {
            let path = item.map_err(|e| e.to_string())?.path();
            check_ancestors(&path)?;
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or("技能文件名无效")?;
            let normalized_name = name.to_ascii_lowercase();
            let name = normalized_name.as_str();
            if [".git", "node_modules", "__pycache__"].contains(&name) {
                continue;
            }
            if name == ".env"
                || name.starts_with(".env.") && name != ".env.example"
                || ["credentials.json", "auth.json", "id_rsa", "id_ed25519"].contains(&name)
            {
                return Err("技能目录包含凭据文件，请移除后导入".into());
            }
            let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
            if meta.is_dir() {
                if path
                    .strip_prefix(root)
                    .map_err(|e| e.to_string())?
                    .components()
                    .count()
                    > 12
                {
                    return Err("技能目录层级过深".into());
                }
                pending.push(path);
                continue;
            }
            if !meta.is_file() {
                return Err("技能目录包含非普通文件".into());
            }
            if meta.len() > 16 * 1024 * 1024 {
                return Err("技能单文件超过 16 MiB".into());
            }
            let bytes = bounded_read(&path, 16 * 1024 * 1024)?;
            total += bytes.len();
            if total > 32 * 1024 * 1024 || files.len() >= 2000 {
                return Err("技能包超过 32 MiB 或 2000 文件".into());
            }
            files.push((
                path.strip_prefix(root)
                    .map_err(|e| e.to_string())?
                    .to_path_buf(),
                bytes,
            ));
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files)
}
fn program_exists(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|p| {
                p.join(if cfg!(windows) {
                    format!("{name}.exe")
                } else {
                    name.to_owned()
                })
                .is_file()
            })
        })
        .unwrap_or(false)
}
fn inspect(files: &[(PathBuf, Vec<u8>)]) -> Vec<String> {
    let mut issues = Vec::new();
    if files
        .iter()
        .any(|(p, _)| p.extension().is_some_and(|e| e == "py"))
        && !program_exists("python")
    {
        issues.push("需要 Python：当前环境未找到 python".into());
    }
    if files
        .iter()
        .any(|(p, _)| p.extension().is_some_and(|e| e == "js" || e == "mjs"))
        && !program_exists("node")
    {
        issues.push("需要 Node.js：当前环境未找到 node".into());
    }
    for (path, bytes) in files {
        let Ok(text) = std::str::from_utf8(bytes) else {
            continue;
        };
        if path == Path::new("agents/openai.yaml") {
            match serde_yaml::from_str::<Yaml>(text) {
                Ok(value) => {
                    if let Some(tools) = value["dependencies"]["tools"].as_sequence() {
                        for tool in tools {
                            issues.push(format!(
                                "工具依赖需适配：{}",
                                tool["value"].as_str().unwrap_or("未命名工具")
                            ));
                        }
                    }
                }
                Err(_) => issues.push("agents/openai.yaml 无法解析，需检查".into()),
            }
        }
        if text.contains("C:\\Users\\")
            || text.contains("C:/Users/")
            || text.contains("/Users/")
            || text.contains("/home/")
        {
            issues.push("包含机器绝对路径，请检查迁移后的路径".into());
        }
        if [
            "image_gen",
            "image2",
            "cua.",
            "mcp__codex_app__",
            "functions.request_user_input",
            "mcp__cua_repl",
        ]
        .iter()
        .any(|s| text.contains(s))
        {
            issues.push("引用 Codex 专用工具，需要适配后再启用".into());
        }
        if ["requirements.txt", "package.json"]
            .iter()
            .any(|s| path.file_name().is_some_and(|n| n == *s))
        {
            issues.push("包含额外软件依赖清单，需先检查安装情况".into());
        }
    }
    issues.sort();
    issues.dedup();
    issues
}
fn import_skill(root: &Path, source: &Path) -> Result<SkillEntry, String> {
    let files = collect_files(source)?;
    let document = files
        .iter()
        .find(|(p, _)| p == Path::new("SKILL.md"))
        .ok_or("所选文件夹根目录没有 SKILL.md")?;
    let (name, description) =
        metadata(std::str::from_utf8(&document.1).map_err(|_| "SKILL.md 不是 UTF-8")?)?;
    let mut library = read_library(root)?;
    if library.entries.len() >= 128 && !library.entries.iter().any(|e| e.id == name) {
        return Err("技能库已达到 128 项上限".into());
    }
    let mut issues = inspect(&files);
    if source.components().any(|part| part.as_os_str() == "plugins") {
        issues.push("插件技能的工具与运行环境尚未在 Simple 验证".into());
    }
    let revision = Uuid::new_v4().to_string();
    let package = root.join("packages").join(&revision);
    check_ancestors(&package)?;
    fs::create_dir_all(&package).map_err(|e| e.to_string())?;
    for (path, bytes) in &files {
        let target = package.join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::write(target, bytes).map_err(|e| e.to_string())?;
    }
    let enabled = issues.is_empty()
        && library
            .entries
            .iter()
            .find(|e| e.id == name)
            .map(|e| e.enabled)
            .unwrap_or(true);
    let entry = SkillEntry {
        id: name.clone(),
        name,
        description,
        source: source.to_path_buf(),
        revision,
        enabled,
        issues,
        files: files.len(),
    };
    library.entries.retain(|e| e.id != entry.id);
    library.entries.push(entry.clone());
    save_library(root, &library)?;
    Ok(entry)
}
fn candidates() -> Vec<(Candidate, PathBuf)> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let mut result = Vec::new();
    let mut visited = 0;
    for base in [home.join(".codex/skills"), home.join(".agents/skills")] {
        let mut pending = vec![(base, 0)];
        while let Some((root, depth)) = pending.pop() {
            visited += 1;
            if visited > 4096 || result.len() >= 256 {
                break;
            }
            if check_ancestors(&root).is_err() {
                continue;
            }
            if let Ok(text) = bounded_read(&root.join("SKILL.md"), 4 * 1024 * 1024)
                .and_then(|b| String::from_utf8(b).map_err(|e| e.to_string()))
            {
                if let Ok((name, description)) = metadata(&text) {
                    result.push((
                        Candidate {
                            id: hash_bytes(root.to_string_lossy().as_bytes()),
                            name,
                            description,
                        },
                        root,
                    ));
                }
                continue;
            }
            if depth >= 4 || result.len() > 256 {
                continue;
            }
            if let Ok(items) = fs::read_dir(root) {
                for item in items.flatten() {
                    if item.file_name().to_string_lossy().starts_with('.') {
                        continue;
                    }
                    if item.path().is_dir() {
                        pending.push((item.path(), depth + 1));
                    }
                }
            }
        }
    }
    result.sort_by(|a, b| a.0.name.cmp(&b.0.name));
    result
}
pub(crate) async fn apply_to_kernel(
    app: &AppHandle,
    client: &CodexKernelClient,
) -> Result<(), DesktopError> {
    let roots = {
        let state = app.state::<SkillState>();
        let _guard = state.0.lock().map_err(|_| DesktopError::StateUnavailable)?;
        let root = library_root(app).map_err(DesktopError::SkillLibrary)?;
        let library = read_library(&root).map_err(DesktopError::SkillLibrary)?;
        let mut roots = Vec::new();
        for entry in library.entries.iter().filter(|e| e.enabled) {
            let path = root.join("packages").join(&entry.revision);
            check_ancestors(&path).map_err(|_| DesktopError::StateUnavailable)?;
            if !path.join("SKILL.md").is_file() {
                return Err(DesktopError::StateUnavailable);
            }
            roots.push(path);
        }
        roots
    };
    CodexFeatureBridge::new(client.clone())
        .set_skill_roots(&roots)
        .await
        .map_err(|e| DesktopError::CodexFeature(e))
}
#[tauri::command]
pub(crate) fn skills_library_list(
    app: AppHandle,
    state: State<'_, SkillState>,
) -> Result<Vec<SkillEntry>, String> {
    let _guard = state.0.lock().map_err(|_| "技能库忙")?;
    Ok(read_library(&library_root(&app)?)?.entries)
}
#[tauri::command]
pub(crate) fn skills_codex_list() -> Vec<Candidate> {
    candidates().into_iter().map(|(c, _)| c).collect()
}
#[tauri::command]
pub(crate) async fn skills_import(
    app: AppHandle,
    source_id: Option<String>,
) -> Result<Option<SkillEntry>, String> {
    let source = match source_id {
        Some(id) if id.starts_with("update:") => {
            let root = library_root(&app)?;
            read_library(&root)?
                .entries
                .into_iter()
                .find(|e| e.id == id[7..])
                .ok_or("技能不存在")?
                .source
        }
        Some(id) => {
            candidates()
                .into_iter()
                .find(|(c, _)| c.id == id)
                .ok_or("本机技能已不存在")?
                .1
        }
        None => match rfd::AsyncFileDialog::new()
            .set_title("选择包含 SKILL.md 的技能文件夹")
            .pick_folder()
            .await
        {
            Some(p) => p.path().to_path_buf(),
            None => return Ok(None),
        },
    };
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<SkillState>();
        let _guard = state.0.lock().map_err(|_| "技能库忙")?;
        import_skill(&library_root(&app)?, &source).map(Some)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(crate) fn skills_set_enabled(
    app: AppHandle,
    state: State<'_, SkillState>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    let _guard = state.0.lock().map_err(|_| "技能库忙")?;
    let root = library_root(&app)?;
    let mut library = read_library(&root)?;
    let entry = library
        .entries
        .iter_mut()
        .find(|e| e.id == id)
        .ok_or("技能不存在")?;
    if enabled {
        entry.issues = inspect(&collect_files(
            &root.join("packages").join(&entry.revision),
        )?);
    }
    entry.enabled = enabled;
    save_library(&root, &library)
}
#[tauri::command]
pub(crate) fn skills_document(app: AppHandle, id: String) -> Result<String, String> {
    let root = library_root(&app)?;
    let entry = read_library(&root)?
        .entries
        .into_iter()
        .find(|e| e.id == id)
        .ok_or("技能不存在")?;
    let path = root.join("packages").join(entry.revision).join("SKILL.md");
    check_ancestors(&path)?;
    String::from_utf8(bounded_read(&path, 4 * 1024 * 1024)?).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(path: &Path) {
        fs::create_dir_all(path.join("scripts")).unwrap();
        fs::write(path.join("SKILL.md"), "---\nname: test-skill\ndescription: >\n  Test a local\n  skill.\n---\nRead scripts/helper.txt").unwrap();
        fs::write(path.join("scripts/helper.txt"), "kept").unwrap();
    }
    #[test]
    fn import_update_and_disable_survive_reload() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let root = temp.path().join("library");
        fixture(&source);
        fs::create_dir(&root).unwrap();
        let first = import_skill(&root, &source).unwrap();
        assert!(first.enabled);
        assert_eq!(first.files, 2);
        assert_eq!(first.description, "Test a local skill.");
        let mut library = read_library(&root).unwrap();
        library.entries[0].enabled = false;
        save_library(&root, &library).unwrap();
        fs::write(source.join("scripts/helper.txt"), "new").unwrap();
        let second = import_skill(&root, &source).unwrap();
        assert!(!second.enabled);
        assert_ne!(first.revision, second.revision);
        assert_eq!(
            fs::read_to_string(
                root.join("packages")
                    .join(first.revision)
                    .join("scripts/helper.txt")
            )
            .unwrap(),
            "kept"
        );
        assert_eq!(read_library(&root).unwrap().entries.len(), 1);
        assert_eq!(
            fs::read_to_string(source.join("scripts/helper.txt")).unwrap(),
            "new"
        );
    }
    #[test]
    fn rejects_credentials_and_unsafe_names() {
        let temp = tempfile::tempdir().unwrap();
        fixture(temp.path());
        fs::write(temp.path().join(".env"), "fake").unwrap();
        assert!(collect_files(temp.path()).unwrap_err().contains("凭据"));
        assert!(metadata("---\nname: ../escape\ndescription: test\n---").is_err());
        assert!(metadata("---\nname: ok\ndescription: ''\n---").is_err());
    }
    #[test]
    fn tool_dependent_import_is_disabled_and_inert() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let root = temp.path().join("library");
        fixture(&source);
        fs::create_dir(&root).unwrap();
        fs::write(
            source.join("scripts/unsafe.txt"),
            "mcp__codex_app__ tool /home/someone/file",
        )
        .unwrap();
        let entry = import_skill(&root, &source).unwrap();
        assert!(!entry.enabled);
        assert_eq!(entry.issues.len(), 2);
    }
    #[test]
    fn loading_label_requires_successful_read_and_valid_frontmatter() {
        let mut item = CodexThreadItem {
            id: "read".into(),
            kind: "commandExecution".into(),
            value: json!({"command":"Get-Content /skills/example/SKILL.md","status":"completed","exitCode":0,"aggregatedOutput":"---\nname: example\ndescription: Example skill\n---\nInstructions"}),
        };
        assert_eq!(read_skill_name(&item).as_deref(), Some("example"));
        item.value["exitCode"] = json!(1);
        assert!(read_skill_name(&item).is_none());
        item.value["exitCode"] = json!(0);
        item.value["command"] = json!("echo SKILL.md");
        assert!(read_skill_name(&item).is_none());
    }
    #[cfg(windows)]
    #[test]
    fn rejects_junction_roots_and_children() {
        use std::os::windows::process::CommandExt;
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target");
        let link = temp.path().join("link");
        fixture(&target);
        let status=std::process::Command::new("powershell.exe")
            .args(["-NoProfile","-NonInteractive","-Command","New-Item -ItemType Junction -Path $env:SIMPLE_TEST_LINK -Target $env:SIMPLE_TEST_TARGET | Out-Null"])
            .env("SIMPLE_TEST_LINK",&link).env("SIMPLE_TEST_TARGET",&target)
            .creation_flags(0x08000000).status().unwrap();
        assert!(status.success());
        let root_rejected = collect_files(&link).is_err();
        let child_rejected = bounded_read(&link.join("SKILL.md"), 4096).is_err();
        fs::remove_dir(&link).unwrap();
        assert!(root_rejected && child_rejected);
        assert!(target.join("SKILL.md").is_file());
    }

    #[test]
    fn read_is_bounded() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("large");
        fs::write(&path, b"12345").unwrap();
        assert!(bounded_read(&path, 4).is_err());
    }
    #[test]
    #[ignore = "Explicitly authorized local skill batch migration"]
    fn import_selected_skills() {
        let list=PathBuf::from(std::env::var("SIMPLE_SKILL_IMPORT_LIST").expect("explicit list"));
        let paths:Vec<PathBuf>=serde_json::from_slice(&fs::read(list).unwrap()).unwrap();
        let root=PathBuf::from(std::env::var("SIMPLE_SKILL_IMPORT_ROOT").expect("explicit root"));
        let commit=std::env::var("SIMPLE_SKILL_IMPORT_COMMIT").as_deref()==Ok("1");
        let mut results=Vec::new();
        for source in paths {
            let result = if commit { import_skill(&root,&source).map(|e|json!({"name":e.name,"enabled":e.enabled,"files":e.files,"issues":e.issues})) }
            else { collect_files(&source).and_then(|files| {let doc=files.iter().find(|(p,_)|p==Path::new("SKILL.md")).ok_or("missing skill")?;let (name,_)=metadata(std::str::from_utf8(&doc.1).map_err(|e|e.to_string())?)?;Ok(json!({"name":name,"files":files.len(),"issues":inspect(&files)}))}) };
            match result {Ok(value)=>{println!("OK {}",value["name"]);results.push(json!({"source":source,"result":value}));},Err(error)=>{println!("FAILED {}: {}",source.display(),error);results.push(json!({"source":source,"error":error}));}}
        }
        let output=PathBuf::from(std::env::var("SIMPLE_SKILL_IMPORT_REPORT").expect("report"));
        fs::write(output,serde_json::to_vec_pretty(&results).unwrap()).unwrap();
        assert!(results.iter().all(|r|r.get("error").is_none()),"see report for failed imports");
    }

    #[test]
    #[ignore = "Explicit local acceptance: import the user's requested novel skill"]
    fn import_requested_novel() {
        let root =
            PathBuf::from(std::env::var("SIMPLE_SKILL_IMPORT_ROOT").expect("explicit test root"));
        check_ancestors(&root).unwrap();
        fs::create_dir_all(&root).unwrap();
        let source = dirs::home_dir()
            .unwrap()
            .join(".codex/skills/write-serialized-novel");
        let entry = import_skill(&root, &source).unwrap();
        println!(
            "imported {}: enabled={}, files={}, issues={:?}",
            entry.name, entry.enabled, entry.files, entry.issues
        );
        assert_eq!(entry.name, "write-serialized-novel");
        assert!(entry.enabled);
    }
}

#[cfg(test)]
mod live_acceptance {
    use super::*;
    #[tokio::test]
    #[ignore = "Uses the explicitly selected local model profile for bounded skill acceptance"]
    async fn implicit_novel_skill_live() -> Result<(), Box<dyn std::error::Error>> {
        let db = PathBuf::from(std::env::var("SIMPLE_SKILL_TEST_DB")?);
        let profiles = Storage::read_model_profiles_read_only(&db)?;
        let profile = profiles
            .into_iter()
            .find(|p| p.is_default)
            .ok_or("no configured default profile")?;
        let gateway = gateway_for_profile(&profile, &SystemSecretStore)?;
        let alias = gateway.codex_model_alias.clone();
        let root = PathBuf::from(std::env::var("SIMPLE_SKILL_TEST_OUTPUT")?);
        if root.exists() {
            return Err("acceptance output must be new".into());
        }
        fs::create_dir_all(root.join("project"))?;
        fs::create_dir_all(root.join("library"))?;
        let source = dirs::home_dir()
            .ok_or("home unavailable")?
            .join(".codex/skills/write-serialized-novel");
        let entry = import_skill(&root.join("library"), &source)?;
        let package =
            KernelPackage::load(Path::new(&std::env::var("SIMPLE_CODEX_KERNEL_MANIFEST")?))?;
        let selection = KernelSelection::Package(package);
        let (client, mut events) =
            CodexKernelClient::start(selection.configuration(root.join("home"), gateway)).await?;
        let result:Result<(), Box<dyn std::error::Error>>=async {
            CodexFeatureBridge::new(client.clone()).set_skill_roots(&[root.join("library/packages").join(entry.revision)]).await?;
            let thread=CodexSessionBridge::new(client.clone()).start_thread(local_agent_model::CodexThreadOptions{
                cwd:&root.join("project"),model:Some(&alias),approval_policy:"never",sandbox:"workspace-write",context_window:profile.context_window_tokens,
            }).await?;
            client.request("turn/start",json!({"threadId":thread,"model":alias,"input":[{"type":"text","text":"帮我写一部长篇中文小说，书名《雾港来信》，悬疑题材，主角是一名寻找失踪妹妹的邮递员，计划三卷。现在只初始化小说项目骨架，列出需要我确认的设定，不生成正文。请把项目建在当前工作目录，不要联网。","text_elements":[]}]})).await?;
            let mut completed=false;let mut read_skill=false;let mut announced=false;
            let deadline=tokio::time::Instant::now()+Duration::from_secs(240);
            while let Some(wire)=tokio::time::timeout_at(deadline,events.next()).await? {
                let wire=wire?;
                if let local_agent_model::CodexKernelWireMessage::Request{id,..}=&wire {
                    client.respond_error(id.clone(),json!({"code":-32601,"message":"No interactive tools in bounded acceptance"})).await?;
                }
                let event=CodexKernelEvent::project(wire)?;
                if let CodexKernelEvent::ItemCompleted{item,..}=&event {
                    if let Some(text)=item.agent_text(){announced |= text.contains("write-serialized-novel") || text.contains("长篇") && text.contains("技能");}
                    if item.kind=="commandExecution" && item.execution_succeeded() {
                        let output=item.value["aggregatedOutput"].as_str().unwrap_or("");
                        read_skill |= read_skill_name(&item).as_deref() == Some("write-serialized-novel");
                        let _ = output;
                    }
                    println!("completed item: {}",item.kind);
                }
                if let CodexKernelEvent::TurnCompleted{status,error_message,..}=&event {
                    println!("turn outcome: {:?}; error present: {}",status,error_message.is_some());
                    completed=*status==CodexTurnStatus::Completed;break;
                }
            }
            let manifest=root.join("project/.novel-project/manifest.json");
            fs::write(root.join("result.json"),serde_json::to_vec_pretty(&json!({"completed":completed,"readSkill":read_skill,"announced":announced,"manifestCreated":manifest.is_file()}))?)?;
            if !completed || !read_skill || !manifest.is_file(){return Err("skill acceptance incomplete; inspect isolated rollout".into());}
            Ok(())
        }.await;
        client.shutdown().await?;
        result
    }
}
