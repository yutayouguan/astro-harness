//! 动态 dispatch wrapper — 将静态泛型 `CompletionModel` 包装为 `dyn` trait object。
//!
//! 运行时注册表需要按 string id 查找 provider，无法用泛型，
//! 因此通过 `DynCompletionModel` 擦除类型。

use anyhow::Result;
use async_trait::async_trait;

use crate::types::{CompletionRequest, CompletionStream};
use super::models::CompletionModel;

/// 类型擦除的补全模型（用于注册表动态 dispatch）。
#[async_trait]
pub trait DynCompletionModel: Send + Sync {
    async fn stream(&self, request: CompletionRequest) -> Result<CompletionStream>;
    fn clone_box(&self) -> Box<dyn DynCompletionModel>;
}

#[async_trait]
impl<M: CompletionModel + Clone + 'static> DynCompletionModel for M {
    async fn stream(&self, request: CompletionRequest) -> Result<CompletionStream> {
        CompletionModel::stream(self, request).await
    }

    fn clone_box(&self) -> Box<dyn DynCompletionModel> {
        Box::new(self.clone())
    }
}

impl Clone for Box<dyn DynCompletionModel> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

/// 动态 provider entry — 注册表中的一项。
#[derive(Clone)]
pub struct DynProvider {
    pub id: String,
    pub name: &'static str,
    completion: Option<Box<dyn DynCompletionModel>>,
}

impl DynProvider {
    pub fn new(id: impl Into<String>, name: &'static str) -> Self {
        Self {
            id: id.into(),
            name,
            completion: None,
        }
    }

    pub fn with_completion(mut self, model: impl CompletionModel + Clone + 'static) -> Self {
        self.completion = Some(Box::new(model));
        self
    }

    pub fn completion_model(&self) -> Option<&dyn DynCompletionModel> {
        self.completion.as_deref()
    }

    pub fn has_completion(&self) -> bool {
        self.completion.is_some()
    }
}

impl std::fmt::Debug for DynProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DynProvider")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("has_completion", &self.has_completion())
            .finish()
    }
}
