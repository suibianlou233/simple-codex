//! Explicit, paid media generation through a user-configured provider.
//!
//! This stays separate from the chat ModelAdapter: image generation is an
//! immediate asset operation, while video generation has a durable remote-task
//! lifecycle. Provider URLs are never persisted as the source of truth.
use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures_util::StreamExt;
use reqwest::{Client, StatusCode, Url};
use sha2::{Digest, Sha256};
use std::io::Write as _;
use tokio::io::AsyncWriteExt;

const STORE_VERSION: u32 = 1;
const CREDENTIAL_REF: &str = "media-profile:volcengine-ark";
const DEFAULT_BASE_URL: &str = "https://ark.cn-beijing.volces.com/api/v3";
const DEFAULT_IMAGE_MODEL: &str = "doubao-seedream-5-0-flash-260915";
const DEFAULT_VIDEO_MODEL: &str = "doubao-seedance-2-0-mini-260615";
const MAX_PROMPT_CHARS: usize = 10_000;
const MAX_IMAGE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_VIDEO_BYTES: u64 = 512 * 1024 * 1024;
const MAX_JOBS: usize = 200;

pub struct MediaGenerationState {
    file: PathBuf,
    client: Client,
    write_lock: AsyncMutex<()>,
}

impl MediaGenerationState {
    pub fn new(data_directory: &Path) -> Result<Self, String> {
        let directory = data_directory.join("media-generation");
        fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        let client = Client::builder()
            .user_agent("Simple/0.1 media-generation")
            .build()
            .map_err(|error| format!("无法初始化媒体连接：{error}"))?;
        Ok(Self {
            file: directory.join("state.json"),
            client,
            write_lock: AsyncMutex::new(()),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredConfig {
    #[serde(default = "default_base_url")]
    base_url: String,
    #[serde(default = "default_image_model")]
    image_model: String,
    #[serde(default = "default_video_model")]
    video_model: String,
}

impl Default for StoredConfig {
    fn default() -> Self {
        Self {
            base_url: default_base_url(),
            image_model: default_image_model(),
            video_model: default_video_model(),
        }
    }
}

fn default_base_url() -> String {
    DEFAULT_BASE_URL.to_owned()
}
fn default_image_model() -> String {
    DEFAULT_IMAGE_MODEL.to_owned()
}
fn default_video_model() -> String {
    DEFAULT_VIDEO_MODEL.to_owned()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MediaStore {
    #[serde(default = "store_version")]
    version: u32,
    #[serde(default)]
    config: StoredConfig,
    #[serde(default)]
    jobs: Vec<MediaJob>,
}

fn store_version() -> u32 {
    STORE_VERSION
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaJob {
    id: String,
    project_id: String,
    kind: String,
    model: String,
    status: String,
    prompt: String,
    created_at_ms: i64,
    updated_at_ms: i64,
    provider_task_id: Option<String>,
    asset_ref: Option<String>,
    asset_path: Option<String>,
    mime_type: Option<String>,
    error: Option<String>,
    usage_tokens: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaSettings {
    base_url: String,
    image_model: String,
    video_model: String,
    has_credential: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaAssetInfo {
    reference: String,
    kind: String,
    mime_type: String,
    width: Option<u32>,
    height: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveMediaSettingsInput {
    base_url: String,
    image_model: String,
    video_model: String,
    api_key: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateImageInput {
    project_id: String,
    prompt: String,
    model: Option<String>,
    size: String,
    reference: Option<String>,
    #[serde(default)]
    watermark: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateVideoInput {
    project_id: String,
    prompt: String,
    model: Option<String>,
    resolution: String,
    ratio: String,
    duration: u32,
    first_frame: Option<String>,
    #[serde(default)]
    generate_audio: bool,
    #[serde(default)]
    watermark: bool,
}

fn merge_stores(mut current: MediaStore, backup: MediaStore) -> MediaStore {
    for candidate in backup.jobs {
        match current.jobs.iter_mut().find(|job| job.id == candidate.id) {
            Some(existing)
                if candidate.updated_at_ms > existing.updated_at_ms
                    || candidate.updated_at_ms == existing.updated_at_ms
                        && candidate.asset_ref.is_some()
                        && existing.asset_ref.is_none() =>
            {
                *existing = candidate;
            }
            Some(_) => {}
            None => current.jobs.push(candidate),
        }
    }
    current
        .jobs
        .sort_by(|left, right| right.created_at_ms.cmp(&left.created_at_ms));
    current.jobs.truncate(MAX_JOBS);
    current
}

fn load_store_unlocked(path: &Path) -> Result<MediaStore, String> {
    let read = |candidate: &Path| -> Result<MediaStore, String> {
        let bytes = fs::read(candidate).map_err(|error| error.to_string())?;
        let store: MediaStore =
            serde_json::from_slice(&bytes).map_err(|error| format!("媒体状态文件损坏：{error}"))?;
        if store.version != STORE_VERSION {
            return Err("媒体状态文件版本暂不支持".into());
        }
        Ok(store)
    };
    let previous = path.with_extension("previous.json");
    match (path.exists(), previous.exists()) {
        (false, false) => Ok(MediaStore {
            version: STORE_VERSION,
            ..MediaStore::default()
        }),
        (true, false) => read(path),
        (false, true) => read(&previous),
        (true, true) => match (read(path), read(&previous)) {
            (Ok(current), Ok(backup)) => Ok(merge_stores(current, backup)),
            (Ok(current), Err(_)) => Ok(current),
            (Err(_), Ok(backup)) => Ok(backup),
            (Err(current), Err(backup)) => Err(format!(
                "媒体状态文件及备份均损坏：{current}；备份：{backup}"
            )),
        },
    }
}

fn store_lock(path: &Path) -> Result<fs::File, String> {
    let parent = path.parent().ok_or("媒体状态目录无效")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(parent.join("state.lock"))
        .map_err(|error| error.to_string())?;
    lock.lock().map_err(|error| error.to_string())?;
    Ok(lock)
}

fn load_store(path: &Path) -> Result<MediaStore, String> {
    let _lock = store_lock(path)?;
    load_store_unlocked(path)
}

fn save_store_unlocked(path: &Path, store: &MediaStore) -> Result<(), String> {
    let parent = path.parent().ok_or("媒体状态目录无效")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temporary = parent.join(format!("state-{}.new", Uuid::new_v4()));
    let bytes = serde_json::to_vec_pretty(store).map_err(|error| error.to_string())?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    file.write_all(&bytes).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    let previous = path.with_extension("previous.json");
    if previous.exists() {
        fs::remove_file(&previous).map_err(|error| error.to_string())?;
    }
    if path.exists() {
        fs::rename(path, &previous).map_err(|error| error.to_string())?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        if previous.exists() && !path.exists() {
            let _ = fs::rename(&previous, path);
        }
        let _ = fs::remove_file(&temporary);
        return Err(error.to_string());
    }
    Ok(())
}

#[cfg(test)]
fn save_store(path: &Path, store: &MediaStore) -> Result<(), String> {
    let _lock = store_lock(path)?;
    save_store_unlocked(path, store)
}

fn mutate_store<T>(
    path: &Path,
    update: impl FnOnce(&mut MediaStore) -> Result<T, String>,
) -> Result<T, String> {
    let _lock = store_lock(path)?;
    let mut store = load_store_unlocked(path)?;
    let result = update(&mut store)?;
    save_store_unlocked(path, &store)?;
    Ok(result)
}

fn has_credential() -> bool {
    SystemSecretStore.get(CREDENTIAL_REF).is_ok()
        || std::env::var("ARK_API_KEY").is_ok_and(|value| !value.trim().is_empty())
}

fn credential() -> Result<String, String> {
    SystemSecretStore
        .get(CREDENTIAL_REF)
        .or_else(|_| {
            std::env::var("ARK_API_KEY")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .ok_or(SecretStoreError::NotFound)
        })
        .map_err(|_| "尚未配置火山方舟 API Key".to_owned())
}

fn validate_base_url(value: &str) -> Result<String, String> {
    let value = value.trim().trim_end_matches('/');
    let url = Url::parse(value).map_err(|_| "媒体接口地址无效".to_owned())?;
    let host = url.host_str().ok_or("媒体接口地址缺少主机")?;
    let loopback = matches!(host, "localhost" | "127.0.0.1" | "::1");
    if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
        return Err("媒体接口必须使用 HTTPS；本机回环地址可以使用 HTTP".into());
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("媒体接口地址不能包含凭据、查询参数或片段".into());
    }
    Ok(value.to_owned())
}

fn validate_model(value: &str, family: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 160
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        || !value.contains(family)
    {
        return Err(format!("{family} 模型名称无效"));
    }
    Ok(value.to_owned())
}

fn validate_prompt(value: &str) -> Result<String, String> {
    let value = value.trim();
    let count = value.chars().count();
    if value.is_empty() {
        return Err("生成提示词不能为空".into());
    }
    if count > MAX_PROMPT_CHARS {
        return Err(format!("生成提示词不能超过 {MAX_PROMPT_CHARS} 个字符"));
    }
    Ok(value.to_owned())
}

fn project_context(state: &DesktopState, project_id: &str) -> Result<(PathBuf, PathBuf), String> {
    let runtime = state.lock().map_err(command_error)?;
    Ok((
        runtime.database_path.clone(),
        runtime.project_root(project_id).map_err(command_error)?,
    ))
}

fn asset_root(database: &Path, project: &Path) -> Result<PathBuf, String> {
    let canonical = fs::canonicalize(project).map_err(|error| error.to_string())?;
    let scope = hash_bytes(canonical.to_string_lossy().as_bytes());
    Ok(database
        .parent()
        .ok_or("本地数据目录无效")?
        .join("media-assets")
        .join(scope))
}

fn now_ms() -> Result<i64, String> {
    unix_time_ms().map_err(command_error)
}

async fn insert_job(state: &MediaGenerationState, job: MediaJob) -> Result<(), String> {
    let _guard = state.write_lock.lock().await;
    mutate_store(&state.file, move |store| {
        store.jobs.insert(0, job);
        store.jobs.truncate(MAX_JOBS);
        Ok(())
    })
}

async fn update_job<F>(
    state: &MediaGenerationState,
    id: &str,
    update: F,
) -> Result<MediaJob, String>
where
    F: FnOnce(&mut MediaJob),
{
    let _guard = state.write_lock.lock().await;
    mutate_store(&state.file, |store| {
        let job = store
            .jobs
            .iter_mut()
            .find(|job| job.id == id)
            .ok_or("媒体任务不存在")?;
        update(job);
        job.updated_at_ms = now_ms()?;
        Ok(job.clone())
    })
}

async fn get_job(state: &MediaGenerationState, id: &str) -> Result<MediaJob, String> {
    let _guard = state.write_lock.lock().await;
    load_store(&state.file)?
        .jobs
        .into_iter()
        .find(|job| job.id == id)
        .ok_or_else(|| "媒体任务不存在".to_owned())
}

async fn provider_json(response: reqwest::Response) -> Result<Value, String> {
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("无法读取媒体接口响应：{error}"))?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| {
        if status.is_success() {
            "媒体接口返回了无法识别的数据".to_owned()
        } else {
            format!("媒体接口请求失败（HTTP {}）", status.as_u16())
        }
    })?;
    if status.is_success() {
        return Ok(value);
    }
    let code = value
        .pointer("/error/code")
        .or_else(|| value.get("code"))
        .and_then(Value::as_str)
        .unwrap_or("provider_error");
    let message = value
        .pointer("/error/message")
        .or_else(|| value.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("供应商拒绝了请求");
    let message: String = message.chars().take(400).collect();
    Err(format!("媒体接口请求失败（{code}）：{message}"))
}

fn validate_download_url(value: &str) -> Result<Url, String> {
    let url = Url::parse(value).map_err(|_| "供应商返回了无效下载地址".to_owned())?;
    let host = url.host_str().ok_or("供应商下载地址缺少主机")?;
    let loopback = matches!(host, "localhost" | "127.0.0.1" | "::1");
    if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
        return Err("供应商下载地址不是安全的 HTTPS 地址".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("供应商下载地址不能包含凭据".into());
    }
    Ok(url)
}

fn image_format(bytes: &[u8]) -> Result<(&'static str, &'static str), String> {
    let format = image::guess_format(bytes).map_err(|_| "生成结果不是有效图片".to_owned())?;
    match format {
        image::ImageFormat::Png => Ok(("png", "image/png")),
        image::ImageFormat::Jpeg => Ok(("jpg", "image/jpeg")),
        image::ImageFormat::WebP => Ok(("webp", "image/webp")),
        _ => Err("生成结果的图片格式暂不支持".into()),
    }
}

async fn download_asset(
    state: &MediaGenerationState,
    database: &Path,
    project: &Path,
    url: &str,
    kind: &str,
    output_format: &str,
) -> Result<(String, String, String), String> {
    let url = validate_download_url(url)?;
    let response = state
        .client
        .get(url)
        .timeout(Duration::from_secs(180))
        .send()
        .await
        .map_err(|error| format!("下载生成结果失败：{error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "下载生成结果失败（HTTP {}）",
            response.status().as_u16()
        ));
    }
    let maximum = if kind == "image" {
        MAX_IMAGE_BYTES
    } else {
        MAX_VIDEO_BYTES
    };
    if response
        .content_length()
        .is_some_and(|length| length > maximum)
    {
        return Err("生成结果超过本地安全大小限制".into());
    }
    let root =
        asset_root(database, project)?.join(if kind == "image" { "images" } else { "videos" });
    tokio::fs::create_dir_all(&root)
        .await
        .map_err(|error| error.to_string())?;
    let temporary = root.join(format!(".download-{}", Uuid::new_v4()));
    let mut file = tokio::fs::File::create(&temporary)
        .await
        .map_err(|error| error.to_string())?;
    let mut stream = response.bytes_stream();
    let mut total = 0_u64;
    let mut digest = Sha256::new();
    let mut header = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| format!("下载生成结果失败：{error}"))?;
        total = total.saturating_add(chunk.len() as u64);
        if total > maximum {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err("生成结果超过本地安全大小限制".into());
        }
        if header.len() < 64 {
            let take = (64 - header.len()).min(chunk.len());
            header.extend_from_slice(&chunk[..take]);
        }
        digest.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|error| error.to_string())?;
    }
    file.sync_all().await.map_err(|error| error.to_string())?;
    drop(file);
    if total == 0 {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err("供应商返回了空文件".into());
    }
    let (extension, mime) = if kind == "image" {
        match image_format(&header) {
            Ok(format) => format,
            Err(error) => {
                let _ = tokio::fs::remove_file(&temporary).await;
                return Err(error);
            }
        }
    } else {
        if header.len() < 12 || &header[4..8] != b"ftyp" {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err("生成结果不是有效的 MP4/MOV 视频".into());
        }
        if output_format == "mov" {
            ("mov", "video/quicktime")
        } else {
            ("mp4", "video/mp4")
        }
    };
    let hash = format!("{:x}", digest.finalize());
    let file_name = format!("{hash}.{extension}");
    let target = root.join(&file_name);
    if target.exists() {
        tokio::fs::remove_file(&temporary)
            .await
            .map_err(|error| error.to_string())?;
    } else {
        tokio::fs::rename(&temporary, &target)
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok((
        format!("simple-media:{hash}"),
        format!(
            "{}/{file_name}",
            if kind == "image" { "images" } else { "videos" }
        ),
        mime.to_owned(),
    ))
}

fn image_reference_data_url(
    database: &Path,
    project: &Path,
    reference: &str,
) -> Result<String, String> {
    if reference.starts_with("simple-image:") {
        let bytes = image_attachments::read(database, project, reference).map_err(command_error)?;
        let (_, mime) = image_format(&bytes)?;
        return Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)));
    }
    if reference.starts_with("simple-media:") {
        let asset = find_media_asset(&asset_root(database, project)?, reference)?;
        if asset.info.kind != "image" {
            return Err("只能使用图片作为参考图或视频首帧".into());
        }
        let metadata = fs::metadata(&asset.path).map_err(|error| error.to_string())?;
        if metadata.len() > MAX_IMAGE_BYTES {
            return Err("媒体资产超过读取限制".into());
        }
        return image_data_url(&asset.path);
    }
    Err("参考图必须是当前项目的 simple-image 或 simple-media 图片引用".into())
}

#[tauri::command]
pub async fn load_media_settings(
    media: State<'_, MediaGenerationState>,
) -> Result<MediaSettings, String> {
    let _guard = media.write_lock.lock().await;
    let config = load_store(&media.file)?.config;
    Ok(MediaSettings {
        base_url: config.base_url,
        image_model: config.image_model,
        video_model: config.video_model,
        has_credential: has_credential(),
    })
}

#[tauri::command]
pub async fn save_media_settings(
    input: SaveMediaSettingsInput,
    media: State<'_, MediaGenerationState>,
) -> Result<MediaSettings, String> {
    let config = StoredConfig {
        base_url: validate_base_url(&input.base_url)?,
        image_model: validate_model(&input.image_model, "seedream")?,
        video_model: validate_model(&input.video_model, "seedance")?,
    };
    if let Some(key) = input
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
    {
        if key.len() > 4096 {
            return Err("API Key 长度异常".into());
        }
        SystemSecretStore
            .set(CREDENTIAL_REF, key)
            .map_err(|error| error.to_string())?;
    } else if !has_credential() {
        return Err("请输入火山方舟 API Key".into());
    }
    let _guard = media.write_lock.lock().await;
    mutate_store(&media.file, |store| {
        store.config = config.clone();
        Ok(())
    })?;
    Ok(MediaSettings {
        base_url: config.base_url,
        image_model: config.image_model,
        video_model: config.video_model,
        has_credential: true,
    })
}

#[tauri::command]
pub async fn list_media_jobs(
    project_id: String,
    desktop: State<'_, DesktopState>,
    media: State<'_, MediaGenerationState>,
) -> Result<Vec<MediaJob>, String> {
    project_context(&desktop, &project_id)?;
    let _guard = media.write_lock.lock().await;
    Ok(load_store(&media.file)?
        .jobs
        .into_iter()
        .filter(|job| job.project_id == project_id)
        .collect())
}

#[tauri::command]
pub async fn generate_image(
    input: GenerateImageInput,
    desktop: State<'_, DesktopState>,
    media: State<'_, MediaGenerationState>,
) -> Result<MediaJob, String> {
    let prompt = validate_prompt(&input.prompt)?;
    if !matches!(input.size.as_str(), "1K" | "1.5K" | "2K") {
        return Err("图片尺寸只支持 1K、1.5K 或 2K".into());
    }
    let (database, project) = project_context(&desktop, &input.project_id)?;
    let (config, api_key) = {
        let _guard = media.write_lock.lock().await;
        (load_store(&media.file)?.config, credential()?)
    };
    let model = validate_model(
        input.model.as_deref().unwrap_or(&config.image_model),
        "seedream",
    )?;
    let id = Uuid::new_v4().to_string();
    insert_job(
        &media,
        MediaJob {
            id: id.clone(),
            project_id: input.project_id.clone(),
            kind: "image".into(),
            model: model.clone(),
            status: "submitting".into(),
            prompt: prompt.clone(),
            created_at_ms: now_ms()?,
            updated_at_ms: now_ms()?,
            provider_task_id: None,
            asset_ref: None,
            asset_path: None,
            mime_type: None,
            error: None,
            usage_tokens: None,
        },
    )
    .await?;
    let mut body = json!({
        "model": model,
        "prompt": prompt,
        "size": input.size,
        "output_format": "png",
        "response_format": "url",
        "watermark": input.watermark
    });
    if let Some(reference) = input.reference.as_deref() {
        body["image"] = Value::String(image_reference_data_url(&database, &project, reference)?);
    }
    let endpoint = format!(
        "{}/images/generations",
        validate_base_url(&config.base_url)?
    );
    let response = match media
        .client
        .post(endpoint)
        .bearer_auth(api_key)
        .json(&body)
        .timeout(Duration::from_secs(180))
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            let message = format!("图片请求结果无法确认，不会自动重试：{error}");
            let _ = update_job(&media, &id, |job| {
                job.status = "uncertain".into();
                job.error = Some(message.clone());
            })
            .await;
            return Err(message);
        }
    };
    let value = match provider_json(response).await {
        Ok(value) => value,
        Err(error) => {
            let _ = update_job(&media, &id, |job| {
                job.status = "failed".into();
                job.error = Some(error.clone());
            })
            .await;
            return Err(error);
        }
    };
    let url = match value.pointer("/data/0/url").and_then(Value::as_str) {
        Some(url) => url,
        None => {
            let message = "图片接口没有返回下载地址".to_owned();
            let _ = update_job(&media, &id, |job| {
                job.status = "failed".into();
                job.error = Some(message.clone());
            })
            .await;
            return Err(message);
        }
    };
    let (asset_ref, asset_path, mime_type) =
        match download_asset(&media, &database, &project, url, "image", "png").await {
            Ok(asset) => asset,
            Err(error) => {
                let _ = update_job(&media, &id, |job| {
                    job.status = "failed".into();
                    job.error = Some(error.clone());
                })
                .await;
                return Err(error);
            }
        };
    let usage = value
        .pointer("/usage/completion_tokens")
        .or_else(|| value.pointer("/usage/output_tokens"))
        .and_then(Value::as_u64);
    update_job(&media, &id, |job| {
        job.status = "succeeded".into();
        job.asset_ref = Some(asset_ref);
        job.asset_path = Some(asset_path);
        job.mime_type = Some(mime_type);
        job.usage_tokens = usage;
    })
    .await
}

#[tauri::command]
pub async fn create_video(
    input: CreateVideoInput,
    desktop: State<'_, DesktopState>,
    media: State<'_, MediaGenerationState>,
) -> Result<MediaJob, String> {
    let prompt = validate_prompt(&input.prompt)?;
    if !matches!(input.resolution.as_str(), "480p" | "720p" | "1080p") {
        return Err("视频清晰度只支持 480p、720p 或 1080p".into());
    }
    if !matches!(
        input.ratio.as_str(),
        "16:9" | "9:16" | "1:1" | "4:3" | "3:4" | "21:9" | "adaptive"
    ) {
        return Err("视频画幅无效".into());
    }
    if !(4..=30).contains(&input.duration) {
        return Err("视频时长必须在 4–30 秒之间".into());
    }
    let (database, project) = project_context(&desktop, &input.project_id)?;
    let (config, api_key) = {
        let _guard = media.write_lock.lock().await;
        (load_store(&media.file)?.config, credential()?)
    };
    let model = validate_model(
        input.model.as_deref().unwrap_or(&config.video_model),
        "seedance",
    )?;
    if model.contains("seedance-2-5") && input.first_frame.is_some() && input.ratio != "adaptive" {
        return Err("Seedance 2.5 使用首帧时，画幅必须选择自适应".into());
    }
    if model.contains("seedance-2-0") && input.duration > 15 {
        return Err("Seedance 2.0 系列只支持最长 15 秒".into());
    }
    if model.contains("2-0-mini") || model.contains("2-0-fast") {
        if input.resolution == "1080p" {
            return Err("Seedance 2.0 mini/fast 只支持最长 15 秒及最高 720p".into());
        }
    }
    let id = Uuid::new_v4().to_string();
    insert_job(
        &media,
        MediaJob {
            id: id.clone(),
            project_id: input.project_id.clone(),
            kind: "video".into(),
            model: model.clone(),
            status: "submitting".into(),
            prompt: prompt.clone(),
            created_at_ms: now_ms()?,
            updated_at_ms: now_ms()?,
            provider_task_id: None,
            asset_ref: None,
            asset_path: None,
            mime_type: None,
            error: None,
            usage_tokens: None,
        },
    )
    .await?;
    let mut content = vec![json!({"type":"text", "text":prompt})];
    if let Some(reference) = input.first_frame.as_deref() {
        content.push(json!({
            "type":"image_url",
            "image_url":{"url":image_reference_data_url(&database, &project, reference)?},
            "role":"first_frame"
        }));
    }
    let body = json!({
        "model": model,
        "content": content,
        "resolution": input.resolution,
        "ratio": input.ratio,
        "duration": input.duration,
        "generate_audio": input.generate_audio,
        "output_format": "mp4",
        "watermark": input.watermark
    });
    let endpoint = format!(
        "{}/contents/generations/tasks",
        validate_base_url(&config.base_url)?
    );
    let response = match media
        .client
        .post(endpoint)
        .bearer_auth(api_key)
        .json(&body)
        .timeout(Duration::from_secs(90))
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            let message = format!("视频任务提交结果无法确认，不会自动重试：{error}");
            let _ = update_job(&media, &id, |job| {
                job.status = "uncertain".into();
                job.error = Some(message.clone());
            })
            .await;
            return Err(message);
        }
    };
    let value = match provider_json(response).await {
        Ok(value) => value,
        Err(error) => {
            let _ = update_job(&media, &id, |job| {
                job.status = "failed".into();
                job.error = Some(error.clone());
            })
            .await;
            return Err(error);
        }
    };
    let provider_id = match value
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        Some(provider_id) => provider_id.to_owned(),
        None => {
            let message = "视频接口没有返回任务 ID".to_owned();
            let _ = update_job(&media, &id, |job| {
                job.status = "failed".into();
                job.error = Some(message.clone());
            })
            .await;
            return Err(message);
        }
    };
    update_job(&media, &id, |job| {
        job.status = "queued".into();
        job.provider_task_id = Some(provider_id);
    })
    .await
}

#[tauri::command]
pub async fn refresh_video(
    job_id: String,
    desktop: State<'_, DesktopState>,
    media: State<'_, MediaGenerationState>,
) -> Result<MediaJob, String> {
    let job = get_job(&media, &job_id).await?;
    if job.kind != "video" {
        return Err("该任务不是视频任务".into());
    }
    if matches!(job.status.as_str(), "failed" | "cancelled" | "expired") {
        return Ok(job);
    }
    if job.status == "succeeded" && job.asset_ref.is_some() {
        return Ok(job);
    }
    let provider_id = job
        .provider_task_id
        .as_deref()
        .ok_or("视频任务缺少供应商任务 ID")?;
    let (database, project) = project_context(&desktop, &job.project_id)?;
    let (config, api_key) = {
        let _guard = media.write_lock.lock().await;
        (load_store(&media.file)?.config, credential()?)
    };
    let endpoint = format!(
        "{}/contents/generations/tasks/{provider_id}",
        validate_base_url(&config.base_url)?
    );
    let value = provider_json(
        media
            .client
            .get(endpoint)
            .bearer_auth(api_key)
            .timeout(Duration::from_secs(45))
            .send()
            .await
            .map_err(|error| format!("查询视频任务失败：{error}"))?,
    )
    .await?;
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("running");
    let status = match status {
        "queued" | "running" | "succeeded" | "failed" | "cancelled" | "expired" => status,
        _ => "running",
    };
    let provider_error = value
        .pointer("/error/message")
        .and_then(Value::as_str)
        .map(|message| message.chars().take(400).collect::<String>());
    let usage = value
        .pointer("/usage/completion_tokens")
        .and_then(Value::as_u64);
    let mut current = update_job(&media, &job_id, |job| {
        job.status = status.to_owned();
        job.error = provider_error;
        job.usage_tokens = usage;
    })
    .await?;
    if status == "succeeded" && current.asset_ref.is_none() {
        let Some(url) = value.pointer("/content/video_url").and_then(Value::as_str) else {
            return update_job(&media, &job_id, |job| {
                job.error = Some("视频已完成，但接口没有返回下载地址".into())
            })
            .await;
        };
        match download_asset(&media, &database, &project, url, "video", "mp4").await {
            Ok((asset_ref, asset_path, mime_type)) => {
                current = update_job(&media, &job_id, |job| {
                    job.asset_ref = Some(asset_ref);
                    job.asset_path = Some(asset_path);
                    job.mime_type = Some(mime_type);
                    job.error = None;
                })
                .await?;
            }
            Err(error) => {
                current = update_job(&media, &job_id, |job| {
                    job.error = Some(format!("视频已经生成，但本地下载失败：{error}"));
                })
                .await?;
            }
        }
    }
    Ok(current)
}

#[tauri::command]
pub async fn cancel_video(
    job_id: String,
    media: State<'_, MediaGenerationState>,
) -> Result<MediaJob, String> {
    let job = get_job(&media, &job_id).await?;
    if job.status != "queued" {
        return Err("只有仍在排队的视频任务可以远程取消".into());
    }
    let provider_id = job
        .provider_task_id
        .as_deref()
        .ok_or("视频任务缺少供应商任务 ID")?;
    let (config, api_key) = {
        let _guard = media.write_lock.lock().await;
        (load_store(&media.file)?.config, credential()?)
    };
    let endpoint = format!(
        "{}/contents/generations/tasks/{provider_id}",
        validate_base_url(&config.base_url)?
    );
    let response = media
        .client
        .delete(endpoint)
        .bearer_auth(api_key)
        .timeout(Duration::from_secs(45))
        .send()
        .await
        .map_err(|error| format!("取消视频任务失败：{error}"))?;
    if response.status() != StatusCode::NO_CONTENT && !response.status().is_success() {
        return provider_json(response).await.map(|_| job);
    }
    update_job(&media, &job_id, |job| job.status = "cancelled".into()).await
}

fn safe_asset_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let candidate = root.join(relative);
    let root = fs::canonicalize(root).map_err(|error| error.to_string())?;
    let candidate = fs::canonicalize(candidate).map_err(|error| error.to_string())?;
    if !candidate.starts_with(&root) || !candidate.is_file() {
        return Err("媒体资产路径越界或不存在".into());
    }
    Ok(candidate)
}

fn media_reference_hash(reference: &str) -> Result<&str, String> {
    let hash = reference
        .strip_prefix("simple-media:")
        .ok_or("媒体引用格式无效")?;
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("媒体引用格式无效".into());
    }
    Ok(hash)
}

struct ResolvedMediaAsset {
    path: PathBuf,
    info: MediaAssetInfo,
}

fn find_media_asset(root: &Path, reference: &str) -> Result<ResolvedMediaAsset, String> {
    let hash = media_reference_hash(reference)?;
    let mut matches = Vec::new();
    for (directory, extension, kind, mime_type) in [
        ("images", "png", "image", "image/png"),
        ("images", "jpg", "image", "image/jpeg"),
        ("images", "webp", "image", "image/webp"),
        ("videos", "mp4", "video", "video/mp4"),
        ("videos", "mov", "video", "video/quicktime"),
    ] {
        let relative = format!("{directory}/{hash}.{extension}");
        if root.join(&relative).is_file() {
            matches.push((relative, kind, mime_type));
        }
    }
    if matches.len() != 1 {
        return Err(if matches.is_empty() {
            "媒体资产不存在或不属于当前项目".into()
        } else {
            "媒体引用对应多个本地资产".into()
        });
    }
    let (relative, kind, mime_type) = matches.remove(0);
    Ok(ResolvedMediaAsset {
        path: safe_asset_path(root, &relative)?,
        info: MediaAssetInfo {
            reference: reference.to_owned(),
            kind: kind.to_owned(),
            mime_type: mime_type.to_owned(),
            width: None,
            height: None,
        },
    })
}

fn image_asset_info(mut asset: ResolvedMediaAsset) -> Result<ResolvedMediaAsset, String> {
    if asset.info.kind == "image" {
        let (width, height) = image::image_dimensions(&asset.path)
            .map_err(|error| format!("无法读取图片尺寸：{error}"))?;
        asset.info.width = Some(width);
        asset.info.height = Some(height);
    }
    Ok(asset)
}

async fn resolve_asset(
    project_id: &str,
    reference: &str,
    desktop: &DesktopState,
    _media: &MediaGenerationState,
) -> Result<(ResolvedMediaAsset, PathBuf), String> {
    let (database, project) = project_context(desktop, project_id)?;
    let asset = find_media_asset(&asset_root(&database, &project)?, reference)?;
    Ok((asset, project))
}

#[tauri::command]
pub async fn inspect_media_asset(
    project_id: String,
    reference: String,
    desktop: State<'_, DesktopState>,
    media: State<'_, MediaGenerationState>,
) -> Result<MediaAssetInfo, String> {
    resolve_asset(&project_id, &reference, &desktop, &media)
        .await
        .and_then(|(asset, _)| image_asset_info(asset))
        .map(|asset| asset.info)
}

#[tauri::command]
pub async fn read_media_asset(
    project_id: String,
    reference: String,
    desktop: State<'_, DesktopState>,
    media: State<'_, MediaGenerationState>,
) -> Result<tauri::ipc::Response, String> {
    let (asset, _) = resolve_asset(&project_id, &reference, &desktop, &media).await?;
    let metadata = fs::metadata(&asset.path).map_err(|error| error.to_string())?;
    let maximum = if asset.info.kind == "image" {
        MAX_IMAGE_BYTES
    } else {
        MAX_VIDEO_BYTES
    };
    if metadata.len() > maximum {
        return Err("媒体资产超过读取限制".into());
    }
    let bytes = tokio::fs::read(asset.path)
        .await
        .map_err(|error| error.to_string())?;
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
pub async fn read_media_image_data_url(
    project_id: String,
    reference: String,
    desktop: State<'_, DesktopState>,
    media: State<'_, MediaGenerationState>,
) -> Result<String, String> {
    let (asset, _) = resolve_asset(&project_id, &reference, &desktop, &media).await?;
    if asset.info.kind != "image" {
        return Err("媒体资产不是图片".into());
    }
    let metadata = fs::metadata(&asset.path).map_err(|error| error.to_string())?;
    if metadata.len() > MAX_IMAGE_BYTES {
        return Err("媒体资产超过读取限制".into());
    }
    image_data_url(&asset.path)
}

fn image_data_url(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let (_, mime_type) = image_format(&bytes)?;
    Ok(format!(
        "data:{mime_type};base64,{}",
        STANDARD.encode(bytes)
    ))
}

#[tauri::command]
pub async fn media_asset_as_attachment(
    project_id: String,
    reference: String,
    desktop: State<'_, DesktopState>,
    media: State<'_, MediaGenerationState>,
) -> Result<BackendAttachment, String> {
    let (asset, project) = resolve_asset(&project_id, &reference, &desktop, &media).await?;
    if asset.info.kind != "image" {
        return Err("只有生成图片可以加入对话".into());
    }
    let bytes = fs::read(asset.path).map_err(|error| error.to_string())?;
    let runtime = desktop.lock().map_err(command_error)?;
    image_attachments::save(&runtime.database_path, &project, "生成图片.png", &bytes)
        .map_err(command_error)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MediaToolRequest {
    action: String,
    prompt: Option<String>,
    reference: Option<String>,
    model: Option<String>,
    size: Option<String>,
    resolution: Option<String>,
    ratio: Option<String>,
    duration: Option<u32>,
    generate_audio: Option<bool>,
    watermark: Option<bool>,
    job_id: Option<String>,
}

fn live_tool_context(
    runtime: &DesktopRuntime,
    instance: &str,
    params: &Value,
) -> Result<(CodexTurnBinding, String), String> {
    let thread = params["threadId"].as_str().ok_or("缺少任务标识")?;
    let turn = params["turnId"].as_str().ok_or("缺少回合标识")?;
    let binding = runtime
        .codex_action_binding(thread, turn)
        .ok_or("媒体请求不属于当前任务")?;
    if !runtime.project_leases.contains_key(&binding.turn_id)
        || !runtime
            .codex_turn_owners
            .get(&binding.turn_id)
            .is_some_and(|owner| owner.instance_id == instance)
        || !runtime.core.snapshot().turns.iter().any(|turn| {
            turn.id.to_string() == binding.turn_id && turn.status == TurnStatus::Running
        })
    {
        return Err("任务已结束，媒体请求已取消".into());
    }
    let task_id = parse_task_id(&binding.task_id).map_err(command_error)?;
    let project_id = runtime
        .core
        .snapshot()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .map(|task| task.project_id.to_string())
        .ok_or("媒体请求找不到当前项目")?;
    Ok((binding, project_id))
}

fn media_event(
    runtime: &Arc<Mutex<DesktopRuntime>>,
    binding: &CodexTurnBinding,
    call_id: &str,
    action: &str,
    phase: &str,
    job: Option<&MediaJob>,
) -> Result<(), String> {
    let mut payload = json!({"call_id":call_id,"action":action});
    if let Some(job) = job {
        payload["job_id"] = json!(job.id);
        payload["status"] = json!(job.status);
        payload["asset_ref"] = json!(job.asset_ref);
    }
    runtime
        .lock()
        .map_err(|_| "任务状态不可用".to_owned())?
        .storage
        .append_event(NewEvent {
            event_id: Uuid::new_v4().to_string(),
            task_id: binding.task_id.clone(),
            turn_id: Some(binding.turn_id.clone()),
            event_type: format!("media_action_{phase}"),
            payload,
            created_at_ms: unix_time_ms().map_err(command_error)?,
        })
        .map(|_| ())
        .map_err(command_error)
}

fn media_result_text(job: &MediaJob) -> Result<String, String> {
    let reference = job.asset_ref.as_deref().ok_or_else(|| {
        job.error
            .clone()
            .unwrap_or_else(|| format!("媒体任务尚未完成（状态：{}）", job.status))
    })?;
    if job.kind == "image" {
        Ok(format!(
            "图片已生成并保存到本地。请在最终回答中原样单独输出：\n![生成图片]({reference})"
        ))
    } else {
        Ok(format!(
            "视频已生成并保存到本地。请在最终回答中原样单独输出：\n[生成视频]({reference})"
        ))
    }
}

async fn run_media_tool(
    app: &AppHandle,
    runtime: &Arc<Mutex<DesktopRuntime>>,
    client: &CodexKernelClient,
    params: &Value,
) -> Result<Value, String> {
    if params["tool"] != "simple_media"
        || params
            .get("namespace")
            .is_some_and(|value| !value.is_null())
    {
        return Err("不支持的媒体工具".into());
    }
    let request: MediaToolRequest = serde_json::from_value(params["arguments"].clone())
        .map_err(|_| "媒体工具参数无效".to_owned())?;
    let call_id = params["callId"].as_str().ok_or("缺少调用标识")?;
    let (binding, project_id) = {
        let state = runtime.lock().map_err(|_| "任务状态不可用")?;
        let context = live_tool_context(&state, client.instance_id(), params)?;
        if state
            .storage
            .load_events(&context.0.task_id)
            .map_err(command_error)?
            .iter()
            .any(|event| {
                event.turn_id.as_deref() == Some(context.0.turn_id.as_str())
                    && event.event_type == "media_action_claimed"
                    && event.payload["call_id"] == call_id
            })
        {
            return Err("该媒体请求已经处理；为避免重复计费，不会自动重试".into());
        }
        context
    };
    media_event(runtime, &binding, call_id, &request.action, "claimed", None)?;

    let desktop = app.state::<DesktopState>();
    let media = app.state::<MediaGenerationState>();
    let outcome = match request.action.as_str() {
        "generate_image" => {
            let prompt = request.prompt.ok_or("生成图片需要 prompt")?;
            let job = generate_image(
                GenerateImageInput {
                    project_id,
                    prompt,
                    model: request.model,
                    size: request.size.unwrap_or_else(|| "1K".into()),
                    reference: request.reference,
                    watermark: request.watermark.unwrap_or(false),
                },
                desktop,
                media,
            )
            .await?;
            let text = media_result_text(&job)?;
            Ok((job, text))
        }
        "generate_video" => {
            let prompt = request.prompt.ok_or("生成视频需要 prompt")?;
            let ratio = request.ratio.unwrap_or_else(|| {
                if request.reference.is_some() {
                    "adaptive".into()
                } else {
                    "16:9".into()
                }
            });
            let mut job = create_video(
                CreateVideoInput {
                    project_id,
                    prompt,
                    model: request.model,
                    resolution: request.resolution.unwrap_or_else(|| "720p".into()),
                    ratio,
                    duration: request.duration.unwrap_or(5),
                    first_frame: request.reference,
                    generate_audio: request.generate_audio.unwrap_or(false),
                    watermark: request.watermark.unwrap_or(false),
                },
                desktop.clone(),
                media.clone(),
            )
            .await?;
            let deadline = Instant::now() + Duration::from_secs(20 * 60);
            while matches!(job.status.as_str(), "queued" | "running") && Instant::now() < deadline {
                tokio::time::sleep(Duration::from_secs(10)).await;
                job = refresh_video(job.id.clone(), desktop.clone(), media.clone()).await?;
            }
            let text = if job.asset_ref.is_some() {
                media_result_text(&job)?
            } else if matches!(job.status.as_str(), "queued" | "running") {
                format!(
                    "视频任务仍在生成，任务 ID 为 {}。不要重新提交；稍后用 check_video 查询。",
                    job.id
                )
            } else {
                return Err(job
                    .error
                    .clone()
                    .unwrap_or_else(|| format!("视频生成失败（状态：{}）", job.status)));
            };
            Ok((job, text))
        }
        "check_video" => {
            let job_id = request.job_id.ok_or("查询视频需要 job_id")?;
            let existing = get_job(&media, &job_id).await?;
            if existing.project_id != project_id || existing.kind != "video" {
                return Err("视频任务不存在或不属于当前项目".into());
            }
            let job = refresh_video(job_id, desktop, media).await?;
            let text = if job.asset_ref.is_some() {
                media_result_text(&job)?
            } else {
                format!("视频任务当前状态：{}。任务 ID：{}。", job.status, job.id)
            };
            Ok((job, text))
        }
        _ => Err("不支持的媒体操作".into()),
    };

    match outcome {
        Ok((job, text)) => {
            media_event(
                runtime,
                &binding,
                call_id,
                &request.action,
                "completed",
                Some(&job),
            )?;
            Ok(json!({
                "success": true,
                "contentItems": [{"type":"inputText","text":text}]
            }))
        }
        Err(error) => {
            let _ = media_event(runtime, &binding, call_id, &request.action, "failed", None);
            Err(error)
        }
    }
}

pub(super) async fn handle_tool(
    app: AppHandle,
    runtime: Arc<Mutex<DesktopRuntime>>,
    client: CodexKernelClient,
    id: Value,
    params: Value,
) {
    let response = run_media_tool(&app, &runtime, &client, &params)
        .await
        .unwrap_or_else(|error| {
            let message = if error.contains("API Key") || error.contains("配置") {
                format!("{error}。请在 Simple 设置中打开“图片与视频设置”完成一次配置。")
            } else {
                error
            };
            json!({"success":false,"contentItems":[{"type":"inputText","text":message}]})
        });
    let _ = client.respond(id, response).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image_job(id: &str, status: &str, updated_at_ms: i64, with_asset: bool) -> MediaJob {
        let hash = "a".repeat(64);
        MediaJob {
            id: id.into(),
            project_id: "project-1".into(),
            kind: "image".into(),
            model: DEFAULT_IMAGE_MODEL.into(),
            status: status.into(),
            prompt: "test".into(),
            created_at_ms: 1,
            updated_at_ms,
            provider_task_id: None,
            asset_ref: with_asset.then(|| format!("simple-media:{hash}")),
            asset_path: with_asset.then(|| format!("images/{hash}.png")),
            mime_type: with_asset.then(|| "image/png".into()),
            error: None,
            usage_tokens: with_asset.then_some(1),
        }
    }

    #[test]
    fn validates_provider_boundaries() {
        assert!(validate_base_url(DEFAULT_BASE_URL).is_ok());
        assert!(validate_base_url("http://127.0.0.1:8080/api/v3").is_ok());
        assert!(validate_base_url("http://example.com/api/v3").is_err());
        assert!(validate_base_url("https://user:secret@example.com").is_err());
        assert!(validate_download_url("file:///etc/passwd").is_err());
        assert!(validate_model(DEFAULT_IMAGE_MODEL, "seedream").is_ok());
        assert!(validate_model("../../bad", "seedream").is_err());
    }

    #[test]
    fn state_round_trip_preserves_jobs_without_credentials() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let file = directory.path().join("state.json");
        let mut store = MediaStore {
            version: STORE_VERSION,
            ..MediaStore::default()
        };
        store.jobs.push(image_job("job-1", "succeeded", 2, true));
        save_store(&file, &store).expect("state save");
        let restored = load_store(&file).expect("state load");
        assert_eq!(restored.jobs.len(), 1);
        let text = fs::read_to_string(file).expect("state text");
        assert!(!text.contains("ark-"));
    }

    #[test]
    fn state_load_recovers_jobs_from_newer_backup() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let file = directory.path().join("state.json");
        let current = MediaStore {
            version: STORE_VERSION,
            ..MediaStore::default()
        };
        let mut backup = current.clone();
        backup
            .jobs
            .push(image_job("recovered", "succeeded", 5, true));
        fs::write(&file, serde_json::to_vec_pretty(&current).unwrap()).unwrap();
        fs::write(
            file.with_extension("previous.json"),
            serde_json::to_vec_pretty(&backup).unwrap(),
        )
        .unwrap();

        let restored = load_store(&file).expect("state recovery");
        assert_eq!(restored.jobs.len(), 1);
        assert_eq!(restored.jobs[0].id, "recovered");
        assert!(restored.jobs[0].asset_ref.is_some());
    }

    #[test]
    fn project_scoped_asset_can_render_without_job_index() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path().join("project-scope");
        let hash = "b".repeat(64);
        let image = root.join("images").join(format!("{hash}.png"));
        fs::create_dir_all(image.parent().unwrap()).unwrap();
        fs::write(&image, b"fixture image bytes").unwrap();

        let asset = find_media_asset(&root, &format!("simple-media:{hash}"))
            .expect("orphaned asset should remain renderable");
        assert_eq!(asset.path, fs::canonicalize(image).unwrap());
        assert_eq!(asset.info.kind, "image");
        assert_eq!(asset.info.mime_type, "image/png");
        assert!(
            find_media_asset(
                &directory.path().join("different-project"),
                &format!("simple-media:{hash}")
            )
            .is_err()
        );
        assert!(find_media_asset(&root, "simple-media:../escape").is_err());
    }

    #[test]
    fn generated_png_is_returned_as_a_browser_decodable_data_url() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let hash = "c".repeat(64);
        let image = directory.path().join("images").join(format!("{hash}.png"));
        fs::create_dir_all(image.parent().unwrap()).expect("image directory");
        let bytes = STANDARD
            .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=")
            .expect("fixture PNG");
        fs::write(&image, bytes).expect("write fixture PNG");

        let data_url = image_data_url(&image).expect("image data URL");
        assert!(data_url.starts_with("data:image/png;base64,"));
        assert!(!data_url.contains(char::is_whitespace));
        let asset = find_media_asset(directory.path(), &format!("simple-media:{hash}"))
            .and_then(image_asset_info)
            .expect("image metadata");
        assert_eq!(asset.info.width, Some(1));
        assert_eq!(asset.info.height, Some(1));
    }

    #[test]
    fn generated_image_can_be_reused_as_a_project_scoped_reference() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("local-agent.db");
        let project = directory.path().join("project-a");
        let other_project = directory.path().join("project-b");
        fs::create_dir_all(&project).expect("project directory");
        fs::create_dir_all(&other_project).expect("other project directory");

        let hash = "d".repeat(64);
        let reference = format!("simple-media:{hash}");
        let image = asset_root(&database, &project)
            .expect("asset root")
            .join("images")
            .join(format!("{hash}.png"));
        fs::create_dir_all(image.parent().unwrap()).expect("image directory");
        let bytes = STANDARD
            .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=")
            .expect("fixture PNG");
        fs::write(&image, bytes).expect("write fixture PNG");

        let data_url = image_reference_data_url(&database, &project, &reference)
            .expect("generated image reference");
        assert!(data_url.starts_with("data:image/png;base64,"));
        assert!(
            image_reference_data_url(&database, &other_project, &reference).is_err(),
            "a generated image must not cross project boundaries"
        );
    }

    #[test]
    fn generated_video_cannot_be_used_as_an_image_reference() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("local-agent.db");
        let project = directory.path().join("project");
        fs::create_dir_all(&project).expect("project directory");

        let hash = "e".repeat(64);
        let reference = format!("simple-media:{hash}");
        let video = asset_root(&database, &project)
            .expect("asset root")
            .join("videos")
            .join(format!("{hash}.mp4"));
        fs::create_dir_all(video.parent().unwrap()).expect("video directory");
        fs::write(video, b"fixture video bytes").expect("write fixture video");

        let error = image_reference_data_url(&database, &project, &reference)
            .expect_err("video references must be rejected");
        assert!(error.contains("只能使用图片"));
        assert!(image_reference_data_url(&database, &project, "simple-media:../escape").is_err());
    }

    #[test]
    fn media_result_uses_only_opaque_local_references() {
        let mut job = image_job("job-1", "succeeded", 2, true);
        job.prompt = "private prompt".into();
        let image = media_result_text(&job).expect("image result");
        assert!(image.contains("![生成图片](simple-media:"));
        assert!(!image.contains("private prompt"));
        job.kind = "video".into();
        assert!(
            media_result_text(&job)
                .expect("video result")
                .contains("[生成视频](simple-media:")
        );
    }
}
