//! 多子 Agent 并行编排：转调 [`crate::exec::delegate`]。
//!
//! 工具层 `multi_agent` 已改为串行 orchestration；本模块 API 供程序化并行派发。

use ::delegate::spawn::DelegateRunRequest;
use tokio::task::JoinSet;

use crate::exec::delegate as delegate_exec;

/// 编排器全局约束：并发子 Agent 数量与单任务最大轮次。
pub struct OrchestratorConfig {
    /// 同时运行的子 Agent 上限。
    pub max_sub_agents: usize,
    /// 每个子 Agent 允许的最大对话轮次（保留字段；执行器内固定上限）。
    pub sub_agent_max_turns: usize,
}

/// 待派发的原子子任务。
pub struct SubTask {
    /// 任务唯一标识，用于结果关联。
    pub id: String,
    /// 自然语言任务描述，将传给子 Agent。
    pub description: String,
}

/// 单个子 Agent 执行完毕后的汇总。
pub struct SubAgentResult {
    /// 对应 `SubTask::id`。
    pub task_id: String,
    /// 子 Agent 产出的文本结果。
    pub output: String,
    /// 实际消耗的对话轮次（真执行时估算为 1+）。
    pub turns_used: usize,
}

/// 单个子 Agent 运行时配置。
pub struct SubAgentConfig {
    /// 该子 Agent 的最大轮次；默认 50。
    pub max_turns: usize,
}

impl Default for SubAgentConfig {
    fn default() -> Self {
        SubAgentConfig { max_turns: 50 }
    }
}

/// 子任务调度器。
pub struct Orchestrator {
    config: OrchestratorConfig,
}

impl Orchestrator {
    pub fn new(config: OrchestratorConfig) -> Self {
        Orchestrator { config }
    }

    /// 使用父凭据并行派发子任务（真委派）。
    pub async fn dispatch_parallel(
        &self,
        creds: DelegateRunRequest,
        tasks: Vec<SubTask>,
    ) -> Vec<SubAgentResult> {
        let max = self.config.max_sub_agents.max(1);
        let descriptions: Vec<_> = tasks
            .into_iter()
            .take(max)
            .map(|t| (t.id, t.description))
            .collect();
        let mut creds = creds;
        creds.max_concurrent = max;
        let pairs = delegate_exec::run_subtasks_parallel(creds, descriptions).await;
        pairs
            .into_iter()
            .map(|(task_id, output)| SubAgentResult {
                task_id,
                output,
                turns_used: 1,
            })
            .collect()
    }

    /// 无凭据时的占位路径（仅测试）：JoinSet 返回描述回显。
    pub async fn dispatch_parallel_stub(&self, tasks: Vec<SubTask>) -> Vec<SubAgentResult> {
        let mut join_set = JoinSet::new();
        for task in tasks.into_iter().take(self.config.max_sub_agents) {
            join_set.spawn(async move {
                SubAgentResult {
                    task_id: task.id,
                    output: format!("完成: {}", task.description),
                    turns_used: 1,
                }
            });
        }
        let mut results = Vec::new();
        while let Some(result) = join_set.join_next().await {
            if let Ok(r) = result {
                results.push(r);
            }
        }
        results
    }
}
