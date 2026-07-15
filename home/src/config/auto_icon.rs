//! 新建 Agent 时按名称/职能自动选择 Lucide 风格 SVG 图标。

/// 单个可自动匹配的图标条目。
pub struct AutoLucideIcon {
    pub id: &'static str,
    pub keywords: &'static [&'static str],
    pub inner_svg: &'static str,
}

/// 候选图标表（keyword 匹配打分用）。
pub const AUTO_LUCIDE_ICONS: &[AutoLucideIcon] = &[
    AutoLucideIcon {
        id: "code-2",
        keywords: &["代码", "编程", "开发", "程序", "软件", "engineer", "developer", "coding", "程序员", "工程"],
        inner_svg: "<path d=\"m18 16 4-4-4-4\"></path><path d=\"m6 8-4 4 4 4\"></path><path d=\"m14.5 4-5 16\"></path>",
    },
    AutoLucideIcon {
        id: "terminal",
        keywords: &["cli", "命令行", "shell", "运维", "devops"],
        inner_svg: "<polyline points=\"4 17 10 11 4 5\"></polyline><line x1=\"12\" x2=\"20\" y1=\"19\" y2=\"19\"></line>",
    },
    AutoLucideIcon {
        id: "bug",
        keywords: &["调试", "测试", "qa", "bug", "修复"],
        inner_svg: "<path d=\"m8 2 1.88 1.88\"></path><path d=\"M14.12 3.88 16 2\"></path><path d=\"M9 7.13v-1a3.003 3.003 0 1 1 6 0v1\"></path><path d=\"M12 20c-3.3 0-6-2.7-6-6v-3a4 4 0 0 1 4-4h4a4 4 0 0 1 4 4v3c0 3.3-2.7 6-6 6\"></path><path d=\"M12 20v-9\"></path><path d=\"M6.53 9C4.6 8.8 3 7.1 3 5\"></path><path d=\"M6 13H2\"></path><path d=\"M3 21c0-2.1 1.7-3.9 3.8-4\"></path><path d=\"M20.97 5c0 2.1-1.6 3.8-3.5 4\"></path><path d=\"M22 13h-4\"></path><path d=\"M17.2 17c2.1.1 3.8 1.9 3.8 4\"></path>",
    },
    AutoLucideIcon {
        id: "cpu",
        keywords: &["ai", "算力", "模型", "ml", "llm", "机器学习"],
        inner_svg: "<rect width=\"16\" height=\"16\" x=\"4\" y=\"4\" rx=\"2\"></rect><rect width=\"6\" height=\"6\" x=\"9\" y=\"9\" rx=\"1\"></rect><path d=\"M15 2v2\"></path><path d=\"M15 20v2\"></path><path d=\"M2 15h2\"></path><path d=\"M2 9h2\"></path><path d=\"M20 15h2\"></path><path d=\"M20 9h2\"></path><path d=\"M9 2v2\"></path><path d=\"M9 20v2\"></path>",
    },
    AutoLucideIcon {
        id: "brain",
        keywords: &["思考", "智能", "推理", "策略", "顾问"],
        inner_svg: "<path d=\"M12 5a3 3 0 1 0-5.997.125 4 4 0 0 0-2.526 5.77 4 4 0 0 0 .556 6.588A4 4 0 1 0 12 18Z\"></path><path d=\"M12 5a3 3 0 1 1 5.997.125 4 4 0 0 1 2.526 5.77 4 4 0 0 1-.556 6.588A4 4 0 1 1 12 18Z\"></path><path d=\"M15 13a4.5 4.5 0 0 1-3-4 4.5 4.5 0 0 1-3 4\"></path><path d=\"M17.599 6.5a3 3 0 0 0 .399-1.375\"></path><path d=\"M6.003 5.125A3 3 0 0 0 6.401 6.5\"></path><path d=\"M3.477 10.896a4 4 0 0 1 .585-.396\"></path><path d=\"M19.938 10.5a4 4 0 0 1 .585.396\"></path><path d=\"M6 18a4 4 0 0 1-1.967-.516\"></path><path d=\"M19.967 17.484A4 4 0 0 1 18 18\"></path>",
    },
    AutoLucideIcon {
        id: "database",
        keywords: &["数据", "数据库", "sql", "存储", "etl"],
        inner_svg: "<ellipse cx=\"12\" cy=\"5\" rx=\"9\" ry=\"3\"></ellipse><path d=\"M3 5V19A9 3 0 0 0 21 19V5\"></path><path d=\"M3 12A9 3 0 0 0 21 12\"></path>",
    },
    AutoLucideIcon {
        id: "chart-column",
        keywords: &["分析", "数据分析", "统计", "报表", "bi", "洞察"],
        inner_svg: "<path d=\"M3 3v16a2 2 0 0 0 2 2h16\"></path><path d=\"M18 17V9\"></path><path d=\"M13 17V5\"></path><path d=\"M8 17v-3\"></path>",
    },
    AutoLucideIcon {
        id: "search",
        keywords: &["搜索", "检索", "知识库", "调研", "研究"],
        inner_svg: "<circle cx=\"11\" cy=\"11\" r=\"8\"></circle><path d=\"m21 21-4.3-4.3\"></path>",
    },
    AutoLucideIcon {
        id: "file-text",
        keywords: &["文稿", "文档", "写作", "文案", "内容"],
        inner_svg: "<path d=\"M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z\"></path><path d=\"M14 2v4a2 2 0 0 0 2 2h4\"></path><path d=\"M10 9H8\"></path><path d=\"M16 13H8\"></path><path d=\"M16 17H8\"></path>",
    },
    AutoLucideIcon {
        id: "pen-line",
        keywords: &["写作", "编辑", "撰稿", "编剧"],
        inner_svg: "<path d=\"M12 20h9\"></path><path d=\"M16.376 3.622a1 1 0 0 1 3.002 3.002L7.368 18.635a2 2 0 0 1-.855.506l-2.872.838a.5.5 0 0 1-.62-.62l.838-2.872a2 2 0 0 1 .506-.854z\"></path>",
    },
    AutoLucideIcon {
        id: "book-open",
        keywords: &["阅读", "图书", "资料", "知识"],
        inner_svg: "<path d=\"M12 7v14\"></path><path d=\"M3 18a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h5a4 4 0 0 1 4 4 4 4 0 0 1 4-4h5a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1h-6a3 3 0 0 0-3 3 3 3 0 0 0-3-3z\"></path>",
    },
    AutoLucideIcon {
        id: "graduation-cap",
        keywords: &["教育", "学习", "课程", "老师", "教学"],
        inner_svg: "<path d=\"M21.42 10.922a1 1 0 0 0-.019-1.838L12.83 5.18a2 2 0 0 0-1.66 0L2.6 9.08a1 1 0 0 0 0 1.832l8.57 3.908a2 2 0 0 0 1.66 0z\"></path><path d=\"M22 10v6\"></path><path d=\"M6 12.5V16a6 3 0 0 0 12 0v-3.5\"></path>",
    },
    AutoLucideIcon {
        id: "languages",
        keywords: &["翻译", "语言", "英语", "多语言"],
        inner_svg: "<path d=\"m5 8 6 6\"></path><path d=\"m4 14 6-6 2-3\"></path><path d=\"M2 5h12\"></path><path d=\"M7 2h1\"></path><path d=\"m22 22-5-10-5 10\"></path><path d=\"M14 18h6\"></path>",
    },
    AutoLucideIcon {
        id: "palette",
        keywords: &["设计", "美术", "ui", "ux", "视觉"],
        inner_svg: "<circle cx=\"13.5\" cy=\"6.5\" r=\".5\" fill=\"#2563eb\"></circle><circle cx=\"17.5\" cy=\"10.5\" r=\".5\" fill=\"#2563eb\"></circle><circle cx=\"8.5\" cy=\"7.5\" r=\".5\" fill=\"#2563eb\"></circle><circle cx=\"6.5\" cy=\"12.5\" r=\".5\" fill=\"#2563eb\"></circle><path d=\"M12 2C6.5 2 2 6.5 2 12s4.5 10 10 10c.926 0 1.648-.746 1.648-1.688 0-.437-.18-.835-.437-1.125-.29-.289-.438-.652-.438-1.125a1.64 1.64 0 0 1 1.668-1.668h1.996c3.051 0 5.555-2.503 5.555-5.554C21.965 6.012 17.461 2 12 2z\"></path>",
    },
    AutoLucideIcon {
        id: "camera",
        keywords: &["摄影", "拍摄", "照片"],
        inner_svg: "<path d=\"M14.5 4h-5L7 7H4a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-3l-2.5-3z\"></path><circle cx=\"12\" cy=\"13\" r=\"3\"></circle>",
    },
    AutoLucideIcon {
        id: "clapperboard",
        keywords: &["视频", "影视", "剪辑", "短视频"],
        inner_svg: "<path d=\"M20.2 6 3 11l-.9-2.4c-.3-1.1.3-2.2 1.3-2.5l13.5-4c1.1-.3 2.2.3 2.5 1.3Z\"></path><path d=\"m6.2 5.3 3.1 3.9\"></path><path d=\"m12.4 3.4 3.1 4\"></path><path d=\"M3 11h18v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z\"></path>",
    },
    AutoLucideIcon {
        id: "music",
        keywords: &["音乐", "音频", "播客"],
        inner_svg: "<path d=\"M9 18V5l12-2v13\"></path><circle cx=\"6\" cy=\"18\" r=\"3\"></circle><circle cx=\"18\" cy=\"16\" r=\"3\"></circle>",
    },
    AutoLucideIcon {
        id: "megaphone",
        keywords: &["营销", "传播", "推广", "公关"],
        inner_svg: "<path d=\"m3 11 18-5v12L3 14v-3z\"></path><path d=\"M11.6 16.8a3 3 0 1 1-5.8-1.6\"></path>",
    },
    AutoLucideIcon {
        id: "briefcase",
        keywords: &["商务", "职场", "办公", "助理"],
        inner_svg: "<path d=\"M16 20V4a2 2 0 0 0-2-2h-4a2 2 0 0 0-2 2v16\"></path><rect width=\"20\" height=\"14\" x=\"2\" y=\"6\" rx=\"2\"></rect>",
    },
    AutoLucideIcon {
        id: "presentation",
        keywords: &["演示", "汇报", "ppt", "演讲"],
        inner_svg: "<path d=\"M2 3h20\"></path><path d=\"M21 3v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V3\"></path><path d=\"m7 21 5-5 5 5\"></path>",
    },
    AutoLucideIcon {
        id: "users",
        keywords: &["团队", "协作", "人事", "hr"],
        inner_svg: "<path d=\"M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2\"></path><circle cx=\"9\" cy=\"7\" r=\"4\"></circle><path d=\"M22 21v-2a4 4 0 0 0-3-3.87\"></path><path d=\"M16 3.13a4 4 0 0 1 0 7.75\"></path>",
    },
    AutoLucideIcon {
        id: "message-circle",
        keywords: &["客服", "对话", "聊天", "社群"],
        inner_svg: "<path d=\"M7.9 20A9 9 0 1 0 4 16.1L2 22Z\"></path>",
    },
    AutoLucideIcon {
        id: "shopping-bag",
        keywords: &["电商", "购物", "零售", "带货"],
        inner_svg: "<path d=\"M6 2 3 6v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V6l-3-4Z\"></path><path d=\"M3 6h18\"></path><path d=\"M16 10a4 4 0 0 1-8 0\"></path>",
    },
    AutoLucideIcon {
        id: "scale",
        keywords: &["法律", "合规", "法务", "律师"],
        inner_svg: "<path d=\"m16 16 3-8 3 8c-.87.65-1.92 1-3 1s-2.13-.35-3-1Z\"></path><path d=\"m2 16 3-8 3 8c-.87.65-1.92 1-3 1s-2.13-.35-3-1Z\"></path><path d=\"M7 21h10\"></path><path d=\"M12 3v18\"></path><path d=\"M3 7h2c2 0 5-1 7-2 2 1 5 2 7 2h2\"></path>",
    },
    AutoLucideIcon {
        id: "stethoscope",
        keywords: &["医疗", "健康", "医生", "护理"],
        inner_svg: "<path d=\"M11 2v2\"></path><path d=\"M5 2v2\"></path><path d=\"M5 3H4a2 2 0 0 0-2 2v4a6 6 0 0 0 12 0V5a2 2 0 0 0-2-2h-1\"></path><path d=\"M8 15a6 6 0 0 0 12 0v-3\"></path><circle cx=\"20\" cy=\"10\" r=\"2\"></circle>",
    },
    AutoLucideIcon {
        id: "heart",
        keywords: &["情感", "心理", "陪伴"],
        inner_svg: "<path d=\"M19 14c1.49-1.46 3-3.21 3-5.5A5.5 5.5 0 0 0 16.5 3c-1.76 0-3 .5-4.5 2-1.5-1.5-2.74-2-4.5-2A5.5 5.5 0 0 0 2 8.5c0 2.3 1.5 4.05 3 5.5l7 7Z\"></path>",
    },
    AutoLucideIcon {
        id: "dumbbell",
        keywords: &["健身", "运动", "训练"],
        inner_svg: "<path d=\"M14.4 14.4 9.6 9.6\"></path><path d=\"M18.657 21.485a2 2 0 1 1-2.829-2.828l-1.767 1.768a2 2 0 1 1-2.829-2.829l6.364-6.364a2 2 0 1 1 2.829 2.829l-1.768 1.767a2 2 0 1 1 2.828 2.829z\"></path><path d=\"m21.5 21.5-1.4-1.4\"></path><path d=\"M3.9 3.9 2.5 2.5\"></path><path d=\"M6.404 12.768a2 2 0 1 1-2.829-2.829l1.768-1.767a2 2 0 1 1-2.828-2.829l2.828-2.828a2 2 0 1 1 2.829 2.828l1.767-1.768a2 2 0 1 1 2.829 2.829z\"></path>",
    },
    AutoLucideIcon {
        id: "leaf",
        keywords: &["环保", "自然", "植物"],
        inner_svg: "<path d=\"M11 20A7 7 0 0 1 9.8 6.1C15.5 5 17 4.48 19 2c1 2 2 4.18 2 8 0 5.5-4.78 10-10 10Z\"></path><path d=\"M2 21c0-3 1.85-5.36 5.08-6C9.5 14.52 12 13 13 12\"></path>",
    },
    AutoLucideIcon {
        id: "map",
        keywords: &["出行", "旅行", "地图", "导航"],
        inner_svg: "<path d=\"M14.106 5.553a2 2 0 0 0 1.788 0l3.659-1.83A1 1 0 0 1 21 4.619v12.764a1 1 0 0 1-.553.894l-4.553 2.277a2 2 0 0 1-1.788 0l-4.212-2.106a2 2 0 0 0-1.788 0l-3.659 1.83A1 1 0 0 1 3 19.381V6.618a1 1 0 0 1 .553-.894l4.553-2.277a2 2 0 0 1 1.788 0z\"></path><path d=\"M15 5.764v15\"></path><path d=\"M9 3.236v15\"></path>",
    },
    AutoLucideIcon {
        id: "globe-2",
        keywords: &["国际", "跨境", "海外"],
        inner_svg: "<path d=\"M21.54 15H17a2 2 0 0 0-2 2v4.54\"></path><path d=\"M7 3.34V5a3 3 0 0 0 3 3a2 2 0 0 1 2 2c0 1.1.9 2 2 2a2 2 0 0 0 2-2c0-1.1.9-2 2-2h3.17\"></path><path d=\"M11 21.95V18a2 2 0 0 0-2-2a2 2 0 0 1-2-2v-1a2 2 0 0 0-2-2H2.05\"></path><circle cx=\"12\" cy=\"12\" r=\"10\"></circle>",
    },
    AutoLucideIcon {
        id: "wrench",
        keywords: &["工具", "维修", "技术支持"],
        inner_svg: "<path d=\"M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z\"></path>",
    },
    AutoLucideIcon {
        id: "rocket",
        keywords: &["创业", "增长", "启动", "增长黑客"],
        inner_svg: "<path d=\"M4.5 16.5c-1.5 1.26-2 5-2 5s3.74-.5 5-2c.71-.84.7-2.13-.09-2.91a2.18 2.18 0 0 0-2.91-.09z\"></path><path d=\"m12 15-3-3a22 22 0 0 1 2-3.95A12.88 12.88 0 0 1 22 2c0 2.72-.78 7.5-6 11a22.35 22.35 0 0 1-4 2z\"></path><path d=\"M9 12H4s.55-3.03 2-4c1.62-1.08 5 0 5 0\"></path><path d=\"M12 15v5s3.03-.55 4-2c1.08-1.62 0-5 0-5\"></path>",
    },
    AutoLucideIcon {
        id: "lightbulb",
        keywords: &["创意", "灵感", "点子"],
        inner_svg: "<path d=\"M15 14c.2-1 .7-1.7 1.5-2.5 1-.9 1.5-2.2 1.5-3.5A6 6 0 0 0 6 8c0 1 .2 2.2 1.5 3.5.7.7 1.3 1.5 1.5 2.5\"></path><path d=\"M9 18h6\"></path><path d=\"M10 22h4\"></path>",
    },
    AutoLucideIcon {
        id: "wand-sparkles",
        keywords: &["生成", "魔法", "创意生成"],
        inner_svg: "<path d=\"m21.64 3.64-1.28-1.28a1.21 1.21 0 0 0-1.72 0L2.36 18.64a1.21 1.21 0 0 0 0 1.72l1.28 1.28a1.2 1.2 0 0 0 1.72 0L21.64 5.36a1.2 1.2 0 0 0 0-1.72\"></path><path d=\"m14 7 3 3\"></path><path d=\"M5 6v4\"></path><path d=\"M19 14v4\"></path><path d=\"M10 2v2\"></path><path d=\"M7 8H3\"></path><path d=\"M21 16h-4\"></path><path d=\"M11 3H9\"></path>",
    },
    AutoLucideIcon {
        id: "sparkles",
        keywords: &["助手", "通用", "智能"],
        inner_svg: "<path d=\"M9.937 15.5A2 2 0 0 0 8.5 14.063l-6.135-1.582a.5.5 0 0 1 0-.962L8.5 9.936A2 2 0 0 0 9.937 8.5l1.582-6.135a.5.5 0 0 1 .963 0L14.063 8.5A2 2 0 0 0 15.5 9.937l6.135 1.581a.5.5 0 0 1 0 .964L15.5 14.063a2 2 0 0 0-1.437 1.437l-1.582 6.135a.5.5 0 0 1-.963 0z\"></path><path d=\"M20 3v4\"></path><path d=\"M22 5h-4\"></path><path d=\"M4 17v2\"></path><path d=\"M5 18H3\"></path>",
    },
    AutoLucideIcon {
        id: "bot",
        keywords: &["机器人", "agent", "bot"],
        inner_svg: "<path d=\"M12 8V4H8\"></path><rect width=\"16\" height=\"12\" x=\"4\" y=\"8\" rx=\"2\"></rect><path d=\"M2 14h2\"></path><path d=\"M20 14h2\"></path><path d=\"M15 13v2\"></path><path d=\"M9 13v2\"></path>",
    },
];

const COLORS: &[&str] = &[
    "#2563eb", "#0891b2", "#0d9488", "#16a34a", "#d97706", "#ea580c",
    "#dc2626", "#db2777", "#7c3aed", "#4f46e5", "#0f172a", "#65a30d",
];

fn hash_seed(s: &str) -> usize {
    let mut h: u64 = 1469598103934665603;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(1099511628211);
    }
    h as usize
}

/// 用名称 + 职能文本匹配最合适的 Lucide 图标 id。
pub fn suggest_lucide_icon_id(name: &str, focus: &str, style: &str) -> &'static str {
    let hay = format!("{name} {focus} {style}").to_lowercase();
    let mut best_id = "bot";
    let mut best_score = 0i32;
    let mut tied: Vec<&'static str> = vec!["bot"];
    for item in AUTO_LUCIDE_ICONS {
        let mut score = 0i32;
        if hay.contains(item.id) {
            score += 4;
        }
        for kw in item.keywords {
            let k = kw.to_lowercase();
            if hay.contains(&k) {
                score += 1 + (k.chars().count() as i32 / 2);
            }
        }
        if matches!(item.id, "bot" | "sparkles") {
            score = score.saturating_sub(1);
        }
        if score > best_score {
            best_score = score;
            best_id = item.id;
            tied = vec![item.id];
        } else if score == best_score && score > 0 {
            tied.push(item.id);
        }
    }
    if best_score == 0 {
        let idx = hash_seed(name.trim()) % AUTO_LUCIDE_ICONS.len().max(1);
        return AUTO_LUCIDE_ICONS[idx].id;
    }
    if tied.len() == 1 {
        return best_id;
    }
    tied[hash_seed(name.trim()) % tied.len()]
}

fn color_for(name: &str) -> &'static str {
    COLORS[hash_seed(name.trim()) % COLORS.len()]
}

/// 生成带颜色的完整 Lucide SVG 字节。
pub fn lucide_svg_bytes(icon_id: &str, name_for_color: &str) -> Option<Vec<u8>> {
    let item = AUTO_LUCIDE_ICONS.iter().find(|i| i.id == icon_id)?;
    let color = color_for(name_for_color);
    let inner = item.inner_svg.replace("#2563eb", color).replace("currentColor", color);
    let svg = format!(
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8"?>"#, "\n",
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128" "#,
            r#"viewBox="0 0 24 24" fill="none" stroke="{color}" stroke-width="2" "#,
            r#"stroke-linecap="round" stroke-linejoin="round">"#, "{inner}</svg>"
        ),
        color = color,
        inner = inner,
    );
    Some(svg.into_bytes())
}

/// 若工作区尚无 emoji 图标，按文本自动写入 Lucide SVG。
pub fn apply_auto_lucide_icon(
    ws: &std::path::Path,
    name: &str,
    focus: &str,
    style: &str,
) -> anyhow::Result<Option<&'static str>> {
    use crate::config::agent_icons::{write_agent_icon, AgentIconKind};

    let assets = ws.join("assets");
    if assets.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&assets) {
            for entry in entries.flatten() {
                if entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("emoji.")
                {
                    return Ok(None);
                }
            }
        }
    }

    let id = suggest_lucide_icon_id(name, focus, style);
    let Some(bytes) = lucide_svg_bytes(id, name) else {
        return Ok(None);
    };
    write_agent_icon(ws, AgentIconKind::Emoji, &bytes, "emoji.svg")?;
    Ok(Some(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn suggests_code_for_dev_names() {
        assert_eq!(suggest_lucide_icon_id("代码助手", "帮我写 Rust", ""), "code-2");
    }

    #[test]
    fn writes_svg_when_missing() {
        let dir = TempDir::new().unwrap();
        let ws = dir.path();
        std::fs::write(ws.join("IDENTITY.md"), "# IDENTITY\n\n- **Name:** Demo\n- **Emoji:** _(可选)_\n").unwrap();
        let id = apply_auto_lucide_icon(ws, "设计专家", "UI 设计", "简洁").unwrap();
        assert_eq!(id, Some("palette"));
        assert!(ws.join("assets/emoji.svg").is_file());
    }
}
