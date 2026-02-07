use crate::types::AuthCommand;
use chrono::Utc;
use log;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use uuid::Uuid;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

fn apply_hidden_process_flags(cmd: &mut Command) {
    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
}

fn managed_pid_file() -> PathBuf {
    let base = dirs::data_local_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(std::env::temp_dir);
    base.join("vibeproxy").join("managed-server.pid")
}

fn persist_managed_pid(pid: u32) {
    let pid_file = managed_pid_file();
    if let Some(parent) = pid_file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(pid_file, pid.to_string());
}

fn load_managed_pid() -> Option<u32> {
    let text = std::fs::read_to_string(managed_pid_file()).ok()?;
    text.trim().parse::<u32>().ok()
}

fn clear_managed_pid() {
    let _ = std::fs::remove_file(managed_pid_file());
}

// ---------------------------------------------------------------------------
// RingBuffer
// ---------------------------------------------------------------------------

pub struct RingBuffer<T> {
    storage: Vec<Option<T>>,
    head: usize,
    tail: usize,
    count: usize,
}

impl<T> RingBuffer<T> {
    pub fn new(capacity: usize) -> Self {
        let safe_capacity = capacity.max(1);
        Self {
            storage: (0..safe_capacity).map(|_| None).collect(),
            head: 0,
            tail: 0,
            count: 0,
        }
    }

    pub fn append(&mut self, element: T) {
        let capacity = self.storage.len();
        self.storage[self.tail] = Some(element);

        if self.count == capacity {
            self.head = (self.head + 1) % capacity;
        } else {
            self.count += 1;
        }

        self.tail = (self.tail + 1) % capacity;
    }

    #[cfg(test)]
    pub fn elements(&self) -> Vec<&T> {
        let capacity = self.storage.len();
        if self.count == 0 {
            return Vec::new();
        }

        let mut result = Vec::with_capacity(self.count);
        for i in 0..self.count {
            let idx = (self.head + i) % capacity;
            if let Some(ref value) = self.storage[idx] {
                result.push(value);
            }
        }
        result
    }
}

// ---------------------------------------------------------------------------
// ServerManager
// ---------------------------------------------------------------------------

const MAX_LOG_LINES: usize = 1000;

pub struct ServerManager {
    child: Option<Child>,
    is_running: bool,
    log_buffer: Arc<Mutex<RingBuffer<String>>>,
}

impl ServerManager {
    pub fn new() -> Self {
        Self {
            child: None,
            is_running: false,
            log_buffer: Arc::new(Mutex::new(RingBuffer::new(MAX_LOG_LINES))),
        }
    }

    // -- accessors ----------------------------------------------------------

    pub fn is_running(&self) -> bool {
        self.is_running
    }

    pub async fn refresh_running_status(&mut self) -> bool {
        if !self.is_running {
            return false;
        }

        let mut exited_status = None;
        let mut wait_error = None;

        if let Some(child) = self.child.as_mut() {
            match child.try_wait() {
                Ok(Some(status)) => exited_status = Some(status),
                Ok(None) => {}
                Err(e) => wait_error = Some(e),
            }
        } else {
            self.is_running = false;
            clear_managed_pid();
            return false;
        }

        if let Some(status) = exited_status {
            self.child = None;
            self.is_running = false;
            clear_managed_pid();
            self.add_log(&format!(
                "Server exited unexpectedly with status: {}",
                status
            ))
            .await;
            return false;
        }

        if let Some(err) = wait_error {
            self.child = None;
            self.is_running = false;
            clear_managed_pid();
            self.add_log(&format!("Failed to check server process state: {}", err))
                .await;
            return false;
        }

        true
    }

    // -- logging ------------------------------------------------------------

    pub async fn add_log(&self, message: &str) {
        let timestamp = Utc::now().format("%H:%M:%S").to_string();
        let log_line = format!("[{}] {}", timestamp, message);
        let mut buf = self.log_buffer.lock().await;
        buf.append(log_line);
    }

    // -- start / stop -------------------------------------------------------

    pub async fn start(&mut self, config_path: &str, binary_path: &str) -> Result<(), String> {
        if self.refresh_running_status().await {
            return Ok(());
        }

        // Kill only the previously managed stale process before starting.
        Self::kill_orphaned_processes().await;

        use std::process::Stdio;

        let mut cmd = Command::new(binary_path);
        apply_hidden_process_flags(&mut cmd);
        cmd.args(["-config", config_path])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn server: {}", e))?;

        // Take stdout/stderr handles before moving child
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        self.child = Some(child);
        self.is_running = true;
        if let Some(pid) = self.child.as_ref().and_then(|c| c.id()) {
            persist_managed_pid(pid);
        }
        self.add_log(&format!("Server started (binary={})", binary_path))
            .await;

        // Spawn stdout reader
        if let Some(stdout) = stdout {
            let buf = Arc::clone(&self.log_buffer);
            tokio::spawn(async move {
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if !line.is_empty() {
                        let ts = Utc::now().format("%H:%M:%S").to_string();
                        let entry = format!("[{}] {}", ts, line);
                        let mut b = buf.lock().await;
                        b.append(entry);
                    }
                }
            });
        }

        // Spawn stderr reader
        if let Some(stderr) = stderr {
            let buf = Arc::clone(&self.log_buffer);
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if !line.is_empty() {
                        let ts = Utc::now().format("%H:%M:%S").to_string();
                        let entry = format!("[{}] WARN: {}", ts, line);
                        let mut b = buf.lock().await;
                        b.append(entry);
                    }
                }
            });
        }

        Ok(())
    }

    pub async fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            self.add_log("Stopping server...").await;

            // On Windows, child.kill() calls TerminateProcess.
            let _ = child.kill().await;

            // Wait with a 2-second timeout
            let wait_result =
                tokio::time::timeout(std::time::Duration::from_secs(2), child.wait()).await;

            match wait_result {
                Ok(Ok(status)) => {
                    self.add_log(&format!("Server stopped with status: {}", status))
                        .await;
                }
                Ok(Err(e)) => {
                    self.add_log(&format!("Error waiting for server: {}", e))
                        .await;
                }
                Err(_) => {
                    self.add_log("Server did not stop within 2s timeout").await;
                }
            }
        }

        self.is_running = false;
        clear_managed_pid();
    }

    // -- auth commands ------------------------------------------------------

    pub async fn run_auth_command(
        binary_path: &str,
        config_path: &str,
        command: &AuthCommand,
    ) -> Result<(bool, String), String> {
        use std::process::Stdio;

        let mut args: Vec<&str> = vec!["--config", config_path];
        let mut qwen_email: Option<String> = None;

        match command {
            AuthCommand::ClaudeLogin => args.push("-claude-login"),
            AuthCommand::CodexLogin => args.push("-codex-login"),
            AuthCommand::CopilotLogin => args.push("-github-copilot-login"),
            AuthCommand::GeminiLogin => args.push("-login"),
            AuthCommand::QwenLogin { email } => {
                args.push("-qwen-login");
                qwen_email = Some(email.clone());
            }
            AuthCommand::AntigravityLogin => args.push("-antigravity-login"),
        }

        let mut cmd = Command::new(binary_path);
        apply_hidden_process_flags(&mut cmd);
        let mut child = cmd
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn auth process: {}", e))?;

        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        // For Copilot we capture stdout to extract the device code.
        let captured_output = Arc::new(Mutex::new(String::new()));

        if let Some(stdout) = stdout {
            let capture = Arc::clone(&captured_output);
            let is_copilot = matches!(command, AuthCommand::CopilotLogin);
            tokio::spawn(async move {
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if is_copilot {
                        let mut cap = capture.lock().await;
                        cap.push_str(&line);
                        cap.push('\n');
                    }
                    log::info!("[Auth] stdout: {}", line);
                }
            });
        }

        if let Some(stderr) = stderr {
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    log::info!("[Auth] stderr: {}", line);
                }
            });
        }

        // Delayed stdin interactions
        if let Some(mut stdin) = stdin {
            match command {
                AuthCommand::GeminiLogin => {
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                        let _ = stdin.write_all(b"\n").await;
                        log::info!("[Auth] Sent newline for Gemini default project");
                    });
                }
                AuthCommand::CodexLogin => {
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_secs(12)).await;
                        let _ = stdin.write_all(b"\n").await;
                        log::info!("[Auth] Sent newline to keep Codex login waiting");
                    });
                }
                AuthCommand::QwenLogin { .. } => {
                    if let Some(email) = qwen_email {
                        tokio::spawn(async move {
                            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                            let payload = format!("{}\n", email);
                            let _ = stdin.write_all(payload.as_bytes()).await;
                            log::info!("[Auth] Sent Qwen email: {}", email);
                        });
                    }
                }
                _ => {
                    // Drop stdin so the process doesn't hang waiting for input
                    drop(stdin);
                }
            }
        }

        // Wait a short time then check process status
        let wait_secs = if matches!(command, AuthCommand::CopilotLogin) {
            2
        } else {
            1
        };
        tokio::time::sleep(std::time::Duration::from_secs(wait_secs)).await;

        // Check if process is still running by trying wait with zero timeout
        match tokio::time::timeout(std::time::Duration::from_millis(100), child.wait()).await {
            Err(_) => {
                // Timeout => still running, which means browser probably opened

                // For Copilot, try to extract the device code
                if matches!(command, AuthCommand::CopilotLogin) {
                    let output = captured_output.lock().await;
                    if let Some(code) = extract_copilot_code(&output) {
                        // Copy to clipboard
                        if let Ok(mut clipboard) = arboard::Clipboard::new() {
                            let _ = clipboard.set_text(&code);
                        }
                        return Ok((
                            true,
                            format!(
                                "Browser opened for GitHub authentication.\n\n\
                                 Code copied to clipboard:\n\n{}\n\n\
                                 Just paste it in the browser!\n\n\
                                 The app will automatically detect when you're authenticated.",
                                code
                            ),
                        ));
                    }
                    return Ok((
                        true,
                        "Browser opened for GitHub authentication.\n\n\
                         Check your terminal or the opened browser for the device code.\n\n\
                         The app will automatically detect when you're authenticated."
                            .to_string(),
                    ));
                }

                Ok((
                    true,
                    "Browser opened for authentication.\n\n\
                     Please complete the login in your browser.\n\n\
                     The app will automatically detect when you're authenticated."
                        .to_string(),
                ))
            }
            Ok(Ok(status)) => {
                // Process exited
                let output = captured_output.lock().await;
                if output.contains("Opening browser") || output.contains("Attempting to open URL") {
                    Ok((
                        true,
                        "Browser opened for authentication.\n\n\
                         Please complete the login in your browser.\n\n\
                         The app will automatically detect when you're authenticated."
                            .to_string(),
                    ))
                } else if status.success() {
                    Ok((true, "Authentication completed.".to_string()))
                } else {
                    Err(format!(
                        "Authentication process exited with code {}. Output: {}",
                        status.code().unwrap_or(-1),
                        output
                    ))
                }
            }
            Ok(Err(e)) => Err(format!("Error waiting for auth process: {}", e)),
        }
    }

    // -- orphaned process cleanup -------------------------------------------

    pub async fn kill_orphaned_processes() {
        let Some(pid) = load_managed_pid() else {
            return;
        };

        // Confirm this PID still matches the managed binary name before killing.
        let mut tasklist = Command::new("tasklist");
        apply_hidden_process_flags(&mut tasklist);
        let pid_filter = format!("PID eq {}", pid);
        let output = tasklist
            .args([
                "/FI",
                &pid_filter,
                "/FI",
                "IMAGENAME eq cli-proxy-api-plus.exe",
                "/FO",
                "CSV",
                "/NH",
            ])
            .output()
            .await;

        let Ok(output) = output else {
            clear_managed_pid();
            return;
        };

        let text = String::from_utf8_lossy(&output.stdout);
        let has_match = text
            .lines()
            .any(|line| line.contains("cli-proxy-api-plus.exe"));
        if !has_match {
            clear_managed_pid();
            return;
        }

        log::info!(
            "[ServerManager] Killing previously managed process PID={}",
            pid
        );
        let mut taskkill = Command::new("taskkill");
        apply_hidden_process_flags(&mut taskkill);
        let _ = taskkill
            .args(["/F", "/PID", &pid.to_string()])
            .output()
            .await;
        clear_managed_pid();

        // Small delay for cleanup
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    // -- Z.AI key persistence -----------------------------------------------

    pub fn save_zai_api_key(api_key: &str) -> Result<(bool, String), String> {
        let home = dirs::home_dir().ok_or("Could not determine home directory")?;
        let auth_dir = home.join(".cli-proxy-api");

        std::fs::create_dir_all(&auth_dir)
            .map_err(|e| format!("Failed to create auth directory: {}", e))?;

        // Masked preview: first 8 chars + "..." + last 4 chars
        let key_preview = if api_key.len() > 12 {
            format!("{}...{}", &api_key[..8], &api_key[api_key.len() - 4..])
        } else {
            api_key.to_string()
        };

        let timestamp = Utc::now().to_rfc3339();
        let id = Uuid::new_v4().to_string();
        let filename = format!("zai-{}.json", &id[..8]);
        let file_path = auth_dir.join(&filename);

        let auth_data = serde_json::json!({
            "type": "zai",
            "email": key_preview,
            "api_key": crate::secure_store::encrypt_secret(api_key)?,
            "api_key_encrypted": true,
            "created": timestamp
        });

        let json_bytes = serde_json::to_vec_pretty(&auth_data)
            .map_err(|e| format!("Failed to serialize JSON: {}", e))?;

        std::fs::write(&file_path, &json_bytes)
            .map_err(|e| format!("Failed to write key file: {}", e))?;

        log::info!("[ServerManager] Z.AI API key saved to {}", filename);
        Ok((true, "API key saved successfully".to_string()))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract the device code from Copilot CLI output.
/// Looks for patterns like "enter the code: XXXX-XXXX".
fn extract_copilot_code(output: &str) -> Option<String> {
    for line in output.lines() {
        if let Some(pos) = line.find("enter the code:") {
            let after = &line[pos + "enter the code:".len()..];
            let code = after.trim();
            if !code.is_empty() {
                return Some(code.to_string());
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_basic() {
        let mut rb = RingBuffer::new(3);
        rb.append("a".to_string());
        rb.append("b".to_string());
        rb.append("c".to_string());
        let elems: Vec<&str> = rb.elements().into_iter().map(|s| s.as_str()).collect();
        assert_eq!(elems, vec!["a", "b", "c"]);
    }

    #[test]
    fn ring_buffer_overflow() {
        let mut rb = RingBuffer::new(3);
        rb.append(1);
        rb.append(2);
        rb.append(3);
        rb.append(4); // overwrites 1
        let elems: Vec<&i32> = rb.elements();
        assert_eq!(elems, vec![&2, &3, &4]);
    }

    #[test]
    fn ring_buffer_empty() {
        let rb: RingBuffer<String> = RingBuffer::new(5);
        assert!(rb.elements().is_empty());
    }

    #[test]
    fn ring_buffer_min_capacity() {
        let mut rb = RingBuffer::new(0); // should become 1
        rb.append("only");
        let elems = rb.elements();
        assert_eq!(elems.len(), 1);
        assert_eq!(*elems[0], "only");
    }

    #[test]
    fn extract_copilot_code_found() {
        let output = "Please visit https://...\nenter the code: ABCD-1234\nWaiting...";
        assert_eq!(extract_copilot_code(output), Some("ABCD-1234".to_string()));
    }

    #[test]
    fn extract_copilot_code_not_found() {
        let output = "Some other output";
        assert_eq!(extract_copilot_code(output), None);
    }
}
