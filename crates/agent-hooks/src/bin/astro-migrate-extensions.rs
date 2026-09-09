use anyhow::{bail, Context, Result};
fn main() -> Result<()> {
    let mut root = None;
    let mut apply = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => root = Some(std::path::PathBuf::from(args.next().context("--root needs a path")?)),
            "--apply" => apply = true,
            _ => bail!("usage: astro-migrate-extensions --root <Astro home> [--apply]; stop Astro before applying"),
        }
    }
    let report = hooks::migration::migrate(&root.context("explicit --root is required")?, apply)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
