//! Read-only home diagnostics. JSON contains paths/metadata, never config values.
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let root = match args.next() {
        None => home::default_memory_dir(),
        Some(flag) if flag == "--root" => args
            .next()
            .map(std::path::PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("--root requires a path"))?,
        _ => anyhow::bail!("usage: astro-doctor [--root <path>]"),
    };
    anyhow::ensure!(args.next().is_none(), "usage: astro-doctor [--root <path>]");
    println!(
        "{}",
        serde_json::to_string_pretty(&home::storage_diagnostics::inspect_home(&root, &[]))?
    );
    Ok(())
}
