// Runs the programs the app depends on (gpg, reprepro, git...) with a time limit,
// so one that hangs never freezes the window, and optionally streams live output to the window.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use serde::Serialize;
use tauri::Emitter;

pub static CURRENT_PID: AtomicU32 = AtomicU32::new(0);
pub static IS_CANCELLED: AtomicBool = AtomicBool::new(false);

pub fn reset_cancel() {
    IS_CANCELLED.store(false, Ordering::SeqCst);
    CURRENT_PID.store(0, Ordering::SeqCst);
}

pub fn _check_cancelled() -> Result<(), String> {
    if IS_CANCELLED.load(Ordering::SeqCst) {
        Err("Cancelled".to_string())
    } else {
        Ok(())
    }
}

#[tauri::command]
pub fn cancel_current_job() -> Result<(), String> {
    IS_CANCELLED.store(true, Ordering::SeqCst);
    let pid = CURRENT_PID.load(Ordering::SeqCst);
    if pid > 0 {
        let _ = Command::new("kill").args(["-TERM", &pid.to_string()]).status();
        let _ = Command::new("kill").args(["-KILL", &pid.to_string()]).status();
    }
    Ok(())
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct JobStart {
    pub id: u64,
    pub title: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct JobLine {
    pub id: u64,
    pub line: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct JobDone {
    pub id: u64,
    pub success: bool,
    pub cancelled: bool,
}

#[derive(Clone)]
pub struct JobLogger {
    pub app: tauri::AppHandle,
    pub id: u64,
}

impl JobLogger {
    pub fn new(app: tauri::AppHandle, title: &str) -> Self {
        reset_cancel();
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let _ = app.emit("job-start", JobStart { id, title: title.to_string() });
        Self { app, id }
    }

    /// A job whose title the interface translates: `key` is under `jobs.steps`.
    pub fn new_step(app: tauri::AppHandle, key: &str, values: &[(&str, &str)]) -> Self {
        Self::new(app, &step_text(key, values))
    }

    /// Raw output of a tool (git, reprepro, gpg), shown as it is.
    pub fn line(&self, text: impl Into<String>) {
        let _ = self.app.emit("job-line", JobLine { id: self.id, line: text.into() });
    }

    /// A step of the program itself, translated by the interface.
    pub fn step(&self, key: &str, values: &[(&str, &str)]) {
        self.line(step_text(key, values));
    }

    pub fn done(&self, success: bool) {
        let cancelled = IS_CANCELLED.load(Ordering::SeqCst);
        let _ = self.app.emit("job-done", JobDone { id: self.id, success: success && !cancelled, cancelled });
    }
}

/// Marks a line as a translatable step: the record separator, then
/// `{"k": key, "v": {name: value}}`. The interface (lib/jobs.ts) turns it into
/// the text of `jobs.steps.<key>` in its language; tool output never starts
/// with this character.
pub const STEP_MARK: char = '\u{1e}';

pub fn step_text(key: &str, values: &[(&str, &str)]) -> String {
    let v: serde_json::Map<String, serde_json::Value> = values.iter().map(|(k, v)| (k.to_string(), serde_json::Value::String(v.to_string()))).collect();
    format!("{STEP_MARK}{}", serde_json::json!({ "k": key, "v": v }))
}

pub struct Run {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

impl Run {
    /// The last lines the program wrote to its error output, as the message
    /// of an error.
    pub fn error(&self, program: &str) -> String {
        let lines: Vec<&str> = self.stderr.lines().filter(|l| !l.trim().is_empty()).collect();
        let tail = lines[lines.len().saturating_sub(3)..].join("\n");
        if tail.is_empty() {
            format!("{program} failed")
        } else {
            tail
        }
    }
}

fn stream_pipe(
    app: &tauri::AppHandle,
    id: u64,
    mut stream: impl Read + Send + 'static,
    sink: Arc<Mutex<Vec<u8>>>,
) -> std::thread::JoinHandle<()> {
    let app = app.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 1024];
        let mut line_buf = Vec::new();
        while let Ok(n) = stream.read(&mut buf) {
            if n == 0 {
                break;
            }
            if let Ok(mut s) = sink.lock() {
                s.extend_from_slice(&buf[..n]);
            }
            for &b in &buf[..n] {
                if b == b'\n' || b == b'\r' {
                    if !line_buf.is_empty() {
                        let text = String::from_utf8_lossy(&line_buf).trim().to_string();
                        if !text.is_empty() {
                            let _ = app.emit("job-line", JobLine { id, line: text });
                        }
                        line_buf.clear();
                    }
                } else {
                    line_buf.push(b);
                }
            }
        }
        if !line_buf.is_empty() {
            let text = String::from_utf8_lossy(&line_buf).trim().to_string();
            if !text.is_empty() {
                let _ = app.emit("job-line", JobLine { id, line: text });
            }
        }
    })
}

pub fn run_logged(
    logger: Option<&JobLogger>,
    program: &str,
    args: &[&str],
    dir: Option<&Path>,
    input: Option<&str>,
    timeout: Duration,
) -> Result<Run, String> {
    run_logged_env(logger, program, args, dir, input, &[], timeout)
}

/// As run_logged, with environment variables for the program. They are not
/// written to the log, which is where secrets (a token) can travel.
pub fn run_logged_env(
    logger: Option<&JobLogger>,
    program: &str,
    args: &[&str],
    dir: Option<&Path>,
    input: Option<&str>,
    envs: &[(String, String)],
    timeout: Duration,
) -> Result<Run, String> {
    if IS_CANCELLED.load(Ordering::SeqCst) {
        return Err("Cancelled".to_string());
    }
    if let Some(l) = logger {
        l.line(format!("$ {} {}", program, args.join(" ")));
    }
    let mut command = Command::new(program);
    command
        .args(args)
        .envs(envs.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = dir {
        command.current_dir(dir);
    }
    let mut child = command.spawn().map_err(|e| format!("{program} could not be started: {e}"))?;
    CURRENT_PID.store(child.id(), Ordering::SeqCst);

    if let (Some(text), Some(mut stdin)) = (input, child.stdin.take()) {
        let _ = stdin.write_all(text.as_bytes());
    }
    let out_pipe = child.stdout.take().ok_or("no output")?;
    let err_pipe = child.stderr.take().ok_or("no output")?;

    let out_sink = Arc::new(Mutex::new(Vec::new()));
    let err_sink = Arc::new(Mutex::new(Vec::new()));

    let (out_handle, err_handle) = if let Some(l) = logger {
        (
            stream_pipe(&l.app, l.id, out_pipe, out_sink.clone()),
            stream_pipe(&l.app, l.id, err_pipe, err_sink.clone()),
        )
    } else {
        let os = out_sink.clone();
        let es = err_sink.clone();
        (
            std::thread::spawn(move || {
                let mut p = out_pipe;
                let mut b = Vec::new();
                let _ = p.read_to_end(&mut b);
                if let Ok(mut lock) = os.lock() {
                    *lock = b;
                }
            }),
            std::thread::spawn(move || {
                let mut p = err_pipe;
                let mut b = Vec::new();
                let _ = p.read_to_end(&mut b);
                if let Ok(mut lock) = es.lock() {
                    *lock = b;
                }
            }),
        )
    };

    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if IS_CANCELLED.load(Ordering::SeqCst) => {
                let _ = child.kill();
                let _ = child.wait();
                CURRENT_PID.store(0, Ordering::SeqCst);
                return Err("Cancelled".to_string());
            }
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                CURRENT_PID.store(0, Ordering::SeqCst);
                return Err(format!("{program} did not answer in {} seconds", timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => {
                CURRENT_PID.store(0, Ordering::SeqCst);
                return Err(e.to_string());
            }
        }
    };
    CURRENT_PID.store(0, Ordering::SeqCst);

    let _ = out_handle.join();
    let _ = err_handle.join();

    let stdout_bytes = out_sink.lock().unwrap().clone();
    let stderr_bytes = err_sink.lock().unwrap().clone();
    let stdout = String::from_utf8_lossy(&stdout_bytes).to_string();
    let stderr = String::from_utf8_lossy(&stderr_bytes).to_string();
    Ok(Run { ok: status.success(), stdout, stderr })
}

/// Runs `program` with `args` (in `dir` when given), writes `input` to its
/// standard input when given, and kills it after `timeout`.
pub fn run(program: &str, args: &[&str], dir: Option<&Path>, input: Option<&str>, timeout: Duration) -> Result<Run, String> {
    run_logged(None, program, args, dir, input, timeout)
}
