//! Built-in HTTP/REST client for `.http` and `.rest` files.
//!
//! Parses HTTP request files (method, URL, headers, body), executes
//! them via `curl`, and displays the response with status, headers,
//! and body.

/// A parsed HTTP request from a `.http` file.
#[derive(Debug, Clone)]
pub struct HttpRequest {
    /// HTTP method (GET, POST, PUT, DELETE, etc.).
    pub method: String,
    /// Request URL.
    pub url: String,
    /// Request headers.
    pub headers: Vec<(String, String)>,
    /// Request body (empty for GET/DELETE).
    pub body: String,
}

/// An HTTP response from executing a request.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    /// HTTP status code.
    pub status: u16,
    /// Status text (e.g., "OK", "Not Found").
    pub status_text: String,
    /// Response headers.
    pub headers: Vec<(String, String)>,
    /// Response body.
    pub body: String,
    /// Time taken in milliseconds.
    pub time_ms: u64,
}

/// Parse an HTTP request from the text around a cursor position.
///
/// Looks for a request block starting with `METHOD URL` (e.g., `GET https://...`).
/// Headers follow on subsequent lines as `Key: Value`.
/// Body follows after an empty line.
/// Blocks are separated by `###`.
pub fn parse_request_at_cursor(content: &str, cursor_line: usize) -> Option<HttpRequest> {
    let lines: Vec<&str> = content.lines().collect();

    // Find the start of the request block containing the cursor.
    let mut block_start = cursor_line;
    while block_start > 0 {
        if lines
            .get(block_start.wrapping_sub(1))
            .is_some_and(|l| l.trim() == "###")
        {
            break;
        }
        block_start = block_start.saturating_sub(1);
    }

    // Find the request line (METHOD URL).
    let methods = ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"];
    let mut request_line_idx = None;
    for (i, line) in lines.iter().enumerate().skip(block_start) {
        let trimmed = line.trim();
        if trimmed == "###" && i > block_start {
            break;
        }
        if methods.iter().any(|m| trimmed.starts_with(m)) {
            request_line_idx = Some(i);
            break;
        }
    }

    let req_idx = request_line_idx?;
    let req_line = lines[req_idx].trim();
    let mut parts = req_line.splitn(2, ' ');
    let method = parts.next()?.to_string();
    let url = parts.next()?.trim().to_string();

    // Parse headers (lines after request line until empty line or ###).
    let mut headers = Vec::new();
    let mut body_lines: Vec<&str> = Vec::new();
    let mut in_body = false;
    for line in lines.iter().skip(req_idx + 1) {
        if line.trim() == "###" {
            break;
        }
        if in_body {
            body_lines.push(line);
        } else if line.trim().is_empty() {
            in_body = true;
        } else if let Some((key, value)) = line.split_once(':') {
            headers.push((key.trim().to_string(), value.trim().to_string()));
        }
    }

    let body = body_lines.join("\n");

    Some(HttpRequest {
        method,
        url,
        headers,
        body,
    })
}

/// Substitute variables like `{{name}}` from environment.
pub fn substitute_variables(text: &str) -> String {
    let mut result = text.to_string();
    // Replace {{VAR}} with environment variable values.
    while let Some(start) = result.find("{{") {
        if let Some(end) = result[start..].find("}}") {
            let var_name = &result[start + 2..start + end];
            let value = std::env::var(var_name).unwrap_or_else(|_| format!("{{{{{}}}}}", var_name));
            result = format!(
                "{}{}{}",
                &result[..start],
                value,
                &result[start + end + 2..]
            );
        } else {
            break;
        }
    }
    result
}

/// Execute an HTTP request via `curl` and return the response.
pub fn execute_request(req: &HttpRequest) -> anyhow::Result<HttpResponse> {
    let url = substitute_variables(&req.url);

    let mut cmd = std::process::Command::new("curl");
    cmd.args([
        "-s",
        "-w",
        "\n---AURA_HTTP_META---\n%{http_code}\n%{time_total}",
        "-X",
        &req.method,
    ]);

    // Add headers.
    for (key, value) in &req.headers {
        let header_val = substitute_variables(value);
        cmd.args(["-H", &format!("{}: {}", key, header_val)]);
    }

    // Include response headers.
    cmd.arg("-i");

    // Add body if present.
    if !req.body.is_empty() {
        let body = substitute_variables(&req.body);
        cmd.args(["-d", &body]);
    }

    cmd.arg(&url);

    let output = cmd
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run curl: {e}"))?;

    let raw = String::from_utf8_lossy(&output.stdout);

    // Split response from metadata.
    let (response_text, meta) = if let Some(idx) = raw.find("---AURA_HTTP_META---") {
        (&raw[..idx], &raw[idx + 21..])
    } else {
        (raw.as_ref(), "0\n0")
    };

    let meta_lines: Vec<&str> = meta.trim().lines().collect();
    let status: u16 = meta_lines.first().and_then(|s| s.parse().ok()).unwrap_or(0);
    let time_secs: f64 = meta_lines
        .get(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    let time_ms = (time_secs * 1000.0) as u64;

    // Parse response headers and body.
    let mut headers = Vec::new();
    let mut body = String::new();
    let mut status_text = String::new();
    let mut in_body = false;

    for line in response_text.lines() {
        if in_body {
            if !body.is_empty() {
                body.push('\n');
            }
            body.push_str(line);
        } else if line.trim().is_empty() {
            in_body = true;
        } else if line.starts_with("HTTP/") {
            // Status line: HTTP/1.1 200 OK
            let parts: Vec<&str> = line.splitn(3, ' ').collect();
            if parts.len() >= 3 {
                status_text = parts[2].to_string();
            }
        } else if let Some((key, value)) = line.split_once(':') {
            headers.push((key.trim().to_string(), value.trim().to_string()));
        }
    }

    Ok(HttpResponse {
        status,
        status_text,
        headers,
        body,
        time_ms,
    })
}

/// Format an HTTP response for display.
pub fn format_response(resp: &HttpResponse) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "HTTP {} {} ({} ms)\n",
        resp.status, resp.status_text, resp.time_ms
    ));
    output.push_str(&"─".repeat(40));
    output.push('\n');
    for (key, value) in &resp.headers {
        output.push_str(&format!("{}: {}\n", key, value));
    }
    output.push('\n');
    output.push_str(&resp.body);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_get_request() {
        let content = "GET https://api.example.com/users\nAuthorization: Bearer token123\n";
        let req = parse_request_at_cursor(content, 0).unwrap();
        assert_eq!(req.method, "GET");
        assert_eq!(req.url, "https://api.example.com/users");
        assert_eq!(req.headers.len(), 1);
        assert_eq!(req.headers[0].0, "Authorization");
        assert_eq!(req.headers[0].1, "Bearer token123");
        assert!(req.body.is_empty());
    }

    #[test]
    fn test_parse_post_with_body() {
        let content = "POST https://api.example.com/users\nContent-Type: application/json\n\n{\"name\": \"Alice\"}\n";
        let req = parse_request_at_cursor(content, 0).unwrap();
        assert_eq!(req.method, "POST");
        assert_eq!(req.url, "https://api.example.com/users");
        assert_eq!(req.headers.len(), 1);
        assert_eq!(req.headers[0].0, "Content-Type");
        assert!(req.body.contains("\"name\""));
    }

    #[test]
    fn test_substitute_variables() {
        std::env::set_var("AURA_TEST_VAR_XYZ", "hello_world");
        let result = substitute_variables("prefix-{{AURA_TEST_VAR_XYZ}}-suffix");
        assert_eq!(result, "prefix-hello_world-suffix");
        std::env::remove_var("AURA_TEST_VAR_XYZ");
    }

    #[test]
    fn test_format_response() {
        let resp = HttpResponse {
            status: 200,
            status_text: "OK".to_string(),
            headers: vec![("Content-Type".to_string(), "application/json".to_string())],
            body: "{\"id\": 1}".to_string(),
            time_ms: 42,
        };
        let formatted = format_response(&resp);
        assert!(formatted.contains("HTTP 200 OK"));
        assert!(formatted.contains("42 ms"));
        assert!(formatted.contains("Content-Type: application/json"));
        assert!(formatted.contains("{\"id\": 1}"));
    }
}
