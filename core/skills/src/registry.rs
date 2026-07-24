//! 运行时 Skill 注册表：按名称索引已加载的 [`LoadedSkill`]。

use std::collections::HashMap;

use crate::skill::LoadedSkill;

/// 内存中的 Skill 字典（非磁盘扫描器；扫描见 `installed`）。
pub struct SkillRegistry {
    /// name → skill。
    skills: HashMap<String, LoadedSkill>,
}

impl SkillRegistry {
    /// 创建空注册表。
    pub fn new() -> Self {
        SkillRegistry {
            skills: HashMap::new(),
        }
    }

    /// 按 `metadata.name` 插入或覆盖。
    pub fn register(&mut self, skill: LoadedSkill) {
        self.skills.insert(skill.metadata.name.clone(), skill);
    }

    /// 列出全部已注册 Skill。
    pub fn list(&self) -> Vec<&LoadedSkill> {
        self.skills.values().collect()
    }

    /// 按名称查找。
    pub fn get(&self, name: &str) -> Option<&LoadedSkill> {
        self.skills.get(name)
    }
}

impl Default for SkillRegistry {
    /// 等价于 [`SkillRegistry::new`]。
    fn default() -> Self {
        Self::new()
    }
}
