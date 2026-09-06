use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use uuid::Uuid;

const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
const RETAINED_LOGS: usize = 5;
const MAX_FIELD_TEXT_CHARS: usize = 8_192;

struct Logger {
    path: PathBuf,
    session_id: String,
}

static LOGGER: OnceLock<Logger> = OnceLock::new();
static LOG_WRITE: Mutex<()> = Mutex::new(());

pub fn initialize(path: PathBuf) {
    let _ = LOGGER.set(Logger {
        path,
        session_id: Uuid::new_v4().to_string(),
    });
    install_panic_hook();
    info(
        "backend_started",
        json!({
            "app_version": env!("CARGO_PKG_VERSION"),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "process_id": std::process::id()
        }),
    );
}

pub fn session_id() -> String {
    LOGGER.get().map_or_else(
        || "uninitialized".to_owned(),
        |logger| logger.session_id.clone(),
    )
}

pub fn info(name: &str, fields: Value) {
    write("info", name, fields);
}

pub fn warn(name: &str, fields: Value) {
    write("warn", name, fields);
}

pub fn error(name: &str, fields: Value) {
    write("error", name, fields);
}

pub fn sanitize_value(value: Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .map(|(key, value)| {
                    if is_sensitive_key(&key) {
                        (key, Value::String("[REDACTED]".to_owned()))
                    } else {
                        (key, sanitize_value(value))
                    }
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.into_iter().map(sanitize_value).collect()),
        Value::String(value) => Value::String(sanitize_text(&value)),
        other => other,
    }
}

pub fn sanitize_text(value: &str) -> String {
    let mut result = String::new();
    let mut private_key = false;
    for line in value.lines() {
        let lower = line.to_ascii_lowercase();
        if lower.contains("-----begin ") && lower.contains("private key-----") {
            private_key = true;
            push_line(&mut result, "[REDACTED PRIVATE KEY]");
            continue;
        }
        if private_key {
            if lower.contains("-----end ") && lower.contains("private key-----") {
                private_key = false;
            }
            continue;
        }
        push_line(&mut result, &redact_line(line));
        if result.chars().count() >= MAX_FIELD_TEXT_CHARS {
            result = result.chars().take(MAX_FIELD_TEXT_CHARS).collect();
            result.push_str("…[TRUNCATED]");
            break;
        }
    }
    result.trim_end_matches('\n').to_owned()
}

pub fn read_records() -> io::Result<Vec<Value>> {
    let Some(logger) = LOGGER.get() else {
        return Ok(Vec::new());
    };
    let mut records = Vec::new();
    for path in log_paths_oldest_first(&logger.path) {
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        for line in BufReader::new(file).lines() {
            let line = line?;
            if let Ok(value) = serde_json::from_str::<Value>(&line) {
                records.push(sanitize_value(value));
            }
        }
    }
    Ok(records)
}

fn write(level: &str, name: &str, fields: Value) {
    let timestamp_ms = unix_time_ms();
    let Some(logger) = LOGGER.get() else {
        eprintln!("{timestamp_ms} {level} {name}");
        return;
    };
    let record = json!({
        "timestamp_ms": timestamp_ms,
        "level": level,
        "event": sanitize_event_name(name),
        "session_id": logger.session_id,
        "fields": sanitize_value(fields)
    });
    let Ok(line) = serde_json::to_string(&record) else {
        return;
    };
    eprintln!("{line}");
    let Ok(_guard) = LOG_WRITE.lock() else {
        return;
    };
    if rotate_if_needed(&logger.path, line.len() as u64 + 1).is_err() {
        return;
    }
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&logger.path)
    {
        let _ = writeln!(file, "{line}");
    }
}

fn rotate_if_needed(path: &Path, incoming_bytes: u64) -> io::Result<()> {
    let current = fs::metadata(path).map_or(0, |metadata| metadata.len());
    if current.saturating_add(incoming_bytes) <= MAX_LOG_BYTES {
        return Ok(());
    }
    let oldest = archived_path(path, RETAINED_LOGS);
    if oldest.exists() {
        fs::remove_file(oldest)?;
    }
    for index in (1..RETAINED_LOGS).rev() {
        let source = archived_path(path, index);
        if source.exists() {
            fs::rename(source, archived_path(path, index + 1))?;
        }
    }
    if path.exists() {
        fs::rename(path, archived_path(path, 1))?;
    }
    Ok(())
}

fn archived_path(path: &Path, index: usize) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("local-agent.jsonl");
    path.with_file_name(format!("{name}.{index}"))
}

fn log_paths_oldest_first(path: &Path) -> Vec<PathBuf> {
    let mut paths = (1..=RETAINED_LOGS)
        .rev()
        .map(|index| archived_path(path, index))
        .collect::<Vec<_>>();
    paths.push(path.to_owned());
    paths
}

fn install_panic_hook() {
    std::panic::set_hook(Box::new(|panic| {
        let message = panic
            .payload()
            .downcast_ref::<&str>()
            .map(|value| (*value).to_owned())
            .or_else(|| panic.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "non-string panic payload".to_owned());
        let location = panic.location().map(|location| {
            format!(
                "{}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            )
        });
        error(
            "process_panic",
            json!({ "message": message, "location": location }),
        );
    }));
}

fn sanitize_event_name(name: &str) -> String {
    name.chars()
        .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
        .take(96)
        .collect()
}

fn is_sensitive_key(key: &str) -> bool {
    let compact = key
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    matches!(
        compact.as_str(),
        "apikey"
            | "authorization"
            | "credential"
            | "credentialref"
            | "password"
            | "privatekey"
            | "secret"
            | "token"
            | "accesstoken"
            | "refreshtoken"
            | "idtoken"
            | "cookie"
    ) || compact.ends_with("apikey")
}

fn redact_line(line: &str) -> String {
    let lower = line.to_ascii_lowercase();
    if let Some(index) = lower.find("bearer ") {
        let prefix = &line[..index + "bearer ".len()];
        return format!("{prefix}[REDACTED]");
    }
    for marker in [
        "api_key",
        "api-key",
        "apikey",
        "authorization",
        "password",
        "private_key",
        "secret",
        "access_token",
        "refresh_token",
    ] {
        if let Some(index) = lower.find(marker) {
            let remainder = &line[index + marker.len()..];
            if let Some(separator) = remainder.find(['=', ':']) {
                let end = index + marker.len() + separator + 1;
                return format!("{} [REDACTED]", &line[..end]);
            }
        }
    }
    line.to_owned()
}

fn push_line(target: &mut String, line: &str) {
    target.push_str(line);
    target.push('\n');
}

fn unix_time_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{
        MAX_LOG_BYTES, archived_path, redact_line, rotate_if_needed, sanitize_text, sanitize_value,
    };
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn recursively_redacts_sensitive_keys() {
        let sanitized = sanitize_value(json!({
            "apiKey": "sk-secret",
            "nested": { "authorization": "Bearer secret", "safe": "kept" }
        }));
        assert_eq!(sanitized["apiKey"], "[REDACTED]");
        assert_eq!(sanitized["nested"]["authorization"], "[REDACTED]");
        assert_eq!(sanitized["nested"]["safe"], "kept");
    }

    #[test]
    fn redacts_bearer_values_and_private_keys() {
        assert_eq!(
            redact_line("Authorization: Bearer abc123"),
            "Authorization: Bearer [REDACTED]"
        );
        let private_key = "-----BEGIN PRIVATE KEY-----\nsecret\n-----END PRIVATE KEY-----\nafter";
        assert_eq!(sanitize_text(private_key), "[REDACTED PRIVATE KEY]\nafter");
    }

    #[test]
    fn configured_log_limit_is_bounded() {
        assert_eq!(MAX_LOG_BYTES, 5 * 1024 * 1024);
    }

    #[test]
    fn rotates_a_full_log_before_appending() {
        let directory = tempdir().expect("temporary log directory");
        let path = directory.path().join("local-agent.jsonl");
        fs::write(&path, vec![b'x'; MAX_LOG_BYTES as usize]).expect("full test log");
        rotate_if_needed(&path, 1).expect("log rotation");
        assert!(!path.exists());
        assert_eq!(
            fs::metadata(archived_path(&path, 1))
                .expect("rotated log")
                .len(),
            MAX_LOG_BYTES
        );
    }

    #[test]
    fn token_usage_is_not_treated_as_a_credential() {
        let sanitized = sanitize_value(json!({ "input_tokens": 1200, "token": "secret" }));
        assert_eq!(sanitized["input_tokens"], 1200);
        assert_eq!(sanitized["token"], "[REDACTED]");
    }
}
