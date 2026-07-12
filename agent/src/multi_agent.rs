//! 多子 Agent 并行编排。
//!
//! 将父任务拆为若干 `SubTask`，在并发上限内并行执行并收集 `SubAgentResult`。
//! 当前实现为占位逻辑（固定返回描述摘要），后续可接入真实子 Agent 循环。

use tokio::task::JoinSet;

/// 编排器全局约束：并发子 Agent 数量与单任务最大轮次。
pub struct OrchestratorConfig {
    /// 同时运行的子 Agent 上限；超出部分被截断不调度。
    pub max_sub_agents: usize,
    /// 每个子 Agent 允许的最大对话轮次（供未来真实执行使用）。
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
    /// 实际消耗的对话轮次。
    pub turns_used: usize,
}

/// 单个子 Agent 运行时配置。
pub struct SubAgentConfig {
    /// 该子 Agent 的最大轮次；默认 50。
    pub max_turns: usize,
}

impl Default for SubAgentConfig {
    /// 返回 `max_turns = 50` 的默认配置。
    fn default() -> Self { SubAgentConfig { max_turns: 50 } }
}

/// 子任务调度器；持有 `OrchestratorConfig` 并在 `dispatch_parallel` 中 spawn 异步任务。
pub struct Orchestrator {
    /// 编排约束，控制并发与轮次上限。
    config: OrchestratorConfig,
}

impl Orchestrator {
    /// 根据配置构造编排器。
    pub fn new(config: OrchestratorConfig) -> Self { Orchestrator { config } }

    /// 并行派发子任务，最多 `max_sub_agents` 个；全部完成后返回结果列表。
    ///
    /// 任务顺序与完成顺序无关；单个任务 panic 时该条结果会被跳过。
    ///
    /// # 参数
    ///
    /// - `tasks`：待执行子任务列表，超出并发上限的尾部任务被丢弃。
    ///
    /// # 返回
    ///
    /// 成功完成的 `SubAgentResult` 集合（长度 ≤ `max_sub_agents`）。
    pub async fn dispatch_parallel(&self, tasks: Vec<SubTask>) -> Vec<SubAgentResult> {
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
            if let Ok(r) = result { results.push(r); }
        }
        results
    }
}
