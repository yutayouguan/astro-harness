//! Windows Restricted Token 创建（v3 沙箱增强）。
//!
//! 使用 `CreateRestrictedToken` 创建受限令牌，通过 Capability SID 控制写入权限：
//! - `WRITE_RESTRICTED`: 仅在写操作时检查 restricting SID
//! - `DISABLE_MAX_PRIVILEGE`: 剥离所有特权
//! - `LUA_TOKEN`: 创建有限（非管理员）令牌
//!
//! 注意：本模块仅在 `target_os = "windows"` 下编译。

#[cfg(target_os = "windows")]
use std::io;

/// 生成随机 Capability SID 字符串（格式 `S-1-5-21-{a}-{b}-{c}-{d}`）。
///
/// 这不是真实 Windows 账户，仅作为 ACL 和 Restricted Token 的合成标识符。
pub fn generate_capability_sid() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let a = (seed & 0xFFFF_FFFF) as u32;
    let b = ((seed >> 32) & 0xFFFF_FFFF) as u32;
    let c = ((seed >> 64) & 0xFFFF_FFFF) as u32;
    let d = ((seed >> 96) & 0xFFFF_FFFF) as u32 ^ 0xCAFE_BABE;
    format!("S-1-5-21-{a}-{b}-{c}-{d}")
}

/// 创建受限令牌。
///
/// 使用当前进程令牌作为基础，剥离特权并添加 restricting SID。
/// 返回的令牌仅在写操作时检查 restricting SID（`WRITE_RESTRICTED`）。
///
/// # Safety
/// 调用方负责通过 `CloseHandle` 关闭返回的句柄。
#[cfg(target_os = "windows")]
pub fn create_restricted_token(
    restricting_sids: &[Vec<u8>],
) -> io::Result<windows_sys::Win32::Foundation::HANDLE> {
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::Security::*;

    unsafe {
        let mut base_token: HANDLE = 0;
        let result = OpenProcessToken(
            windows_sys::Win32::System::Threading::GetCurrentProcess(),
            TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_ASSIGN_PRIMARY | TOKEN_ADJUST_PRIVILEGES,
            &mut base_token,
        );
        if result == 0 {
            return Err(io::Error::last_os_error());
        }

        let mut sid_attrs: Vec<SID_AND_ATTRIBUTES> = restricting_sids
            .iter()
            .map(|sid| SID_AND_ATTRIBUTES {
                Sid: sid.as_ptr() as *mut _,
                Attributes: 0,
            })
            .collect();

        let mut restricted_token: HANDLE = 0;
        // DISABLE_MAX_PRIVILEGE(1) | LUA_TOKEN(4) | WRITE_RESTRICTED(8)
        let flags: u32 = 1 | 4 | 8;

        let result = CreateRestrictedToken(
            base_token,
            flags,
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
            sid_attrs.len() as u32,
            if sid_attrs.is_empty() {
                std::ptr::null_mut()
            } else {
                sid_attrs.as_mut_ptr()
            },
            &mut restricted_token,
        );

        CloseHandle(base_token);

        if result == 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(restricted_token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_sid_has_correct_format() {
        let sid = generate_capability_sid();
        assert!(sid.starts_with("S-1-5-21-"), "bad format: {sid}");
        let parts: Vec<&str> = sid.split('-').collect();
        assert_eq!(parts.len(), 7, "expected 7 parts: {sid}");
    }

    #[test]
    fn two_capability_sids_are_different() {
        let a = generate_capability_sid();
        std::thread::sleep(std::time::Duration::from_millis(1));
        let b = generate_capability_sid();
        assert_ne!(a, b);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn creates_restricted_token_with_empty_sids() {
        let token = create_restricted_token(&[]).unwrap();
        assert!(!token.is_null());
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(token);
        }
    }
}
