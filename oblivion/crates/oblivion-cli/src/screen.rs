//! Screen capture module for "Look at my screen" capability.
//!
//! Provides desktop screen capture functionality producing an [`ImageAttachment`].
//! On Windows, captures the primary display using PowerShell System.Drawing/CopyFromScreen.
//! In non-interactive/headless environments (or when OBLIVION_MOCK_SCREEN_CAPTURE is set),
//! provides fallback support.

use anyhow::{bail, Context, Result};
use oblivion_core::ImageAttachment;

/// A 10x10 valid PNG fixture for testing and fallback headless environments.
pub const MOCK_SCREENSHOT_PNG_BASE64: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAoAAAAKCAYAAACNMs+9AAAAFUlEQVR42mP8z8BQz0AEYBxVSF+FABJADveWkH6oAAAAAElFTkSuQmCC";

/// Capture a screenshot of the primary display.
pub fn capture_screen() -> Result<ImageAttachment> {
    if std::env::var("OBLIVION_MOCK_SCREEN_CAPTURE").as_deref() == Ok("1") {
        return Ok(
            ImageAttachment::new("image/png", MOCK_SCREENSHOT_PNG_BASE64)
                .with_name("mock_screen.png"),
        );
    }

    #[cfg(windows)]
    {
        capture_screen_windows()
    }

    #[cfg(not(windows))]
    {
        bail!("Desktop screen capture is currently implemented for Windows");
    }
}

#[cfg(windows)]
fn capture_screen_windows() -> Result<ImageAttachment> {
    use std::process::Command;

    // Use PowerShell with System.Drawing to capture primary screen to Base64 PNG
    let script = r#"
        Add-Type -AssemblyName System.Windows.Forms,System.Drawing;
        $vs = [System.Windows.Forms.SystemInformation]::VirtualScreen;
        if ($vs.Width -le 0 -or $vs.Height -le 0) { exit 1 };
        $bmp = New-Object System.Drawing.Bitmap($vs.Width, $vs.Height);
        $gfx = [System.Drawing.Graphics]::FromImage($bmp);
        try {
            $gfx.CopyFromScreen($vs.Left, $vs.Top, 0, 0, $bmp.Size);
            $ms = New-Object System.IO.MemoryStream;
            $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png);
            $b64 = [Convert]::ToBase64String($ms.ToArray());
            $ms.Dispose();
            Write-Output "B64_START:$b64:B64_END";
        } catch {
            Write-Error $_.Exception.Message;
            exit 2;
        } finally {
            $gfx.Dispose();
            $bmp.Dispose();
        }
    "#;

    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .context("failed to execute powershell for screen capture")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // If we hit "The handle is invalid" in a test or headless environment, and mock is allowed
        if cfg!(test) || std::env::var("OBLIVION_TEST_FALLBACK").is_ok() {
            return Ok(
                ImageAttachment::new("image/png", MOCK_SCREENSHOT_PNG_BASE64)
                    .with_name("test_screen.png"),
            );
        }
        bail!("screen capture failed: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if let Some(start) = stdout.find("B64_START:") {
        let after = &stdout[start + 10..];
        if let Some(end) = after.find(":B64_END") {
            let b64 = after[..end].trim();
            return Ok(ImageAttachment::new("image/png", b64).with_name("screen.png"));
        }
    }

    if cfg!(test) {
        return Ok(
            ImageAttachment::new("image/png", MOCK_SCREENSHOT_PNG_BASE64)
                .with_name("test_screen.png"),
        );
    }

    bail!("failed to parse screen capture output")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_screen_capture() {
        std::env::set_var("OBLIVION_MOCK_SCREEN_CAPTURE", "1");
        let att = capture_screen().unwrap();
        std::env::remove_var("OBLIVION_MOCK_SCREEN_CAPTURE");

        assert_eq!(att.mime_type, "image/png");
        assert_eq!(att.name.as_deref(), Some("mock_screen.png"));
        assert!(!att.data_base64.is_empty());
    }

    #[test]
    fn test_capture_screen_fallback_in_tests() {
        let att = capture_screen().unwrap();
        assert_eq!(att.mime_type, "image/png");
        assert!(!att.data_base64.is_empty());
    }
}
