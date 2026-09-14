#![cfg(target_os = "macos")]

use std::io::{Read, Write};
use std::process::Command;
use std::sync::mpsc;
use std::time::Duration;

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use sandbox::{macos, SandboxPolicy};
use types::SandboxMode;

const PROBE: &str = r#"
import errno, fcntl, os, select, signal, sys, termios, tty
signal.alarm(15)
assert os.isatty(0)
original = termios.tcgetattr(0)
try:
    tty.setraw(0)
    print("__ready__", flush=True)
    assert os.read(0, 1) == b"k"
    try:
        fcntl.ioctl(0, termios.TIOCSTI, b"x")
    except OSError as error:
        assert sys.argv[1] == "deny", error
        assert error.errno == errno.EPERM, error
        assert not select.select([0], [], [], 0)[0]
    else:
        assert os.read(0, 1) == b"x"
        assert sys.argv[1] == "allow", "TIOCSTI unexpectedly succeeded"
finally:
    termios.tcsetattr(0, termios.TCSANOW, original)
print("__passed__", flush=True)
"#;

#[test]
fn sandbox_blocks_terminal_input_injection() -> anyhow::Result<()> {
    let availability = Command::new(macos::SANDBOX_EXEC)
        .args(["-p", "(version 1)(allow default)", "/usr/bin/true"])
        .output()?;
    if !availability.status.success()
        && String::from_utf8_lossy(&availability.stderr)
            .contains("sandbox-exec: sandbox_apply: Operation not permitted")
    {
        eprintln!("skipping terminal injection test: nested Seatbelt is unavailable");
        return Ok(());
    }
    assert!(availability.status.success(), "{availability:?}");

    run_probe(
        "/usr/bin/python3",
        &["-c", PROBE, "allow"],
        tempfile::tempdir()?.path(),
    )?;
    let workspace = tempfile::tempdir()?;
    let policy = SandboxPolicy::new(SandboxMode::ReadOnly, workspace.path(), Vec::new(), false)?;
    run_probe(
        macos::SANDBOX_EXEC,
        &[
            "-p",
            &macos::seatbelt_profile(&policy),
            "/usr/bin/python3",
            "-c",
            PROBE,
            "deny",
        ],
        workspace.path(),
    )
}

fn run_probe(program: &str, args: &[&str], cwd: &std::path::Path) -> anyhow::Result<()> {
    let pair = native_pty_system().openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    let mut command = CommandBuilder::new(program);
    command.args(args);
    command.cwd(cwd);
    let mut child = pair.slave.spawn_command(command)?;
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader()?;
    let (ready_tx, ready_rx) = mpsc::channel();
    let (output_tx, output_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut output = Vec::new();
        let mut chunk = [0_u8; 1024];
        let mut ready_tx = Some(ready_tx);
        loop {
            match reader.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    output.extend_from_slice(&chunk[..read]);
                    if ready_tx.is_some() && String::from_utf8_lossy(&output).contains("__ready__")
                    {
                        let _ = ready_tx.take().unwrap().send(());
                    }
                }
            }
        }
        let _ = output_tx.send(output);
    });

    ready_rx.recv_timeout(Duration::from_secs(10))?;
    let mut writer = pair.master.take_writer()?;
    writer.write_all(b"k")?;
    writer.flush()?;
    let status = child.wait()?;
    drop(writer);
    drop(pair.master);
    let output = output_rx.recv_timeout(Duration::from_secs(5))?;
    let output = String::from_utf8_lossy(&output);
    anyhow::ensure!(status.success(), "terminal probe failed: {output}");
    anyhow::ensure!(
        output.contains("__passed__"),
        "terminal probe failed: {output}"
    );
    Ok(())
}
