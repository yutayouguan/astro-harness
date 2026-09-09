fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let mut root = None;
    let mut apply = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => {
                root = Some(std::path::PathBuf::from(
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("--root requires a path"))?,
                ))
            }
            "--apply" => apply = true,
            _ => anyhow::bail!("usage: astro-migrate-config --root <Astro home> [--apply]"),
        }
    }
    let root = root
        .ok_or_else(|| anyhow::anyhow!("--root is required; omit --apply for read-only preview"))?;
    let report = home::settings::migration::migrate(&root, apply)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
