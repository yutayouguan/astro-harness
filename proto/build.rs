//! 编译 `proto/astro.proto`，生成 tonic server + client 代码。

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile(&["proto/astro.proto"], &["proto"])?;
    Ok(())
}
