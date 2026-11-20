use oblivion_core::hardware::{detect_hardware, recommend_model, HardwareInfo};

#[test]
fn detect_hardware_returns_valid_specs() {
    let hw = detect_hardware();
    // On any real machine or CI, total RAM must be greater than zero
    assert!(hw.total_ram_bytes > 0, "total_ram_bytes must be > 0");
    assert!(hw.total_ram_gb > 0.0, "total_ram_gb must be > 0.0");
    // Free disk space must be greater than zero
    assert!(hw.free_disk_bytes > 0, "free_disk_bytes must be > 0");
    assert!(hw.free_disk_gb > 0.0, "free_disk_gb must be > 0.0");
    // GPU name should not be empty
    assert!(!hw.gpu_name.is_empty(), "gpu_name must not be empty");
}

#[test]
fn recommend_model_small_ram() {
    let hw = HardwareInfo {
        total_ram_bytes: 4 * 1024 * 1024 * 1024, // 4 GB
        available_ram_bytes: 2 * 1024 * 1024 * 1024,
        total_ram_gb: 4.0,
        available_ram_gb: 2.0,
        gpu_name: "Generic Graphics".into(),
        vram_bytes: None,
        vram_gb: None,
        is_integrated_gpu: true,
        free_disk_bytes: 50 * 1024 * 1024 * 1024,
        free_disk_gb: 50.0,
    };
    let rec = recommend_model(&hw);
    assert_eq!(rec.recommended_size, "1B-1.5B");
    assert!(rec.recommended_models.iter().any(|m| m.contains("1")));
    assert!(!rec.fits_in_vram);
    assert!(rec.tradeoff_explanation.contains("RAM"));
}

#[test]
fn recommend_model_medium_ram_8gb() {
    let hw = HardwareInfo {
        total_ram_bytes: 8 * 1024 * 1024 * 1024, // 8 GB
        available_ram_bytes: 4 * 1024 * 1024 * 1024,
        total_ram_gb: 8.0,
        available_ram_gb: 4.0,
        gpu_name: "Intel UHD Graphics".into(),
        vram_bytes: Some(1024 * 1024 * 1024),
        vram_gb: Some(1.0),
        is_integrated_gpu: true,
        free_disk_bytes: 100 * 1024 * 1024 * 1024,
        free_disk_gb: 100.0,
    };
    let rec = recommend_model(&hw);
    assert_eq!(rec.recommended_size, "3B");
    assert!(rec.recommended_models.iter().any(|m| m.contains("3")));
    assert!(!rec.fits_in_vram);
}

#[test]
fn recommend_model_sweet_spot_16gb() {
    let hw = HardwareInfo {
        total_ram_bytes: 16 * 1024 * 1024 * 1024, // 16 GB
        available_ram_bytes: 8 * 1024 * 1024 * 1024,
        total_ram_gb: 16.0,
        available_ram_gb: 8.0,
        gpu_name: "Intel(R) Iris(R) Xe Graphics".into(),
        vram_bytes: Some(1024 * 1024 * 1024),
        vram_gb: Some(1.0),
        is_integrated_gpu: true,
        free_disk_bytes: 500 * 1024 * 1024 * 1024,
        free_disk_gb: 500.0,
    };
    let rec = recommend_model(&hw);
    assert_eq!(rec.recommended_size, "7B-8B");
    assert!(rec
        .recommended_models
        .iter()
        .any(|m| m.contains("7b") || m.contains("8b")));
    assert!(!rec.fits_in_vram);
    assert!(
        rec.tradeoff_explanation.contains("16 GB RAM") || rec.tradeoff_explanation.contains("8B")
    );
}

#[test]
fn recommend_model_dedicated_gpu_rtx() {
    let hw = HardwareInfo {
        total_ram_bytes: 32 * 1024 * 1024 * 1024, // 32 GB
        available_ram_bytes: 20 * 1024 * 1024 * 1024,
        total_ram_gb: 32.0,
        available_ram_gb: 20.0,
        gpu_name: "NVIDIA GeForce RTX 4080".into(),
        vram_bytes: Some(16 * 1024 * 1024 * 1024),
        vram_gb: Some(16.0),
        is_integrated_gpu: false,
        free_disk_bytes: 1000 * 1024 * 1024 * 1024,
        free_disk_gb: 1000.0,
    };
    let rec = recommend_model(&hw);
    assert!(rec.recommended_size == "14B" || rec.recommended_size == "32B");
    assert!(rec.fits_in_vram);
}
