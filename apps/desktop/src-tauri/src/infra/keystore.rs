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
    // A fresh Entry proves persistence; mock backends otherwise report success
    // while retaining the secret only on the now-discarded Entry object.
    anyhow::ensure!(
        load_api_key(service).as_deref() == Some(key),
        "保存后无法从系统凭证存储重新读取密钥，请检查钥匙串是否可用"
    );
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
    #[cfg(debug_assertions)]
    let acceptance_root = option_env!("ASTRO_NATIVE_ACCEPTANCE_ROOT");
    #[cfg(not(debug_assertions))]
    let acceptance_root = None;
    provider_service_name(provider_id, acceptance_root)
}

fn provider_service_name(provider_id: &str, acceptance_root: Option<&str>) -> String {
    if let Some(root) = acceptance_root {
        use sha2::{Digest, Sha256};
        let digest = format!("{:x}", Sha256::digest(root.as_bytes()));
        return format!("astro.qa.{}.provider.{provider_id}", &digest[..16]);
    }
    format!("astro.provider.{provider_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_build_uses_persistent_native_keychain() {
        use keyring::credential::CredentialPersistence;
        assert!(matches!(
            keyring::default::default_credential_builder().persistence(),
            CredentialPersistence::UntilDelete
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "writes and removes one uniquely named dummy credential in the local Keychain"]
    fn native_keychain_round_trip_for_isolated_dummy_credential() {
        let service = format!("astro.qa.keystore-test.{}", uuid::Uuid::new_v4());
        let dummy = "local-native-qa-not-a-real-api-key";
        let saved = save_api_key(&service, dummy);
        let read_back = load_api_key(&service);
        let removed = delete_api_key(&service);
        assert!(saved.is_ok(), "native keychain save failed");
        assert!(
            read_back.as_deref() == Some(dummy),
            "native keychain read-back failed"
        );
        assert!(removed.is_ok(), "test credential cleanup failed");
        assert!(!has_api_key(&service));
    }

    #[test]
    fn qa_keychains_are_separate_from_normal_app_and_other_qa_runs() {
        let normal = provider_service_name("openai", None);
        assert_eq!(normal, "astro.provider.openai");
        let qa = provider_service_name("openai", Some("/qa/run-one/home"));
        assert_ne!(qa, normal);
        assert_ne!(
            qa,
            provider_service_name("openai", Some("/qa/run-two/home"))
        );
        assert_eq!(
            qa,
            provider_service_name("openai", Some("/qa/run-one/home"))
        );
        assert!(!qa.contains("/qa/run-one/home"));
    }
}
