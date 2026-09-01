use std::collections::HashMap;

use anyhow::Result;

#[derive(Clone)]
pub struct RuntimeProviderConfig {
    pub backend_id: String,
    pub config: providers::ProviderConfig,
    pub image_model: String,
    pub video_model: String,
    pub tts_model: String,
    pub music_model: String,
}

impl std::fmt::Debug for RuntimeProviderConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeProviderConfig")
            .field("backend_id", &self.backend_id)
            .field("model", &self.config.model)
            .field("image_model", &self.image_model)
            .field("video_model", &self.video_model)
            .field("tts_model", &self.tts_model)
            .field("music_model", &self.music_model)
            .field("base_url", &self.config.base_url)
            .field("api_key", &"[REDACTED]")
            .finish()
    }
}

/// 工作流执行上下文：管理全局变量与各节点输出
#[derive(Debug, Clone, Default)]
pub struct VariableContext {
    global: HashMap<String, serde_json::Value>,
    node_outputs: HashMap<String, serde_json::Value>,
    provider_configs: HashMap<String, RuntimeProviderConfig>,
}

impl VariableContext {
    pub fn new(globals: HashMap<String, serde_json::Value>) -> Self {
        Self {
            global: globals,
            node_outputs: HashMap::new(),
            provider_configs: HashMap::new(),
        }
    }

    pub fn with_provider_configs(
        mut self,
        provider_configs: HashMap<String, RuntimeProviderConfig>,
    ) -> Self {
        self.provider_configs = provider_configs;
        self
    }

    pub fn provider_config(&self, provider_id: &str) -> Option<&RuntimeProviderConfig> {
        self.provider_configs.get(provider_id)
    }

    pub(crate) fn provider_configs(&self) -> HashMap<String, RuntimeProviderConfig> {
        self.provider_configs.clone()
    }

    pub fn set_node_output(&mut self, node_id: &str, output: serde_json::Value) {
        self.node_outputs.insert(node_id.to_string(), output);
    }

    pub fn get_node_output(&self, node_id: &str) -> Option<&serde_json::Value> {
        self.node_outputs.get(node_id)
    }

    pub fn snapshot_outputs(&self) -> serde_json::Value {
        serde_json::json!(self.node_outputs)
    }

    /// 解析变量引用路径，如 `node_id.field.nested`
    pub fn resolve(&self, path: &str) -> Option<serde_json::Value> {
        let parts: Vec<&str> = path.splitn(2, '.').collect();
        let (root_key, rest) = match parts.as_slice() {
            [key] => (*key, None),
            [key, rest] => (*key, Some(*rest)),
            _ => return None,
        };

        // 先查 node_outputs，再查 global
        let root_val = self
            .node_outputs
            .get(root_key)
            .or_else(|| self.global.get(root_key))?;

        match rest {
            None => Some(root_val.clone()),
            Some(field_path) => resolve_json_path(root_val, field_path),
        }
    }

    /// 对模板字符串执行 `{{var}}` 插值
    pub fn interpolate(&self, template: &str) -> String {
        let mut result = String::with_capacity(template.len());
        let mut chars = template.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '{' && chars.peek() == Some(&'{') {
                chars.next(); // consume second {
                let mut var_name = String::new();
                let mut closed = false;
                while let Some(c) = chars.next() {
                    if c == '}' && chars.peek() == Some(&'}') {
                        chars.next(); // consume second }
                        closed = true;
                        break;
                    }
                    var_name.push(c);
                }
                if closed {
                    let var_name = var_name.trim();
                    match self.resolve(var_name) {
                        Some(serde_json::Value::String(s)) => result.push_str(&s),
                        Some(v) => result.push_str(&v.to_string()),
                        None => {
                            result.push_str("{{");
                            result.push_str(var_name);
                            result.push_str("}}");
                        }
                    }
                } else {
                    result.push_str("{{");
                    result.push_str(&var_name);
                }
            } else {
                result.push(ch);
            }
        }
        result
    }

    /// 评估条件表达式，支持：
    /// - 比较: `{{status}} == "active"`, `{{count}} > 5`
    /// - 逻辑组合: `{{a}} == 1 && {{b}} == 2`, `{{x}} > 0 || {{y}} > 0`
    /// - 取反: `!{{flag}}`
    pub fn evaluate_condition(&self, expr: &str) -> Result<bool> {
        let expr = expr.trim();
        if expr.is_empty() {
            return Ok(true);
        }

        // 先尝试拆分 || （优先级最低）
        if let Some(pos) = find_operator_outside_braces(expr, "||") {
            let lhs = &expr[..pos];
            let rhs = &expr[pos + 2..];
            return Ok(self.evaluate_condition(lhs)? || self.evaluate_condition(rhs)?);
        }

        // 再拆分 &&
        if let Some(pos) = find_operator_outside_braces(expr, "&&") {
            let lhs = &expr[..pos];
            let rhs = &expr[pos + 2..];
            return Ok(self.evaluate_condition(lhs)? && self.evaluate_condition(rhs)?);
        }

        // 取反 !
        if let Some(stripped) = expr.strip_prefix('!') {
            return Ok(!self.evaluate_condition(stripped)?);
        }

        self.evaluate_single_condition(expr)
    }

    fn evaluate_single_condition(&self, expr: &str) -> Result<bool> {
        let expr = expr.trim();
        // 去掉外层括号
        if expr.starts_with('(') && expr.ends_with(')') {
            return self.evaluate_condition(&expr[1..expr.len() - 1]);
        }

        type CmpOp = (&'static str, fn(&str, &str) -> bool);
        let ops: &[CmpOp] = &[
            ("==", compare_eq),
            ("!=", compare_ne),
            (">=", compare_gte),
            ("<=", compare_lte),
            (">", compare_gt),
            ("<", compare_lt),
        ];

        for (op, cmp_fn) in ops {
            if let Some(pos) = find_operator_outside_braces(expr, op) {
                let lhs_raw = expr[..pos].trim();
                let rhs_raw = expr[pos + op.len()..].trim();
                let lhs = self.interpolate(lhs_raw);
                let rhs = self.interpolate(rhs_raw);
                let lhs = lhs.trim().trim_matches('"');
                let rhs = rhs.trim().trim_matches('"');
                return Ok(cmp_fn(lhs, rhs));
            }
        }

        // 无操作符：truthy 判断
        let resolved = self.interpolate(expr);
        let resolved = resolved.trim();
        Ok(!resolved.is_empty()
            && resolved != "false"
            && resolved != "null"
            && resolved != "0"
            && resolved != "\"\"")
    }

    /// 对 JSON Value 执行插值（递归处理字符串字段）
    pub fn interpolate_value(&self, value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::String(s) => serde_json::Value::String(self.interpolate(s)),
            serde_json::Value::Array(arr) => {
                serde_json::Value::Array(arr.iter().map(|v| self.interpolate_value(v)).collect())
            }
            serde_json::Value::Object(obj) => {
                let map = obj
                    .iter()
                    .map(|(k, v)| (k.clone(), self.interpolate_value(v)))
                    .collect();
                serde_json::Value::Object(map)
            }
            other => other.clone(),
        }
    }
}

/// 在字符串中查找操作符，跳过 `{{ }}`  和 `"..."` 内部
fn find_operator_outside_braces(s: &str, op: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let op_bytes = op.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // 跳过 {{ }} 模板
        if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            i += 2;
            while i + 1 < bytes.len() {
                if bytes[i] == b'}' && bytes[i + 1] == b'}' {
                    i += 2;
                    break;
                }
                i += 1;
            }
            continue;
        }
        // 跳过 "..." 引号字符串
        if bytes[i] == b'"' {
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                i += 1;
            }
            if i < bytes.len() {
                i += 1;
            }
            continue;
        }
        if i + op_bytes.len() <= bytes.len() && &bytes[i..i + op_bytes.len()] == op_bytes {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn resolve_json_path(val: &serde_json::Value, path: &str) -> Option<serde_json::Value> {
    let mut current = val;
    for part in path.split('.') {
        match current {
            serde_json::Value::Object(map) => {
                current = map.get(part)?;
            }
            serde_json::Value::Array(arr) => {
                let idx: usize = part.parse().ok()?;
                current = arr.get(idx)?;
            }
            _ => return None,
        }
    }
    Some(current.clone())
}

fn compare_eq(a: &str, b: &str) -> bool {
    a == b
}
fn compare_ne(a: &str, b: &str) -> bool {
    a != b
}
fn compare_gt(a: &str, b: &str) -> bool {
    match (a.parse::<f64>(), b.parse::<f64>()) {
        (Ok(a), Ok(b)) => a > b,
        _ => a > b,
    }
}
fn compare_lt(a: &str, b: &str) -> bool {
    match (a.parse::<f64>(), b.parse::<f64>()) {
        (Ok(a), Ok(b)) => a < b,
        _ => a < b,
    }
}
fn compare_gte(a: &str, b: &str) -> bool {
    match (a.parse::<f64>(), b.parse::<f64>()) {
        (Ok(a), Ok(b)) => a >= b,
        _ => a >= b,
    }
}
fn compare_lte(a: &str, b: &str) -> bool {
    match (a.parse::<f64>(), b.parse::<f64>()) {
        (Ok(a), Ok(b)) => a <= b,
        _ => a <= b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolation_basic() {
        let mut ctx = VariableContext::default();
        ctx.global.insert("name".into(), serde_json::json!("Alice"));
        ctx.set_node_output("n1", serde_json::json!({"count": 42}));

        assert_eq!(ctx.interpolate("Hello {{name}}!"), "Hello Alice!");
        assert_eq!(ctx.interpolate("count={{n1.count}}"), "count=42");
        assert_eq!(ctx.interpolate("{{missing}}"), "{{missing}}");
    }

    #[test]
    fn condition_eval() {
        let mut ctx = VariableContext::default();
        ctx.global
            .insert("status".into(), serde_json::json!("active"));
        ctx.global.insert("count".into(), serde_json::json!("10"));

        assert!(ctx.evaluate_condition("{{status}} == active").unwrap());
        assert!(!ctx.evaluate_condition("{{status}} == inactive").unwrap());
        assert!(ctx.evaluate_condition("{{count}} > 5").unwrap());
        assert!(!ctx.evaluate_condition("{{count}} < 5").unwrap());
        assert!(ctx.evaluate_condition("{{count}} >= 10").unwrap());
    }

    #[test]
    fn resolve_nested() {
        let mut ctx = VariableContext::default();
        ctx.set_node_output("api", serde_json::json!({"data": {"items": [1, 2, 3]}}));
        assert_eq!(ctx.resolve("api.data.items.1"), Some(serde_json::json!(2)));
    }

    #[test]
    fn runtime_provider_config_is_available_without_exposing_key_in_debug() {
        let runtime = RuntimeProviderConfig {
            backend_id: "azure".into(),
            config: providers::ProviderConfig {
                api_key: "super-secret".into(),
                model: "gpt-image-2".into(),
                ..providers::ProviderConfig::default()
            },
            image_model: "gpt-image-2".into(),
            video_model: String::new(),
            tts_model: String::new(),
            music_model: String::new(),
        };
        let ctx = VariableContext::default()
            .with_provider_configs(HashMap::from([("azure-record".into(), runtime)]));
        assert_eq!(
            ctx.provider_config("azure-record")
                .expect("runtime config")
                .backend_id,
            "azure"
        );
        let debug = format!("{ctx:?}");
        assert!(!debug.contains("super-secret"));
        assert!(debug.contains("[REDACTED]"));
    }
}
