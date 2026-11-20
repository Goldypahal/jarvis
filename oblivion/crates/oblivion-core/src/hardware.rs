//! Hardware detection and local model-size recommendation (SRS Month 3).
//!
//! Detects physical RAM, available RAM, GPU, VRAM, and free disk space.
//! Based on the detected capabilities, recommends an optimal local model size
//! (e.g. 1.5B, 3B, 7B-8B, 14B, 32B) with a one-sentence trade-off explanation.

use serde::{Deserialize, Serialize};

/// System hardware specifications.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HardwareInfo {
    /// Total physical RAM in bytes.
    pub total_ram_bytes: u64,
    /// Available physical RAM in bytes.
    pub available_ram_bytes: u64,
    /// Total physical RAM formatted in GB.
    pub total_ram_gb: f64,
    /// Available physical RAM formatted in GB.
    pub available_ram_gb: f64,
    /// Detected primary GPU name.
    pub gpu_name: String,
    /// Detected dedicated VRAM in bytes, if available.
    pub vram_bytes: Option<u64>,
    /// Detected dedicated VRAM formatted in GB.
    pub vram_gb: Option<f64>,
    /// Whether the primary GPU is an integrated graphics unit.
    pub is_integrated_gpu: bool,
    /// Free disk space on current workspace volume in bytes.
    pub free_disk_bytes: u64,
    /// Free disk space formatted in GB.
    pub free_disk_gb: f64,
}

/// Model size recommendation tailored to the detected hardware.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelRecommendation {
    /// Recommended parameter class: "1B-1.5B", "3B", "7B-8B", "14B", "32B".
    pub recommended_size: String,
    /// Suggested model names available in Ollama/GGUF format.
    pub recommended_models: Vec<String>,
    /// Clear one-sentence explanation of the trade-off.
    pub tradeoff_explanation: String,
    /// Whether the model weights and context can fit entirely in dedicated GPU VRAM.
    pub fits_in_vram: bool,
    /// Estimated RAM or VRAM needed at Q4_K_M quantization (in GB).
    pub estimated_ram_required_gb: f64,
    /// Recommended quantization scheme.
    pub recommended_quantization: String,
    /// Maximum recommended context window in tokens.
    pub max_recommended_context_tokens: usize,
}

/// Detect system hardware on the current machine.
pub fn detect_hardware() -> HardwareInfo {
    let (total_ram, avail_ram) = detect_ram();
    let (gpu_name, vram_bytes, is_integrated) = detect_gpu();
    let free_disk = detect_free_disk();

    let total_ram_gb = round_gb(total_ram);
    let available_ram_gb = round_gb(avail_ram);
    let vram_gb = vram_bytes.map(round_gb);
    let free_disk_gb = round_gb(free_disk);

    HardwareInfo {
        total_ram_bytes: total_ram,
        available_ram_bytes: avail_ram,
        total_ram_gb,
        available_ram_gb,
        gpu_name,
        vram_bytes,
        vram_gb,
        is_integrated_gpu: is_integrated,
        free_disk_bytes: free_disk,
        free_disk_gb,
    }
}

fn round_gb(bytes: u64) -> f64 {
    let gb = bytes as f64 / (1024.0 * 1024.0 * 1024.0);
    (gb * 10.0).round() / 10.0
}

/// Recommend an optimal model size based on hardware specifications.
pub fn recommend_model(hw: &HardwareInfo) -> ModelRecommendation {
    let vram_gb = hw.vram_gb.unwrap_or(0.0);
    let total_ram_gb = hw.total_ram_gb;

    // Check for large dedicated GPU (>= 14 GB VRAM) or high-RAM workstation (>= 48 GB RAM)
    if (vram_gb >= 14.0 && !hw.is_integrated_gpu) || total_ram_gb >= 48.0 {
        ModelRecommendation {
            recommended_size: "14B".into(),
            recommended_models: vec![
                "qwen2.5-coder:14b".into(),
                "qwen2.5-coder:32b".into(),
                "llama3.3:70b".into(),
            ],
            tradeoff_explanation: "A 14B or 32B model delivers near-frontier reasoning accuracy, but consumes significant memory and runs slower than smaller models.".into(),
            fits_in_vram: vram_gb >= 11.0,
            estimated_ram_required_gb: 11.0,
            recommended_quantization: "Q4_K_M".into(),
            max_recommended_context_tokens: 32_768,
        }
    } else if total_ram_gb >= 15.0 || (vram_gb >= 6.0 && !hw.is_integrated_gpu) {
        // Sweet spot: 16 GB RAM or 6-8 GB dedicated VRAM
        let fits_in_vram = vram_gb >= 6.0 && !hw.is_integrated_gpu;
        ModelRecommendation {
            recommended_size: "7B-8B".into(),
            recommended_models: vec![
                "qwen2.5-coder:7b".into(),
                "llama3.1:8b".into(),
                "mistral:7b".into(),
            ],
            tradeoff_explanation: "An 8B model offers the best balance of coding intelligence and execution speed on 16 GB RAM, while a 3B model is faster if you need lighter system memory usage.".into(),
            fits_in_vram,
            estimated_ram_required_gb: 5.5,
            recommended_quantization: "Q4_K_M".into(),
            max_recommended_context_tokens: 16_384,
        }
    } else if total_ram_gb >= 7.5 || (vram_gb >= 3.5 && !hw.is_integrated_gpu) {
        // 8 GB RAM systems
        let fits_in_vram = vram_gb >= 3.5 && !hw.is_integrated_gpu;
        ModelRecommendation {
            recommended_size: "3B".into(),
            recommended_models: vec![
                "llama3.2:3b".into(),
                "qwen2.5-coder:3b".into(),
                "phi-3.5-mini:3.8b".into(),
            ],
            tradeoff_explanation: "A 3B model runs fast and fits comfortably within limited RAM, but will have simpler coding reasoning on complex multi-file architectures.".into(),
            fits_in_vram,
            estimated_ram_required_gb: 3.0,
            recommended_quantization: "Q4_K_M".into(),
            max_recommended_context_tokens: 8_192,
        }
    } else {
        // < 8 GB RAM constrained environments
        let fits_in_vram = vram_gb >= 1.5 && !hw.is_integrated_gpu;
        ModelRecommendation {
            recommended_size: "1B-1.5B".into(),
            recommended_models: vec![
                "llama3.2:1b".into(),
                "qwen2.5-coder:1.5b".into(),
            ],
            tradeoff_explanation: "A 1.5B model fits within constrained RAM with near-zero latency, but can only handle single-step edits and concise instructions.".into(),
            fits_in_vram,
            estimated_ram_required_gb: 1.8,
            recommended_quantization: "Q4_K_M".into(),
            max_recommended_context_tokens: 4_096,
        }
    }
}

// ---------------------------------------------------------------------------
// Platform-specific detection routines
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod win32 {
    #[repr(C)]
    pub struct MemoryStatusEx {
        pub dw_length: u32,
        pub dw_memory_load: u32,
        pub ull_total_phys: u64,
        pub ull_avail_phys: u64,
        pub ull_total_page_file: u64,
        pub ull_avail_page_file: u64,
        pub ull_total_virtual: u64,
        pub ull_avail_virtual: u64,
        pub ull_avail_extended_virtual: u64,
    }

    extern "system" {
        pub fn GlobalMemoryStatusEx(lp_buffer: *mut MemoryStatusEx) -> i32;
        pub fn GetDiskFreeSpaceExW(
            lp_directory_name: *const u16,
            lp_free_bytes_available_to_caller: *mut u64,
            lp_total_number_of_bytes: *mut u64,
            lp_total_number_of_free_bytes: *mut u64,
        ) -> i32;
    }
}

#[cfg(windows)]
fn detect_ram() -> (u64, u64) {
    let mut status = win32::MemoryStatusEx {
        dw_length: std::mem::size_of::<win32::MemoryStatusEx>() as u32,
        dw_memory_load: 0,
        ull_total_phys: 0,
        ull_avail_phys: 0,
        ull_total_page_file: 0,
        ull_avail_page_file: 0,
        ull_total_virtual: 0,
        ull_avail_virtual: 0,
        ull_avail_extended_virtual: 0,
    };

    unsafe {
        if win32::GlobalMemoryStatusEx(&mut status) != 0 {
            return (status.ull_total_phys, status.ull_avail_phys);
        }
    }

    // Fallback if API fails
    (16 * 1024 * 1024 * 1024, 4 * 1024 * 1024 * 1024)
}

#[cfg(not(windows))]
fn detect_ram() -> (u64, u64) {
    if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {
        let mut total = 0u64;
        let mut avail = 0u64;
        for line in content.lines() {
            if line.starts_with("MemTotal:") {
                total = parse_meminfo_line(line);
            } else if line.starts_with("MemAvailable:") {
                avail = parse_meminfo_line(line);
            }
        }
        if total > 0 {
            return (total, avail);
        }
    }
    (16 * 1024 * 1024 * 1024, 8 * 1024 * 1024 * 1024)
}

#[cfg(not(windows))]
fn parse_meminfo_line(line: &str) -> u64 {
    line.split_whitespace()
        .nth(1)
        .and_then(|v| v.parse::<u64>().ok())
        .map(|kb| kb * 1024)
        .unwrap_or(0)
}

#[cfg(windows)]
fn detect_free_disk() -> u64 {
    use std::os::windows::ffi::OsStrExt;
    let path = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);

    let mut free_caller: u64 = 0;
    let mut total: u64 = 0;
    let mut total_free: u64 = 0;

    unsafe {
        if win32::GetDiskFreeSpaceExW(wide.as_ptr(), &mut free_caller, &mut total, &mut total_free)
            != 0
        {
            return free_caller;
        }
    }

    100 * 1024 * 1024 * 1024
}

#[cfg(not(windows))]
fn detect_free_disk() -> u64 {
    100 * 1024 * 1024 * 1024
}

#[cfg(windows)]
fn detect_gpu() -> (String, Option<u64>, bool) {
    // Query Win32_VideoController via PowerShell CIM
    let script = "Get-CimInstance Win32_VideoController | Select-Object -Property Name, AdapterRAM | ConvertTo-Json -Compress";
    if let Ok(output) = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
    {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(text.trim()) {
                // Could be a single object or an array of objects
                let target_obj = if val.is_array() {
                    // Prefer discrete GPU if present
                    val.as_array().and_then(|arr| {
                        arr.iter()
                            .find(|o| {
                                let name = o
                                    .get("Name")
                                    .and_then(|n| n.as_str())
                                    .unwrap_or("")
                                    .to_lowercase();
                                name.contains("nvidia")
                                    || name.contains("geforce")
                                    || name.contains("rtx")
                                    || name.contains("radeon rx")
                            })
                            .or_else(|| arr.first())
                    })
                } else if val.is_object() {
                    Some(&val)
                } else {
                    None
                };

                if let Some(obj) = target_obj {
                    let name = obj
                        .get("Name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("Generic Display Device")
                        .to_string();
                    let vram = obj.get("AdapterRAM").and_then(|r| r.as_u64());
                    let lower = name.to_lowercase();
                    let is_integrated = lower.contains("intel")
                        || lower.contains("iris")
                        || lower.contains("uhd")
                        || lower.contains("integrated")
                        || lower.contains("vega")
                        || lower.contains("radeon(tm) graphics");
                    return (name, vram, is_integrated);
                }
            }
        }
    }

    ("Standard Graphics Adapter".to_string(), None, true)
}

#[cfg(not(windows))]
fn detect_gpu() -> (String, Option<u64>, bool) {
    ("Generic Linux GPU".to_string(), None, true)
}
