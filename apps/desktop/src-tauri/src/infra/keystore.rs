//! 系统钥匙串：按 Provider 存取 API Key。

use anyhow::{Context, Result};
use keyring::Entry;

/// keyring 账户名（各服务共用）。
const APP_NAME: &str = "astro";

/// 将 API Key 写入钥匙串（`service` 通常为 [`keyring_service_for_provider`]）。
pub fn save_api_key(service: &str, key: &str) -> Result<()> {
    let entry =
        Entry::new(service, APP_NAME).with_context(|| format!("无法创建密钥链条目: {service}"))?;
    entry
        .set_password(key)
        .with_context(|| format!("无法保存 API Key: {service}"))?;
    Ok(())
}

/// 删除指定服务的 API Key。
pub fn delete_api_key(service: &str) -> Result<()> {
    let entry =
        Entry::new(service, APP_NAME).with_context(|| format!("无法访问密钥链: {service}"))?;
    entry
        .delete_credential()
        .with_context(|| format!("无法删除 API Key: {service}"))?;
    Ok(())
}

/// 读取 API Key；缺失或空白返回 `None`。
pub fn load_api_key(service: &str) -> Option<String> {
    Entry::new(service, APP_NAME)
        .ok()
        .and_then(|e| e.get_password().ok())
        .filter(|k| !k.trim().is_empty())
}

/// 是否已保存非空 API Key。
pub fn has_api_key(service: &str) -> bool {
    load_api_key(service).is_some()
}

/// Provider id → 钥匙串 service 名（`astro.provider.{id}`）。
pub fn keyring_service_for_provider(provider_id: &str) -> String {
    format!("astro.provider.{provider_id}")
}
