use std::ffi::c_void;

use anyhow::Result;
use windows_sys::Win32::Foundation::{HLOCAL, LocalFree};

use crate::token::convert_string_sid_to_sid;

/// Owns a SID allocated by `ConvertStringSidToSidW`.
pub struct LocalSid {
    psid: *mut c_void,
}

impl LocalSid {
    pub fn from_string(sid: &str) -> Result<Self> {
        let psid = unsafe { convert_string_sid_to_sid(sid) }
            .ok_or_else(|| anyhow::anyhow!("invalid SID string: {sid}"))?;
        Ok(Self { psid })
    }

    pub fn as_ptr(&self) -> *mut c_void {
        self.psid
    }
}

impl Drop for LocalSid {
    fn drop(&mut self) {
        if !self.psid.is_null() {
            unsafe {
                LocalFree(self.psid as HLOCAL);
            }
        }
    }
}
