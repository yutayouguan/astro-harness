//! Per-request cancellation. IDs stay reserved until the generating future has stopped.
use std::{
    collections::{HashMap, VecDeque},
    sync::{Mutex, OnceLock},
};
use tokio::sync::watch;

#[derive(Default)]
struct Requests {
    active: HashMap<String, watch::Sender<bool>>,
    cancelled: VecDeque<String>,
}
static REQUESTS: OnceLock<Mutex<Requests>> = OnceLock::new();
const CANCELLED: &str = "桌宠生成已取消；已提交给模型的请求可能仍计费";
pub(super) struct PetGeneration {
    id: String,
    receiver: watch::Receiver<bool>,
}

impl PetGeneration {
    pub fn start(id: &str) -> Result<Self, String> {
        if id.is_empty()
            || id.len() > 100
            || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err("生成请求 id 无效".into());
        }
        let mut requests = REQUESTS
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| "生成锁不可用")?;
        if requests.active.contains_key(id) {
            return Err("生成请求 id 已在使用".into());
        }
        if let Some(index) = requests.cancelled.iter().position(|key| key == id) {
            requests.cancelled.remove(index);
            return Err(CANCELLED.into());
        }
        let (sender, receiver) = watch::channel(false);
        requests.active.insert(id.into(), sender);
        Ok(Self {
            id: id.into(),
            receiver,
        })
    }
    pub fn check(&self) -> Result<(), String> {
        if *self.receiver.borrow() {
            Err(CANCELLED.into())
        } else {
            Ok(())
        }
    }
    pub async fn run<T>(
        &mut self,
        future: impl std::future::Future<Output = Result<T, String>>,
    ) -> Result<T, String> {
        self.check()?;
        tokio::select! { biased;
            _ = self.receiver.changed() => Err(CANCELLED.into()),
            result = future => result,
        }
    }
}
impl Drop for PetGeneration {
    fn drop(&mut self) {
        if let Ok(mut requests) = REQUESTS.get_or_init(Default::default).lock() {
            requests.active.remove(&self.id);
        }
    }
}

#[tauri::command]
pub fn cancel_pet_generation(request_id: String) -> Result<bool, String> {
    if request_id.is_empty()
        || request_id.len() > 100
        || !request_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err("生成请求 id 无效".into());
    }
    let mut requests = REQUESTS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "生成锁不可用")?;
    if let Some(request) = requests.active.get(&request_id) {
        return Ok(request.send(true).is_ok());
    }
    // Cancel may arrive before the generation IPC is dispatched. Keep bounded tombstones.
    if !requests.cancelled.contains(&request_id) {
        if requests.cancelled.len() >= 256 {
            requests.cancelled.pop_front();
        }
        requests.cancelled.push_back(request_id);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn pet_generation_cancel_is_scoped_and_reserves_id_until_done() {
        let mut first = PetGeneration::start("test-first").unwrap();
        let other = PetGeneration::start("test-other").unwrap();
        assert!(cancel_pet_generation("test-first".into()).unwrap());
        assert!(first.run(async { Ok(42) }).await.is_err());
        assert!(other.check().is_ok());
        assert!(PetGeneration::start("test-first").is_err());
        drop(first);
        assert!(cancel_pet_generation("test-before-start".into()).unwrap());
        assert!(PetGeneration::start("test-before-start").is_err());
    }
}
