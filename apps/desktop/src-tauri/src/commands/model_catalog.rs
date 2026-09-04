/// 返回 OpenRouter 全量模型目录（缓存刷新后读取）。
#[tauri::command]
pub async fn list_model_catalog(
    force_refresh: bool,
) -> Result<Vec<crate::meta::openrouter_meta::ModelCatalogEntry>, String> {
    crate::meta::openrouter_meta::ensure_cache(force_refresh).await?;
    Ok(crate::meta::openrouter_meta::all_entries())
}
