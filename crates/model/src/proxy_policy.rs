//! Keep Simple's loopback transport local without disabling external proxies.
use std::ffi::{OsStr, OsString};
use std::net::IpAddr;
use std::process::Command;

pub(crate) fn is_loopback_endpoint(url: &reqwest::Url) -> bool {
    url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .trim_start_matches('[')
                .trim_end_matches(']')
                .parse::<IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    })
}

pub(crate) fn preserve_local_gateway_routing(command: &mut Command) {
    let mut bypass = OsString::from("127.0.0.1,localhost,::1");
    let mut retained = Vec::new();
    for name in ["NO_PROXY", "no_proxy"] {
        let value = match command.get_envs().find(|(key, _)| {
            if cfg!(windows) {
                key.to_string_lossy().eq_ignore_ascii_case(name)
            } else {
                *key == OsStr::new(name)
            }
        }) {
            Some((_, value)) => value.map(OsStr::to_os_string),
            None => std::env::var_os(name),
        };
        if let Some(value) = value.filter(|value| !value.is_empty() && !retained.contains(value)) {
            bypass.push(",");
            bypass.push(&value);
            retained.push(value);
        }
    }
    command.env("NO_PROXY", &bypass).env("no_proxy", &bypass);
}

#[cfg(test)]
#[path = "proxy_policy_tests.rs"]
mod tests;
