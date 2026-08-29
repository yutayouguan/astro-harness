#[path = "../runtime/code_mode.rs"]
mod code_mode;

use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let service = code_mode::CodeModeService::default();
    let source = code_mode::parse_exec_source(
        "store('answer', 42); const value = await tools.echo({answer: load('answer')}); text(value.answer); await yield_control(); text('resumed');",
    )
    .map_err(anyhow::Error::msg)?;
    let cwd = std::env::current_dir()?;
    let cell_id = service
        .execute(
            &source,
            &[code_mode::NestedToolMetadata {
                name: "echo".into(),
                wire_name: "echo".into(),
                description: "echo input".into(),
            }],
            &cwd,
        )
        .await?;
    let mut saw_tool = false;
    let mut saw_text = false;
    let mut saw_yield = false;
    loop {
        match service.next_event(&cell_id, Duration::from_secs(2)).await? {
            code_mode::NextEvent::Event(code_mode::RuntimeEvent::Store { key, value }) => {
                service.update_store(key, value).await;
            }
            code_mode::NextEvent::Event(code_mode::RuntimeEvent::ToolCall { id, name, input }) => {
                anyhow::ensure!(name == "echo" && input["answer"] == 42);
                saw_tool = true;
                service.send_tool_result(&cell_id, &id, Ok(input)).await?;
            }
            code_mode::NextEvent::Event(code_mode::RuntimeEvent::Content { value, .. }) => {
                saw_text |= value == "42";
            }
            code_mode::NextEvent::Event(code_mode::RuntimeEvent::Yield) => {
                saw_yield = true;
                service.resume(&cell_id).await?;
            }
            code_mode::NextEvent::Event(code_mode::RuntimeEvent::Result { error }) => {
                anyhow::ensure!(error.is_none() && saw_tool && saw_text && saw_yield);
                break;
            }
            code_mode::NextEvent::TimedOut => anyhow::bail!("runtime timed out"),
            code_mode::NextEvent::Closed(error) => anyhow::bail!(error),
        }
    }
    println!("code mode smoke passed");
    Ok(())
}
