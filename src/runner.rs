use std::process::Stdio;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::io::{AsyncBufReadExt, BufReader};

static STREAM_BUF: OnceLock<Arc<Mutex<Vec<String>>>> = OnceLock::new();

fn stream_buf() -> Arc<Mutex<Vec<String>>> {
    Arc::clone(STREAM_BUF.get_or_init(|| Arc::new(Mutex::new(Vec::new()))))
}

pub fn stream_push(line: String) {
    stream_buf().lock().unwrap().push(line);
}

pub fn stream_drain() -> Vec<String> {
    std::mem::take(&mut *stream_buf().lock().unwrap())
}

#[derive(Debug, Clone)]
pub struct CmdResult {
    pub success: bool,
    pub output: String,
}

pub async fn run(cmd: &str, args: &[&str]) -> CmdResult {
    let result = tokio::process::Command::new(cmd)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await;
    match result {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let combined = merge(stdout, stderr);
            CmdResult { success: out.status.success(), output: combined }
        }
        Err(e) => CmdResult { success: false, output: format!("실행 오류: {e}") },
    }
}

pub async fn run_sh(script: &str) -> CmdResult {
    run("bash", &["-c", script]).await
}

pub async fn run_stream(script: &str) -> CmdResult {
    let child = tokio::process::Command::new("bash")
        .args(["-c", script])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return CmdResult { success: false, output: format!("실행 오류: {e}") },
    };
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let buf1 = stream_buf();
    let buf2 = Arc::clone(&buf1);
    let t1 = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        let mut out = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            out.push_str(&line); out.push('\n');
            buf1.lock().unwrap().push(line);
        }
        out
    });
    let t2 = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut out = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            out.push_str(&line); out.push('\n');
            buf2.lock().unwrap().push(line);
        }
        out
    });
    let (r1, r2) = tokio::join!(t1, t2);
    let mut all = r1.unwrap_or_default();
    all.push_str(&r2.unwrap_or_default());
    let status = child.wait().await.ok();
    CmdResult { success: status.map(|s| s.success()).unwrap_or(false), output: all }
}

fn merge(stdout: String, stderr: String) -> String {
    match (stdout.trim().is_empty(), stderr.trim().is_empty()) {
        (true, true)   => String::new(),
        (false, true)  => stdout,
        (true, false)  => stderr,
        (false, false) => format!("{stdout}\n{stderr}"),
    }
}
