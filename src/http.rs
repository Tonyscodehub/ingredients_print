//! Minimal HTTPS GET using Windows' own WinHTTP (system proxy and certificates).

use crate::label::wide;
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::Networking::WinHttp::*;

struct H(*mut c_void);
impl Drop for H {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

fn fail(what: &str) -> String {
    let code = unsafe { GetLastError() };
    let hint = match code {
        12007 => " (couldn't find the server – is the PC online?)",
        12002 => " (timed out)",
        12029 | 12030 => " (couldn't connect – is the PC online?)",
        12175 | 12045 | 12044 => " (secure connection problem)",
        _ => "",
    };
    format!("{what} failed, error {code}{hint}")
}

/// Returns (HTTP status, body).
pub fn get(host: &str, path: &str, user_agent: &str) -> Result<(u32, String), String> {
    unsafe {
        let agent = wide(user_agent);
        // 4 = WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY (Windows 8.1+); fall back to 0 = default proxy.
        let mut session = H(WinHttpOpen(agent.as_ptr(), 4, null(), null(), 0));
        if session.0.is_null() {
            session = H(WinHttpOpen(agent.as_ptr(), 0, null(), null(), 0));
        }
        if session.0.is_null() {
            return Err(fail("starting the internet connection"));
        }
        WinHttpSetTimeouts(session.0, 10_000, 10_000, 15_000, 20_000);
        let host_w = wide(host);
        let conn = H(WinHttpConnect(session.0, host_w.as_ptr(), 443, 0));
        if conn.0.is_null() {
            return Err(fail("connecting"));
        }
        let verb = wide("GET");
        let path_w = wide(path);
        let req = H(WinHttpOpenRequest(conn.0, verb.as_ptr(), path_w.as_ptr(), null(), null(), null(), 0x0080_0000 /* WINHTTP_FLAG_SECURE */));
        if req.0.is_null() {
            return Err(fail("opening the request"));
        }
        let headers = wide("Accept: application/json\r\n");
        if WinHttpSendRequest(req.0, headers.as_ptr(), u32::MAX /* -1: null-terminated */, null(), 0, 0, 0) == 0 {
            return Err(fail("sending the request"));
        }
        if WinHttpReceiveResponse(req.0, null_mut()) == 0 {
            return Err(fail("getting a reply"));
        }
        let mut status: u32 = 0;
        let mut len: u32 = 4;
        WinHttpQueryHeaders(
            req.0,
            19 | 0x2000_0000, // WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER
            null(),
            &mut status as *mut u32 as *mut c_void,
            &mut len,
            null_mut(),
        );
        let mut body = Vec::new();
        loop {
            let mut avail: u32 = 0;
            if WinHttpQueryDataAvailable(req.0, &mut avail) == 0 {
                return Err(fail("reading the reply"));
            }
            if avail == 0 {
                break;
            }
            let start = body.len();
            body.resize(start + avail as usize, 0u8);
            let mut read: u32 = 0;
            if WinHttpReadData(req.0, body[start..].as_mut_ptr() as *mut c_void, avail, &mut read) == 0 {
                return Err(fail("reading the reply"));
            }
            body.truncate(start + read as usize);
            if body.len() > 4_000_000 {
                return Err("reply was unexpectedly large".into());
            }
        }
        Ok((status, String::from_utf8_lossy(&body).into_owned()))
    }
}

/// Looks a barcode up on Open Food Facts.
pub fn lookup_barcode(barcode: &str) -> Result<Option<crate::lookup::Found>, String> {
    let (status, body) = get(crate::lookup::OFF_HOST, &crate::lookup::off_path(barcode), crate::lookup::USER_AGENT)?;
    match status {
        200 => crate::lookup::parse_off(&body),
        404 => Ok(None),
        429 => Err("Open Food Facts is busy – try again in a minute".into()),
        s => Err(format!("Open Food Facts replied with error {s}")),
    }
}
