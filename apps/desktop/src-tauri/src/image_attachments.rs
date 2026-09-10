//! Explicitly selected images, stored outside the workspace and scoped to a project.
//! Conversation text holds durable opaque references, never base64 or arbitrary paths.
use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::io::{Cursor, Read, Write};

const PREFIX: &str = "simple-image:";
const MAX_IMAGE_BYTES: usize = 4 * 1024 * 1024;
const MAX_IMAGES: usize = 8;

fn image_error(message: &str) -> DesktopError {
    DesktopError::InvalidAttachment(message.into())
}

fn mime(bytes: &[u8]) -> Result<&'static str, DesktopError> {
    if bytes.is_empty() || bytes.len() > MAX_IMAGE_BYTES {
        return Err(image_error("图片不得超过 4 MiB"));
    }
    let format = image::guess_format(bytes).map_err(|_| image_error("图片格式无效"))?;
    let mime = match format {
        image::ImageFormat::Png => "image/png",
        image::ImageFormat::Jpeg => "image/jpeg",
        image::ImageFormat::Gif => "image/gif",
        image::ImageFormat::WebP => "image/webp",
        _ => return Err(image_error("只支持 PNG、JPEG、GIF 和 WebP 图片")),
    };
    let (width, height) = image::ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .map_err(|_| image_error("无法读取图片尺寸"))?;
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return Err(image_error("图片单边尺寸必须在 1–8192 像素之间"));
    }
    Ok(mime)
}

fn directory(database: &Path, project: &Path) -> Result<PathBuf, DesktopError> {
    let canonical = fs::canonicalize(project)?;
    let scope = hash_bytes(canonical.to_string_lossy().as_bytes());
    Ok(database
        .parent()
        .ok_or(DesktopError::InvalidStoredPath)?
        .join("image-attachments")
        .join(scope))
}

fn image_id(reference: &str) -> Result<&str, DesktopError> {
    let id = reference
        .strip_prefix(PREFIX)
        .ok_or_else(|| image_error("图片引用无效"))?;
    if id.len() != 64
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(image_error("图片引用无效"));
    }
    Ok(id)
}

pub(super) fn save(
    database: &Path,
    project: &Path,
    name: &str,
    bytes: &[u8],
) -> Result<BackendAttachment, DesktopError> {
    mime(bytes)?;
    let id = hash_bytes(bytes);
    let root = directory(database, project)?;
    fs::create_dir_all(&root)?;
    let path = root.join(&id);
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(mut file) => {
            file.write_all(bytes)?;
            file.sync_all()?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let existing = read(database, project, &format!("{PREFIX}{id}"))?;
            if existing != bytes {
                return Err(image_error("已保存图片的校验不匹配"));
            }
        }
        Err(error) => return Err(error.into()),
    }
    Ok(BackendAttachment {
        path: format!("{PREFIX}{id}"),
        name: name
            .chars()
            .filter(|c| !c.is_control() && !matches!(c, '[' | ']' | '\\'))
            .take(120)
            .collect(),
        size_bytes: bytes.len() as u64,
    })
}

fn read(database: &Path, project: &Path, reference: &str) -> Result<Vec<u8>, DesktopError> {
    let id = image_id(reference)?;
    let root = fs::canonicalize(directory(database, project)?)?;
    let path = fs::canonicalize(root.join(id))?;
    if path.parent() != Some(root.as_path()) {
        return Err(image_error("图片路径越界"));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_IMAGE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    mime(&bytes)?;
    if hash_bytes(&bytes) != id {
        return Err(image_error("图片内容已改变，请重新添加"));
    }
    Ok(bytes)
}

fn references(content: &str) -> Result<Vec<String>, DesktopError> {
    let mut result = Vec::new();
    let mut fence: Option<&str> = None;
    for line in content.lines().map(str::trim) {
        if line.starts_with("```") || line.starts_with("~~~") {
            let marker = &line[..3];
            if fence == Some(marker) {
                fence = None;
            } else if fence.is_none() {
                fence = Some(marker);
            }
            continue;
        }
        if fence.is_some() || !line.starts_with("![") {
            continue;
        }
        let Some((_, target)) = line.rsplit_once("](") else {
            continue;
        };
        let Some(reference) = target.strip_suffix(')').filter(|s| s.starts_with(PREFIX)) else {
            continue;
        };
        image_id(reference)?;
        if !result.iter().any(|value| value == reference) {
            result.push(reference.to_owned());
        }
    }
    if result.len() > MAX_IMAGES {
        return Err(image_error("每条消息最多 8 张图片"));
    }
    Ok(result)
}

pub(super) fn input(
    database: &Path,
    project: &Path,
    content: &str,
    supported: bool,
) -> Result<Vec<Value>, DesktopError> {
    let refs = references(content)?;
    if !refs.is_empty() && !supported {
        return Err(image_error(
            "当前模型配置尚不支持图片输入。DeepSeek 原生配置可自动使用视觉模型；其他接口请使用已支持图片的配置。",
        ));
    }
    refs.iter().map(|reference| {
        let bytes = read(database, project, reference)?;
        Ok(json!({"type":"image", "url":format!("data:{};base64,{}", mime(&bytes)?, STANDARD.encode(bytes))}))
    }).collect()
}

#[tauri::command]
pub async fn pick_images(
    window: tauri::Webview,
    project_id: String,
    state: State<'_, DesktopState>,
) -> Result<Vec<BackendAttachment>, String> {
    if window.label() != "main" {
        return Err("仅主窗口可以添加图片".into());
    }
    let (database, project) = {
        let runtime = state.lock().map_err(command_error)?;
        (
            runtime.database_path.clone(),
            runtime.project_root(&project_id).map_err(command_error)?,
        )
    };
    let files = rfd::AsyncFileDialog::new()
        .set_title("添加图片（每张最多 4 MiB）")
        .add_filter("图片", &["png", "jpg", "jpeg", "gif", "webp"])
        .pick_files()
        .await
        .unwrap_or_default();
    if files.len() > MAX_IMAGES {
        return Err("每次最多选择 8 张图片".into());
    }
    files
        .into_iter()
        .map(|file| {
            let mut bytes = Vec::new();
            fs::File::open(file.path())
                .map_err(command_error)?
                .take(MAX_IMAGE_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(command_error)?;
            save(&database, &project, &file.file_name(), &bytes).map_err(command_error)
        })
        .collect()
}

#[tauri::command]
pub fn paste_image(
    window: tauri::Webview,
    project_id: String,
    name: String,
    data: String,
    state: State<'_, DesktopState>,
) -> Result<BackendAttachment, String> {
    if window.label() != "main" {
        return Err("仅主窗口可以添加图片".into());
    }
    if data.len() > MAX_IMAGE_BYTES.div_ceil(3) * 4 {
        return Err("图片不得超过 4 MiB".into());
    }
    let bytes = STANDARD
        .decode(data)
        .map_err(|_| "图片编码无效".to_owned())?;
    let runtime = state.lock().map_err(command_error)?;
    let project = runtime.project_root(&project_id).map_err(command_error)?;
    save(&runtime.database_path, &project, &name, &bytes).map_err(command_error)
}

#[tauri::command]
pub fn read_image_attachment(
    window: tauri::Webview,
    project_id: String,
    reference: String,
    state: State<'_, DesktopState>,
) -> Result<String, String> {
    if window.label() != "main" {
        return Err("仅主窗口可以读取图片".into());
    }
    let runtime = state.lock().map_err(command_error)?;
    let project = runtime.project_root(&project_id).map_err(command_error)?;
    let bytes = read(&runtime.database_path, &project, &reference).map_err(command_error)?;
    Ok(format!(
        "data:{};base64,{}",
        mime(&bytes).map_err(command_error)?,
        STANDARD.encode(bytes)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn references_reject_paths_and_limit_count() {
        assert!(image_id("simple-image:../../secret").is_err());
        assert!(references("![x](simple-image:bad)").is_err());
        assert!(references("ordinary message").unwrap().is_empty());
        let refs = (0..9)
            .map(|n| format!("![x](simple-image:{n:064x})"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(references(&refs).is_err());
    }
    #[test]
    fn rejects_non_images_and_non_visual_model() {
        assert!(mime(b"not an image").is_err());
        assert!(
            input(
                Path::new("db"),
                Path::new("."),
                &format!("![x](simple-image:{:064x})", 0),
                false
            )
            .is_err()
        );
    }
    #[test]
    fn saved_images_survive_reload_and_cannot_cross_projects() {
        let home = tempfile::tempdir().unwrap();
        let project = home.path().join("one");
        let other = home.path().join("two");
        fs::create_dir(&project).unwrap();
        fs::create_dir(&other).unwrap();
        let database = home.path().join("local.db");
        let png = STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLbtAAAAABJRU5ErkJggg==").unwrap();
        let attachment = save(&database, &project, "example.png", &png).unwrap();
        assert_eq!(read(&database, &project, &attachment.path).unwrap(), png);
        assert!(read(&database, &other, &attachment.path).is_err());
        assert!(
            references(&format!("```text\n![x]({})\n```", attachment.path))
                .unwrap()
                .is_empty()
        );
        let path = directory(&database, &project)
            .unwrap()
            .join(image_id(&attachment.path).unwrap());
        let mut changed = png;
        changed.push(0);
        fs::write(path, changed).unwrap();
        assert!(read(&database, &project, &attachment.path).is_err());
    }
}
