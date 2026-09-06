use std::path::Path;

use crate::install_wfp_filters_for_account;

pub fn install_wfp_filters<F>(_sandbox_home: &Path, offline_username: &str, mut log: F)
where
    F: FnMut(&str),
{
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        install_wfp_filters_for_account(offline_username)
    })) {
        Ok(Ok(installed)) => log(&format!(
            "WFP setup succeeded for {offline_username} with {installed} installed filters"
        )),
        Ok(Err(error)) => log(&format!(
            "WFP setup failed for {offline_username}: {error}; continuing elevated setup"
        )),
        Err(_) => log(&format!(
            "WFP setup panicked for {offline_username}; continuing elevated setup"
        )),
    }
}
