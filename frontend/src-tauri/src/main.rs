//! Astro Agent 桌面二进制入口。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    astro_agent_lib::run();
}
