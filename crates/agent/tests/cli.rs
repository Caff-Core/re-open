#![allow(clippy::unwrap_used, clippy::print_stderr)] // Test harness: a panic is the failure report.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use tempfile::TempDir;

struct Env {
    root: TempDir,
}

impl Env {
    fn new() -> Self {
        Self {
            root: TempDir::new().unwrap(),
        }
    }

    fn dir(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_reopen-agent"));
        command
            .args(args)
            .env("REOPEN_DATA_DIR", self.dir("data"))
            .env("REOPEN_STATE_DIR", self.dir("state"))
            .env("REOPEN_RUNTIME_DIR", self.dir("run"))
            .stdin(Stdio::null());
        command
    }

    fn cli(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    fn start(&self, scenario: &str) -> Child {
        let child = self
            .command(&[
                "run",
                "--provider",
                "fake",
                "--interval-secs",
                "1",
                "--fake-scenario",
                scenario,
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        wait_for(
            || self.cli(&["status"]).status.success(),
            "agent to become ready",
        );
        child
    }
}

fn wait_for(mut condition: impl FnMut() -> bool, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("timed out waiting for {what}");
}

fn text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn terminate(mut child: Child) {
    let status = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            if !status.success() {
                let mut stderr = String::new();
                std::io::Read::read_to_string(child.stderr.as_mut().unwrap(), &mut stderr).unwrap();
                eprintln!("{stderr}");
            }
            assert!(status.success(), "agent exited with {status}");
            return;
        }
        assert!(
            Instant::now() < deadline,
            "agent did not stop after SIGTERM"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn logs(dir: &Path) -> String {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
        .collect()
}

#[test]
fn agent_lifecycle_through_the_cli() {
    let env = Env::new();

    let not_running = env.cli(&["status"]);
    assert_eq!(not_running.status.code(), Some(5));
    assert!(String::from_utf8_lossy(&not_running.stderr).contains("not running"));

    let agent = env.start("partial");
    assert!(text(&env.cli(&["status"])).contains("capture active"));

    let second = env.cli(&["run"]);
    assert_eq!(
        second.status.code(),
        Some(3),
        "a second agent must refuse to start"
    );

    assert!(env.cli(&["capture"]).status.success());
    let mut history = String::new();
    wait_for(
        || {
            history = text(&env.cli(&["history", "--limit", "5"]));
            history.contains("partial")
        },
        "a partial checkpoint in history",
    );
    let id = history
        .split_whitespace()
        .find(|word| word.len() == 36 && word.matches('-').count() == 4)
        .unwrap();

    let detail = text(&env.cli(&["checkpoint", "show", id]));
    assert!(
        detail.contains("Partial: not every required source was captured."),
        "{detail}"
    );
    assert!(
        detail.contains("desktop.apps") && detail.contains("(limit_exceeded)"),
        "{detail}"
    );
    assert!(detail.contains("org.example.Editor"), "{detail}");

    let missing = env.cli(&["checkpoint", "show", "0b9c3f0e-5d6a-4c8e-9f1a-2b3c4d5e6f70"]);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("NotFound"));

    terminate(agent);
    assert!(
        !env.dir("run").join("desktop.sock").exists(),
        "socket is removed on shutdown"
    );

    // The previous session had useful work, so a restart protects it.
    let agent = env.start("complete");
    let status = text(&env.cli(&["status"]));
    assert!(
        status.contains("Pending recovery candidates: 1"),
        "{status}"
    );
    terminate(agent);

    let log = logs(&env.dir("state").join("logs"));
    assert!(
        log.contains("agent.ready") && log.contains("capture.committed"),
        "{log}"
    );
    assert!(
        !log.contains("org.example"),
        "resource labels must never be logged"
    );
}

#[test]
fn restore_preview_start_and_status_through_the_cli() {
    let env = Env::new();
    let agent = env.start("complete");
    assert!(env.cli(&["capture"]).status.success());
    let mut history = String::new();
    wait_for(
        || {
            history = text(&env.cli(&["history"]));
            history.contains("complete")
        },
        "a checkpoint",
    );
    let checkpoint = history
        .split_whitespace()
        .find(|word| word.len() == 36 && word.matches('-').count() == 4)
        .unwrap()
        .to_owned();

    let only_second = text(&env.cli(&["restore", "preview", &checkpoint, "--only", "1"]));
    assert!(
        only_second.contains("org.example.Browser") && !only_second.contains("org.example.Editor"),
        "{only_second}"
    );
    assert!(
        !env.cli(&["restore", "preview", &checkpoint, "--only", "9"])
            .status
            .success()
    );

    let preview = text(&env.cli(&["restore", "preview", &checkpoint]));
    assert!(preview.contains("2 to launch"), "{preview}");
    let plan = preview.split_whitespace().nth(1).unwrap().to_owned();

    let started = env.cli(&["restore", "start", &plan]);
    assert!(
        started.status.success(),
        "{}",
        String::from_utf8_lossy(&started.stderr)
    );
    let run = text(&started).split_whitespace().nth(2).unwrap().to_owned();
    let mut status = String::new();
    wait_for(
        || {
            status = text(&env.cli(&["restore", "status", &run]));
            status.contains(": completed")
        },
        "the restore to complete",
    );
    assert_eq!(status.matches("restored").count(), 2, "{status}");

    let reused = env.cli(&["restore", "start", &plan]);
    assert!(
        String::from_utf8_lossy(&reused.stderr).contains("StalePlan"),
        "a plan is single use"
    );
    let unknown = env.cli(&["restore", "status", "0b9c3f0e-5d6a-4c8e-9f1a-2b3c4d5e6f70"]);
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("NotFound"));
    terminate(agent);
}
