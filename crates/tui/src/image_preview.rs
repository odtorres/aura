// `ImageInfo.path` is populated for diagnostics and future "preview a
// different image" lookup but not currently read after construction.
// Keep at module scope so the field stays adjacent to its derive(Debug).
#![allow(dead_code)]

//! Image preview in the terminal using the Kitty graphics protocol.
//!
//! When an image file (PNG, JPG, GIF, SVG, WebP) is opened, AURA renders
//! it directly in the terminal using the Kitty graphics protocol. Falls
//! back to showing file metadata if the terminal doesn't support it.

use std::path::Path;

/// Supported image extensions.
const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "webp", "svg", "ico", "tiff", "tif",
];

/// Check if a file path is an image based on its extension.
pub fn is_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Image metadata for display.
#[derive(Debug, Clone)]
pub struct ImageInfo {
    /// File path.
    pub path: std::path::PathBuf,
    /// File size in bytes.
    pub size: u64,
    /// Image dimensions (if determinable from header).
    pub dimensions: Option<(u32, u32)>,
    /// Format name.
    pub format: String,
}

impl ImageInfo {
    /// Load image info from a file path.
    pub fn from_path(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let format = match ext.as_str() {
            "png" => "PNG",
            "jpg" | "jpeg" => "JPEG",
            "gif" => "GIF",
            "bmp" => "BMP",
            "webp" => "WebP",
            "svg" => "SVG",
            "ico" => "ICO",
            "tiff" | "tif" => "TIFF",
            _ => "Image",
        }
        .to_string();

        // Try to read dimensions from PNG header.
        let dimensions = if ext == "png" {
            read_png_dimensions(path)
        } else if ext == "jpg" || ext == "jpeg" {
            read_jpeg_dimensions(path)
        } else {
            None
        };

        Some(Self {
            path: path.to_path_buf(),
            size: metadata.len(),
            dimensions,
            format,
        })
    }

    /// Human-readable file size.
    pub fn size_display(&self) -> String {
        if self.size < 1024 {
            format!("{} B", self.size)
        } else if self.size < 1024 * 1024 {
            format!("{:.1} KB", self.size as f64 / 1024.0)
        } else {
            format!("{:.1} MB", self.size as f64 / (1024.0 * 1024.0))
        }
    }

    /// Display string for dimensions.
    pub fn dimensions_display(&self) -> String {
        match self.dimensions {
            Some((w, h)) => format!("{}x{}", w, h),
            None => "unknown".to_string(),
        }
    }
}

/// Render an image to the terminal using the Kitty graphics protocol.
///
/// Writes the escape sequence directly to stdout. The image is displayed
/// at the current cursor position. Returns true if rendering was attempted.
pub fn render_kitty_image(path: &Path, max_cols: u16, max_rows: u16) -> bool {
    // Read the file as base64.
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(_) => return false,
    };

    use std::io::Write;
    let b64 = base64_encode(&data);

    // Kitty graphics protocol: send image data in chunks.
    // Format: ESC_G<key=value,...>;<payload>ESC\
    let mut stdout = std::io::stdout().lock();

    // Clear any previous image at this position.
    let _ = write!(stdout, "\x1b_Ga=d;\x1b\\");

    // Send image in chunks of 4096 base64 chars.
    let chunks: Vec<&str> = b64
        .as_bytes()
        .chunks(4096)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect();

    for (i, chunk) in chunks.iter().enumerate() {
        let more = if i + 1 < chunks.len() { 1 } else { 0 };
        if i == 0 {
            // First chunk: include image parameters.
            let _ = write!(
                stdout,
                "\x1b_Ga=T,f=100,c={},r={},m={};{}\x1b\\",
                max_cols, max_rows, more, chunk
            );
        } else {
            // Continuation chunks.
            let _ = write!(stdout, "\x1b_Gm={};{}\x1b\\", more, chunk);
        }
    }

    let _ = stdout.flush();
    true
}

/// Check if the terminal likely supports the Kitty graphics protocol.
pub fn supports_kitty_graphics() -> bool {
    let term_program = std::env::var("TERM_PROGRAM").unwrap_or_default();
    let term = std::env::var("TERM").unwrap_or_default();

    matches!(
        term_program.as_str(),
        "kitty" | "WezTerm" | "ghostty" | "iTerm.app"
    ) || term.contains("kitty")
        || term.contains("xterm-kitty")
}

/// Read PNG dimensions from the IHDR chunk (bytes 16-23).
fn read_png_dimensions(path: &Path) -> Option<(u32, u32)> {
    let data = std::fs::read(path).ok()?;
    if data.len() < 24 || &data[..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let width = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
    let height = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
    Some((width, height))
}

/// Read JPEG dimensions from the SOF marker.
fn read_jpeg_dimensions(path: &Path) -> Option<(u32, u32)> {
    let data = std::fs::read(path).ok()?;
    if data.len() < 2 || data[0] != 0xFF || data[1] != 0xD8 {
        return None;
    }
    let mut i = 2;
    while i + 9 < data.len() {
        if data[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = data[i + 1];
        // SOF0-SOF3 markers contain dimensions.
        if (0xC0..=0xC3).contains(&marker) {
            let height = u16::from_be_bytes([data[i + 5], data[i + 6]]) as u32;
            let width = u16::from_be_bytes([data[i + 7], data[i + 8]]) as u32;
            return Some((width, height));
        }
        let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
        i += 2 + len;
    }
    None
}

/// Simple base64 encoder (no external dependency).
pub(crate) fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let combined = (b0 << 16) | (b1 << 8) | b2;

        result.push(CHARS[((combined >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((combined >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((combined >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(combined & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_is_image_file() {
        assert!(is_image_file(Path::new("photo.png")));
        assert!(is_image_file(Path::new("photo.jpg")));
        assert!(is_image_file(Path::new("photo.JPEG")));
        assert!(is_image_file(Path::new("icon.gif")));
        assert!(is_image_file(Path::new("logo.webp")));
        assert!(!is_image_file(Path::new("main.rs")));
        assert!(!is_image_file(Path::new("plugin.lua")));
        assert!(!is_image_file(Path::new("noext")));
    }

    #[test]
    fn test_base64_encode() {
        // "Hello" in base64 is "SGVsbG8="
        let encoded = base64_encode(b"Hello");
        assert_eq!(encoded, "SGVsbG8=");

        // Empty input.
        let empty = base64_encode(b"");
        assert_eq!(empty, "");

        // Single byte.
        let one = base64_encode(b"A");
        assert_eq!(one, "QQ==");
    }

    #[test]
    fn test_png_magic_bytes() {
        // A non-PNG file should return None.
        // Create a temp file with non-PNG content.
        let tmp = std::env::temp_dir().join("aura_test_not_a_png.bin");
        std::fs::write(&tmp, b"This is not a PNG file at all").unwrap();
        let dims = read_png_dimensions(&tmp);
        assert!(dims.is_none());
        let _ = std::fs::remove_file(&tmp);
    }
}
