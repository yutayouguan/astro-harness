# CI/CD 与发布流水线

> 阶段：系统设计 | 状态：定稿 | 说明：GitHub Actions、多平台、Eval 门控

## 概述

Astro Agent 使用 GitHub Actions 实现两条独立流水线：**PR 验证流水线**（每次 Pull Request 触发）和**发布流水线**（tag push 触发）。Eval 质量门控嵌入两条流水线，确保每次发布都满足质量基准。

---

## 1. PR 验证流水线

### 触发条件

- `push` 到非主分支
- `pull_request` 指向 `main` 或 `develop`

### 检查项目

| 步骤 | 工具 | 通过条件 |
| ---- | ---- | ---- |
| Rust 编译检查 | `cargo check` | 无编译错误 |
| 单元测试 | `cargo test` | 所有测试通过 |
| Lint | `cargo clippy -- -D warnings` | 无 Warning |
| 前端类型检查 | `pnpm tsc --noEmit` | 无类型错误 |
| Eval 质量门控 | `cargo run -p agent-evals` | 分数 ≥ 基准线 95% |

### `.github/workflows/ci.yml`

```yaml
name: CI

on:
  push:
    branches-ignore:
      - main
  pull_request:
    branches:
      - main
      - develop

env:
  CARGO_TERM_COLOR: always
  RUST_BACKTRACE: 1

jobs:
  check:
    name: Rust 检查
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: 安装 Rust 工具链
        uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt

      - name: 缓存 Cargo 依赖
        uses: Swatinem/rust-cache@v2

      - name: cargo check
        run: cargo check --workspace --all-features

      - name: cargo clippy
        run: cargo clippy --workspace --all-features -- -D warnings

      - name: cargo test
        run: cargo test --workspace --all-features

  frontend:
    name: 前端检查
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: 安装 Node.js
        uses: actions/setup-node@v4
        with:
          node-version: '20'

      - name: 安装 pnpm
        uses: pnpm/action-setup@v3
        with:
          version: 9

      - name: 安装依赖
        run: pnpm install --frozen-lockfile

      - name: TypeScript 类型检查
        run: pnpm tsc --noEmit

      - name: ESLint
        run: pnpm lint

  eval-gate:
    name: Eval 质量门控
    runs-on: ubuntu-latest
    needs: [check]
    # 仅在 PR 时运行（避免普通推送消耗 API 费用）
    if: github.event_name == 'pull_request'
    steps:
      - uses: actions/checkout@v4

      - name: 安装 Rust 工具链
        uses: dtolnay/rust-toolchain@stable

      - name: 缓存 Cargo 依赖
        uses: Swatinem/rust-cache@v2

      - name: 运行 Eval 套件
        env:
          ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
          EVAL_BASELINE_FILE: evals/baseline.json
        run: |
          cargo run -p agent-evals -- \
            --output evals/results.json \
            --baseline evals/baseline.json \
            --threshold 0.95

      - name: 上传 Eval 报告
        uses: actions/upload-artifact@v4
        if: always()
        with:
          name: eval-results
          path: evals/results.json
```

---

## 2. 发布流水线

### 触发条件

推送形如 `v*` 的 tag（如 `v0.3.0`）。

### 三平台并行构建

| 平台 | Runner | 产物 |
| ---- | ---- | ---- |
| Windows x64 | `windows-latest` | `.msi` + `.exe`（NSIS 安装包） |
| macOS aarch64 | `macos-14` | `.dmg`（Apple Silicon） |
| Linux x64 | `ubuntu-22.04` | `.AppImage` + `.deb` |

### `.github/workflows/release.yml`

```yaml
name: Release

on:
  push:
    tags:
      - 'v*'

permissions:
  contents: write

env:
  CARGO_TERM_COLOR: always

jobs:
  # 发布前 Eval 质量门控
  eval-gate:
    name: 发布前 Eval 质量门控
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: 安装 Rust 工具链
        uses: dtolnay/rust-toolchain@stable

      - name: 缓存 Cargo 依赖
        uses: Swatinem/rust-cache@v2

      - name: 运行 Eval 套件（严格模式）
        env:
          ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
        run: |
          cargo run -p agent-evals -- \
            --output evals/results-release.json \
            --baseline evals/baseline.json \
            --threshold 0.95 \
            --strict

  build:
    name: 构建 ${{ matrix.platform }}
    needs: eval-gate
    strategy:
      fail-fast: false
      matrix:
        include:
          - platform: windows-latest
            target: x86_64-pc-windows-msvc
            artifact_suffix: windows-x64
          - platform: macos-14
            target: aarch64-apple-darwin
            artifact_suffix: macos-aarch64
          - platform: ubuntu-22.04
            target: x86_64-unknown-linux-gnu
            artifact_suffix: linux-x64

    runs-on: ${{ matrix.platform }}

    steps:
      - uses: actions/checkout@v4

      - name: 安装 Rust 工具链
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      - name: 缓存 Cargo 依赖
        uses: Swatinem/rust-cache@v2
        with:
          key: ${{ matrix.target }}

      - name: 安装 Linux 系统依赖
        if: matrix.platform == 'ubuntu-22.04'
        run: |
          sudo apt-get update
          sudo apt-get install -y \
            libwebkit2gtk-4.1-dev \
            libappindicator3-dev \
            librsvg2-dev \
            patchelf

      - name: 安装 Node.js
        uses: actions/setup-node@v4
        with:
          node-version: '20'

      - name: 安装 pnpm
        uses: pnpm/action-setup@v3
        with:
          version: 9

      - name: 安装前端依赖
        run: pnpm install --frozen-lockfile

      # Windows 代码签名
      - name: 导入 Windows 签名证书
        if: matrix.platform == 'windows-latest'
        env:
          WINDOWS_CERTIFICATE: ${{ secrets.WINDOWS_CERTIFICATE }}
          WINDOWS_CERTIFICATE_PASSWORD: ${{ secrets.WINDOWS_CERTIFICATE_PASSWORD }}
        run: |
          $cert_bytes = [System.Convert]::FromBase64String($env:WINDOWS_CERTIFICATE)
          $cert_path = "$env:TEMP\certificate.pfx"
          [System.IO.File]::WriteAllBytes($cert_path, $cert_bytes)
          echo "TAURI_SIGNING_PRIVATE_KEY_PATH=$cert_path" >> $env:GITHUB_ENV
        shell: pwsh

      # macOS 公证
      - name: 导入 macOS 签名身份
        if: matrix.platform == 'macos-14'
        env:
          APPLE_CERTIFICATE: ${{ secrets.APPLE_CERTIFICATE }}
          APPLE_CERTIFICATE_PASSWORD: ${{ secrets.APPLE_CERTIFICATE_PASSWORD }}
          APPLE_SIGNING_IDENTITY: ${{ secrets.APPLE_SIGNING_IDENTITY }}
          APPLE_ID: ${{ secrets.APPLE_ID }}
          APPLE_PASSWORD: ${{ secrets.APPLE_NOTARIZATION_PASSWORD }}
          APPLE_TEAM_ID: ${{ secrets.APPLE_TEAM_ID }}
        run: |
          echo $APPLE_CERTIFICATE | base64 --decode > certificate.p12
          security create-keychain -p "" build.keychain
          security import certificate.p12 -k build.keychain -P "$APPLE_CERTIFICATE_PASSWORD" -T /usr/bin/codesign
          security set-key-partition-list -S apple-tool:,apple: -s -k "" build.keychain
          security list-keychains -d user -s build.keychain

      - name: 构建 Tauri 应用
        uses: tauri-apps/tauri-action@v0
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
          APPLE_ID: ${{ secrets.APPLE_ID }}
          APPLE_PASSWORD: ${{ secrets.APPLE_NOTARIZATION_PASSWORD }}
          APPLE_TEAM_ID: ${{ secrets.APPLE_TEAM_ID }}
        with:
          tagName: ${{ github.ref_name }}
          releaseName: 'Astro Agent ${{ github.ref_name }}'
          releaseBody: |
            查看 [CHANGELOG](https://github.com/${{ github.repository }}/blob/main/CHANGELOG.md) 了解本版本变更。
          releaseDraft: false
          prerelease: false
          args: --target ${{ matrix.target }}

      - name: 上传产物
        uses: actions/upload-artifact@v4
        with:
          name: astro-agent-${{ matrix.artifact_suffix }}
          path: |
            src-tauri/target/${{ matrix.target }}/release/bundle/

  # 更新 latest.json（供 Tauri Updater 使用）
  update-manifest:
    name: 更新 Updater 清单
    needs: build
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          ref: main

      - name: 下载所有产物
        uses: actions/download-artifact@v4
        with:
          path: artifacts/

      - name: 生成 latest.json
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          TAG: ${{ github.ref_name }}
        run: |
          python3 scripts/generate_latest_json.py \
            --tag "$TAG" \
            --artifacts-dir artifacts/ \
            --output latest.json

      - name: 上传 latest.json 到 Release
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        run: |
          gh release upload ${{ github.ref_name }} latest.json --clobber
```

---

## 3. 版本号管理

### 语义化版本（SemVer）

```
MAJOR.MINOR.PATCH
  │      │     └── Bug 修复、小改动
  │      └────── 新功能（向后兼容）
  └──────────── 破坏性变更
```

版本号需在以下三处保持一致：

- `Cargo.toml`（workspace package version）
- `src-tauri/tauri.conf.json`（`version` 字段）
- `package.json`（`version` 字段）

使用脚本统一更新：

```bash
#!/usr/bin/env bash
# scripts/bump-version.sh

VERSION=$1
sed -i "s/^version = \".*\"/version = \"$VERSION\"/" Cargo.toml
node -e "
  const f = require('./package.json');
  f.version = '$VERSION';
  require('fs').writeFileSync('./package.json', JSON.stringify(f, null, 2) + '\n');
"
jq ".version = \"$VERSION\"" src-tauri/tauri.conf.json > /tmp/tauri.conf.json
mv /tmp/tauri.conf.json src-tauri/tauri.conf.json
git add Cargo.toml package.json src-tauri/tauri.conf.json
git commit -m "chore: bump version to $VERSION"
git tag "v$VERSION"
```

### CHANGELOG 自动生成

使用 [git-cliff](https://git-cliff.org/) 从 Conventional Commits 生成 CHANGELOG：

```toml
# cliff.toml

[changelog]
header = "# Changelog\n\n"
body = """
{% for group, commits in commits | group_by(attribute="group") %}
### {{ group | upper_first }}
{% for commit in commits %}
- {{ commit.message | upper_first }} ([{{ commit.id | truncate(length=7, end="") }}]({{ commit.id }}))
{% endfor %}
{% endfor %}
"""
footer = ""
trim = true

[git]
conventional_commits = true
commit_parsers = [
  { message = "^feat", group = "Features" },
  { message = "^fix", group = "Bug Fixes" },
  { message = "^perf", group = "Performance" },
  { message = "^refactor", group = "Refactor" },
  { message = "^doc", group = "Documentation" },
  { skip = true, message = "^chore" },
]
```

---

## 4. Eval 集成

```rust
// crates/agent-evals/src/main.rs

use clap::Parser;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    output: String,
    #[arg(long)]
    baseline: String,
    /// 质量分阈值（相对基准线的比例）
    #[arg(long, default_value = "0.95")]
    threshold: f32,
    /// 严格模式：任意单项低于阈值即失败
    #[arg(long)]
    strict: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let baseline: EvalBaseline = serde_json::from_str(
        &std::fs::read_to_string(&args.baseline)?
    )?;

    let runner = EvalRunner::new().await?;
    let results = runner.run_all().await?;

    // 与基准线对比
    let passed = results.iter().all(|r| {
        let baseline_score = baseline.get_score(&r.eval_id).unwrap_or(1.0);
        let ratio = r.score / baseline_score;
        if args.strict {
            ratio >= args.threshold
        } else {
            // 宽松模式：整体平均分 >= 阈值
            true
        }
    });

    let avg_ratio = if !baseline.is_empty() {
        results.iter().map(|r| {
            r.score / baseline.get_score(&r.eval_id).unwrap_or(1.0)
        }).sum::<f32>() / results.len() as f32
    } else {
        1.0
    };

    std::fs::write(&args.output, serde_json::to_string_pretty(&results)?)?;

    if !passed || (!args.strict && avg_ratio < args.threshold) {
        eprintln!("❌ Eval 质量门控失败：平均分比率 {:.2}，阈值 {:.2}", avg_ratio, args.threshold);
        std::process::exit(1);
    }

    println!("✅ Eval 质量门控通过：平均分比率 {:.2}", avg_ratio);
    Ok(())
}
```

---

## 5. 本地开发命令速查

| 任务 | 命令 |
| ---- | ---- |
| 运行所有 Rust 测试 | `cargo test --workspace` |
| Lint 检查 | `cargo clippy --workspace -- -D warnings` |
| 格式化代码 | `cargo fmt --all` |
| 启动开发模式（Tauri） | `pnpm tauri dev` |
| 构建生产版本 | `pnpm tauri build` |
| 运行前端测试 | `pnpm test` |
| 前端类型检查 | `pnpm tsc --noEmit` |
| 运行 Eval 套件 | `cargo run -p agent-evals` |
| 更新版本号 | `bash scripts/bump-version.sh 0.3.0` |
| 生成 CHANGELOG | `git cliff -o CHANGELOG.md` |
| 创建发布 Tag | `git tag v0.3.0 && git push origin v0.3.0` |

---

## 相关文档

- [01-测试策略.md](01-测试策略.md) — 测试策略与 Eval 套件设计
- [04-更新机制.md](04-更新机制.md) — Tauri Updater 机制（消费 latest.json）
- [06-自我进化引擎.md](../02-核心功能模块/06-自我进化引擎.md) — 进化引擎（Eval 数据来源）
