use std::collections::HashMap;

use anyhow::Result;

/// 工作流执行上下文：管理全局变量与各节点输出
#[derive(Debug, Clone, Default)]
pub struct VariableContext {
    global: HashMap<String, serde_json::Value>,
    node_outputs: HashMap<String, serde_json::Value>,
}

impl VariableContext {
    pub fn new(globals: HashMap<String, serde_json::Value>) -> Self {
        Self {
            global: globals,
            node_outputs: HashMap::new(),
        }
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

    /// 评估简单条件表达式，如 `{{status}} == "active"` 或 `{{count}} > 5`
    pub fn evaluate_condition(&self, expr: &str) -> Result<bool> {
        let expr = expr.trim();
        if expr.is_empty() {
            return Ok(true);
        }

        // 先做变量插值
        let resolved = self.interpolate(expr);

        // 支持的操作符
        for (op, cmp_fn) in &[
            ("==", compare_eq as fn(&str, &str) -> bool),
            ("!=", compare_ne as fn(&str, &str) -> bool),
            (">=", compare_gte as fn(&str, &str) -> bool),
            ("<=", compare_lte as fn(&str, &str) -> bool),
            (">", compare_gt as fn(&str, &str) -> bool),
            ("<", compare_lt as fn(&str, &str) -> bool),
        ] {
            if let Some(pos) = resolved.find(op) {
                let lhs = resolved[..pos].trim().trim_matches('"');
                let rhs = resolved[pos + op.len()..].trim().trim_matches('"');
                return Ok(cmp_fn(lhs, rhs));
            }
        }

        // 无操作符：truthy 判断
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
            serde_json::Value::String(s) => {
                serde_json::Value::String(self.interpolate(s))
            }
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
        ctx.global.insert("status".into(), serde_json::json!("active"));
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
        ctx.set_node_output(
            "api",
            serde_json::json!({"data": {"items": [1, 2, 3]}}),
        );
        assert_eq!(ctx.resolve("api.data.items.1"), Some(serde_json::json!(2)));
    }
}
