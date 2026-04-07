//! Remote SSH file editing support.
//!
//! Provides functions to read and write files on remote machines via SSH.
//! Uses the system `ssh` and `scp` commands, inheriting the user's SSH
//! config, keys, and known hosts.

use std::path::PathBuf;
use std::process::Command;

/// Parsed remote file specification: `user@host:/path/to/file`.
#[derive(Debug, Clone)]
pub struct RemoteSpec {
    /// SSH user (may be empty to use default).
    pub user: String,
    /// Remote hostname or IP.
    pub host: String,
    /// Optional port (0 = default 22).
    pub port: u16,
    /// Remote file path.
    pub path: String,
}

impl RemoteSpec {
    /// Parse a remote spec string like `user@host:/path` or `host:/path`.
    ///
    /// Returns `None` if the string doesn't look like a remote spec
    /// (i.e., doesn't contain `:`).
    pub fn parse(s: &str) -> Option<Self> {
        // Must contain ':' with a path after it.
        let (host_part, path) = s.split_once(':')?;
        if path.is_empty() || host_part.is_empty() {
            return None;
        }
        // Avoid matching Windows drive letters like C:\path.
        if host_part.len() == 1 && host_part.chars().all(|c| c.is_ascii_alphabetic()) {
            return None;
        }
        let (user, host) = if let Some((u, h)) = host_part.split_once('@') {
            (u.to_string(), h.to_string())
        } else {
            (String::new(), host_part.to_string())
        };
        Some(Self {
            user,
            host,
            port: 0,
            path: path.to_string(),
        })
    }

    /// Format as `user@host:path` or `host:path`.
    pub fn display(&self) -> String {
        if self.user.is_empty() {
            format!("{}:{}", self.host, self.path)
        } else {
            format!("{}@{}:{}", self.user, self.host, self.path)
        }
    }

    /// Build the SSH destination (`user@host` or just `host`).
    fn ssh_dest(&self) -> String {
        if self.user.is_empty() {
            self.host.clone()
        } else {
            format!("{}@{}", self.user, self.host)
        }
    }

    /// Build SSH port args if non-default.
    fn port_args(&self) -> Vec<String> {
        if self.port > 0 && self.port != 22 {
            vec!["-p".to_string(), self.port.to_string()]
        } else {
            Vec::new()
        }
    }

    /// Create a local path for caching the remote file.
    ///
    /// Returns a path like `/tmp/aura-ssh/<host>/<path>`.
    pub fn local_cache_path(&self) -> PathBuf {
        let dir = std::env::temp_dir().join("aura-ssh").join(&self.host);
        // Use the remote path but replace / with _ for flat storage.
        let filename = self.path.trim_start_matches('/').replace('/', "_");
        dir.join(filename)
    }
}

/// Read a file from a remote host via SSH.
///
/// Runs `ssh [user@]host cat <path>` and returns the file contents.
pub fn ssh_read(spec: &RemoteSpec) -> anyhow::Result<String> {
    let mut cmd = Command::new("ssh");
    for arg in spec.port_args() {
        cmd.arg(arg);
    }
    cmd.arg(spec.ssh_dest()).arg("cat").arg(&spec.path);

    let output = cmd
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run ssh: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("SSH read failed: {}", stderr.trim());
    }

    String::from_utf8(output.stdout)
        .map_err(|e| anyhow::anyhow!("Invalid UTF-8 in remote file: {e}"))
}

/// Write content to a remote file via SSH.
///
/// Pipes content through `ssh [user@]host tee <path> > /dev/null`.
pub fn ssh_write(spec: &RemoteSpec, content: &str) -> anyhow::Result<()> {
    let mut cmd = Command::new("ssh");
    for arg in spec.port_args() {
        cmd.arg(arg);
    }
    cmd.arg(spec.ssh_dest())
        .arg("tee")
        .arg(&spec.path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to run ssh: {e}"))?;

    if let Some(ref mut stdin) = child.stdin {
        use std::io::Write;
        stdin.write_all(content.as_bytes())?;
    }
    // Close stdin to signal EOF.
    drop(child.stdin.take());

    let output = child.wait_with_output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("SSH write failed: {}", stderr.trim());
    }
    Ok(())
}

/// List files/directories on a remote host via SSH.
///
/// Runs `ssh host ls -1 <path>` and returns the entries.
pub fn ssh_ls(spec: &RemoteSpec, path: &str) -> anyhow::Result<Vec<String>> {
    let mut cmd = Command::new("ssh");
    for arg in spec.port_args() {
        cmd.arg(arg);
    }
    cmd.arg(spec.ssh_dest()).arg("ls").arg("-1").arg(path);

    let output = cmd
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run ssh: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("SSH ls failed: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.lines().map(String::from).collect())
}

/// Check if a string looks like a remote SSH path.
pub fn is_remote_path(s: &str) -> bool {
    RemoteSpec::parse(s).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_remote_spec() {
        let spec = RemoteSpec::parse("user@host:/path/to/file").unwrap();
        assert_eq!(spec.user, "user");
        assert_eq!(spec.host, "host");
        assert_eq!(spec.path, "/path/to/file");
    }

    #[test]
    fn test_parse_no_user() {
        let spec = RemoteSpec::parse("host:/path/to/file").unwrap();
        assert_eq!(spec.user, "");
        assert_eq!(spec.host, "host");
        assert_eq!(spec.path, "/path/to/file");
    }

    #[test]
    fn test_is_not_remote() {
        // Windows drive letter should not be parsed as remote.
        assert!(!is_remote_path("C:\\path"));
        // No colon at all.
        assert!(!is_remote_path("/local/path"));
    }

    #[test]
    fn test_local_cache_path() {
        let spec = RemoteSpec::parse("deploy@server:/etc/nginx/nginx.conf").unwrap();
        let cache = spec.local_cache_path();
        let cache_str = cache.to_string_lossy();
        // Should contain the host name and a flattened filename.
        assert!(cache_str.contains("aura-ssh"));
        assert!(cache_str.contains("server"));
        assert!(cache_str.contains("etc_nginx_nginx.conf"));
    }
}
