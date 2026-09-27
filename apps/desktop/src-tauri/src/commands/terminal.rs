//! Shared terminal dock commands. PTY state lives in the backend so Agent tools and Desktop
//! attach to the same bounded-output session.

use std::time::Duration;

use proto::astro_service_client::AstroServiceClient;
use proto::{
    TerminalIdRequest, TerminalOpenRequest, TerminalReadRequest, TerminalResizeRequest,
    TerminalWriteRequest,
};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;
use tokio::sync::{Mutex, OnceCell};
use tonic::transport::{Channel, Endpoint};

use crate::infra::grpc::{default_grpc_address, endpoint_url};

const TERMINAL_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const TERMINAL_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

struct CachedTerminalChannel {
    address: String,
    channel: Channel,
}

static TERMINAL_CHANNEL: OnceCell<Mutex<Option<CachedTerminalChannel>>> = OnceCell::const_new();

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalOpenDto {
    pub scope: String,
    pub cwd: Option<String>,
    pub cols: u32,
    pub rows: u32,
    pub execution_mode: Option<String>,
    pub client_token: Option<String>,
    pub agent_default: Option<bool>,
    /// 预填到新 shell 的命令（不执行）：见「在终端打开」。
    pub initial_input: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalSessionDto {
    pub id: u64,
    pub scope: String,
    pub cwd: String,
    pub running: bool,
    pub exit_code: Option<u32>,
    pub base_cursor: u64,
    pub end_cursor: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalReadDto {
    pub id: u64,
    pub cursor: u64,
    pub max_bytes: Option<u32>,
    pub wait_ms: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalReadResultDto {
    pub id: u64,
    pub data: Vec<u8>,
    pub next_cursor: u64,
    pub dropped: bool,
    pub running: bool,
    pub exit_code: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalWriteDto {
    pub id: u64,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalResizeDto {
    pub id: u64,
    pub cols: u32,
    pub rows: u32,
}

#[tauri::command]
pub async fn terminal_open(request: TerminalOpenDto) -> Result<TerminalSessionDto, String> {
    let mut client = terminal_client().await?;
    let client_token = request.client_token.unwrap_or_default();
    let response = tokio::time::timeout(
        TERMINAL_REQUEST_TIMEOUT,
        client.open_terminal(TerminalOpenRequest {
            scope: request.scope,
            cwd: request.cwd.unwrap_or_default(),
            cols: request.cols,
            rows: request.rows,
            execution_mode: request.execution_mode.unwrap_or_default(),
            // Keep the legacy single-terminal behavior intact. Token-owned tabs have stable
            // identities and are rejected on a mode mismatch by the backend instead.
            replace_mode_mismatch: client_token.trim().is_empty(),
            client_token,
            agent_default: request.agent_default.unwrap_or(false),
            initial_input: request.initial_input.unwrap_or_default(),
        }),
    )
    .await
    .map_err(|_| "terminal backend did not respond within 10 seconds".to_string())?
    .map_err(|error| error.to_string())?
    .into_inner();
    Ok(TerminalSessionDto {
        id: response.id,
        scope: response.scope,
        cwd: response.cwd,
        running: response.running,
        exit_code: response.exit_code,
        base_cursor: response.base_cursor,
        end_cursor: response.end_cursor,
    })
}

#[tauri::command]
pub async fn terminal_read(request: TerminalReadDto) -> Result<TerminalReadResultDto, String> {
    let mut client = terminal_client().await?;
    let response = client
        .read_terminal(TerminalReadRequest {
            id: request.id,
            cursor: request.cursor,
            max_bytes: request.max_bytes.unwrap_or(64 * 1024),
            wait_ms: request.wait_ms.unwrap_or(5_000),
        })
        .await
        .map_err(|error| error.to_string())?
        .into_inner();
    Ok(TerminalReadResultDto {
        id: response.id,
        data: response.data,
        next_cursor: response.next_cursor,
        dropped: response.dropped,
        running: response.running,
        exit_code: response.exit_code,
    })
}

#[tauri::command]
pub async fn terminal_write(request: TerminalWriteDto) -> Result<(), String> {
    let mut client = terminal_client().await?;
    client
        .write_terminal(TerminalWriteRequest {
            id: request.id,
            data: request.data,
        })
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn terminal_resize(request: TerminalResizeDto) -> Result<(), String> {
    let mut client = terminal_client().await?;
    client
        .resize_terminal(TerminalResizeRequest {
            id: request.id,
            cols: request.cols,
            rows: request.rows,
        })
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn terminal_kill(id: u64) -> Result<(), String> {
    let mut client = terminal_client().await?;
    client
        .kill_terminal(TerminalIdRequest { id })
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn terminal_close(id: u64) -> Result<(), String> {
    let mut client = terminal_client().await?;
    client
        .close_terminal(TerminalIdRequest { id })
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn terminal_open_external(app: AppHandle, cwd: String) -> Result<(), String> {
    let cwd = std::path::PathBuf::from(cwd)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !cwd.is_dir() {
        return Err(format!(
            "terminal cwd is not a directory: {}",
            cwd.display()
        ));
    }

    #[cfg(target_os = "macos")]
    let command = app.shell().command("/usr/bin/open").args([
        "-a".into(),
        "Terminal".into(),
        cwd.into_os_string(),
    ]);

    #[cfg(target_os = "windows")]
    let command = {
        let path = cwd.to_string_lossy().replace('\'', "''");
        app.shell().command("cmd.exe").args([
            "/C".into(),
            "start".into(),
            "".into(),
            "powershell.exe".into(),
            "-NoExit".into(),
            "-Command".into(),
            format!("Set-Location -LiteralPath '{path}'").into(),
        ])
    };

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let command = app
        .shell()
        .command(std::env::var_os("TERMINAL").unwrap_or_else(|| "x-terminal-emulator".into()))
        .args(["--working-directory".into(), cwd.into_os_string()]);

    command
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

async fn terminal_client() -> Result<AstroServiceClient<tonic::transport::Channel>, String> {
    let address = default_grpc_address();
    let cache = TERMINAL_CHANNEL
        .get_or_init(|| async { Mutex::new(None) })
        .await;
    let mut cached = cache.lock().await;
    if let Some(existing) = cached.as_ref().filter(|entry| entry.address == address) {
        return Ok(AstroServiceClient::new(existing.channel.clone()));
    }

    let channel = Endpoint::from_shared(endpoint_url(&address))
        .map_err(|error| error.to_string())?
        .connect_timeout(TERMINAL_CONNECT_TIMEOUT)
        .timeout(TERMINAL_REQUEST_TIMEOUT)
        .connect_lazy();
    *cached = Some(CachedTerminalChannel {
        address,
        channel: channel.clone(),
    });
    Ok(AstroServiceClient::new(channel))
}
