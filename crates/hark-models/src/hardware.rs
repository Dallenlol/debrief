use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GpuBackend {
    Cuda,
    Metal,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Gpu {
    pub name: String,
    pub vram_gb: f32,
    pub backend: GpuBackend,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Hardware {
    pub cpu_cores: usize,
    pub cpu_name: String,
    pub ram_gb: f32,
    pub gpu: Option<Gpu>,
    pub os: String,
    pub arch: String,
}

/// Best-effort hardware probe. Never fails; unknowns become `None`/0.
pub fn probe() -> Hardware {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    sys.refresh_cpu_list(sysinfo::CpuRefreshKind::nothing());
    let cpu_name = sys.cpus().first().map(|c| c.brand().trim().to_string()).unwrap_or_default();
    Hardware {
        cpu_cores: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
        cpu_name,
        ram_gb: sys.total_memory() as f32 / 1024.0 / 1024.0 / 1024.0,
        gpu: probe_gpu(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
    }
}

#[cfg(windows)]
fn probe_gpu() -> Option<Gpu> {
    let nvml = nvml_wrapper::Nvml::init().ok()?;
    let dev = nvml.device_by_index(0).ok()?;
    let name = dev.name().ok()?;
    let mem = dev.memory_info().ok()?;
    Some(Gpu { name, vram_gb: mem.total as f32 / 1024.0 / 1024.0 / 1024.0, backend: GpuBackend::Cuda })
}

#[cfg(target_os = "macos")]
fn probe_gpu() -> Option<Gpu> {
    // Apple Silicon shares system memory with the GPU; treat all of it as usable.
    if std::env::consts::ARCH == "aarch64" {
        let mut sys = sysinfo::System::new();
        sys.refresh_memory();
        Some(Gpu {
            name: "Apple Silicon (Metal)".into(),
            vram_gb: sys.total_memory() as f32 / 1024.0 / 1024.0 / 1024.0,
            backend: GpuBackend::Metal,
        })
    } else {
        None
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn probe_gpu() -> Option<Gpu> {
    None
}
