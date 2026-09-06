use std::collections::HashMap;
use std::env;

pub fn normalize_null_device_env(env_map: &mut HashMap<String, String>) {
    let keys: Vec<String> = env_map.keys().cloned().collect();
    for key in keys {
        if let Some(value) = env_map.get(&key).cloned() {
            let normalized = value.trim().to_ascii_lowercase();
            if normalized == "/dev/null" || normalized == "\\\\\\\\dev\\\\\\\\null" {
                env_map.insert(key, "NUL".to_owned());
            }
        }
    }
}

pub fn ensure_non_interactive_pager(env_map: &mut HashMap<String, String>) {
    env_map
        .entry("GIT_PAGER".to_owned())
        .or_insert_with(|| "more.com".to_owned());
    env_map
        .entry("PAGER".to_owned())
        .or_insert_with(|| "more.com".to_owned());
    env_map.entry("LESS".to_owned()).or_default();
}

pub fn inherit_path_env(env_map: &mut HashMap<String, String>) {
    if !env_map.contains_key("PATH")
        && let Ok(path) = env::var("PATH")
    {
        env_map.insert("PATH".to_owned(), path);
    }
    if !env_map.contains_key("PATHEXT")
        && let Ok(pathext) = env::var("PATHEXT")
    {
        env_map.insert("PATHEXT".to_owned(), pathext);
    }
}
