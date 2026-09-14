use crate::hardware::Hardware;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    CpuLow,
    CpuHigh,
    Gpu,
}

/// Pick a tier from probed hardware. Overridable by the user in settings.
pub fn select_tier(h: &Hardware) -> Tier {
    if h.gpu.as_ref().map(|g| g.vram_gb >= 6.0).unwrap_or(false) {
        Tier::Gpu
    } else if h.ram_gb >= 12.0 {
        Tier::CpuHigh
    } else {
        Tier::CpuLow
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ModelKind {
    Asr,
    Llm,
    Embedding,
    Diarize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelSpec {
    pub id: String,
    pub kind: ModelKind,
    pub name: String,
    pub file: String,
    pub url: String,
    pub sha256: String,
    pub size_bytes: u64,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TierModels {
    pub asr_live: String,
    pub asr_quality: String,
    pub llm: String,
}

#[derive(Debug, Clone, Deserialize)]
struct CatalogFile {
    models: Vec<ModelSpec>,
    tiers: HashMap<Tier, TierModels>,
    #[serde(default)]
    common: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Catalog {
    models: Vec<ModelSpec>,
    tiers: HashMap<Tier, TierModels>,
    /// Models every tier needs (diarization).
    common: Vec<String>,
}

const BUILTIN: &str = include_str!("../catalog.json");

impl Catalog {
    pub fn builtin() -> Catalog {
        let f: CatalogFile = serde_json::from_str(BUILTIN).expect("catalog.json is valid");
        Catalog { models: f.models, tiers: f.tiers, common: f.common }
    }

    pub fn all(&self) -> &[ModelSpec] {
        &self.models
    }

    pub fn get(&self, id: &str) -> Option<&ModelSpec> {
        self.models.iter().find(|m| m.id == id)
    }

    pub fn common_ids(&self) -> &[String] {
        &self.common
    }

    pub fn tier_models(&self, tier: Tier) -> &TierModels {
        &self.tiers[&tier]
    }

    /// The models a tier needs, in download order (live ASR first so captions work soonest).
    pub fn for_tier(&self, tier: Tier) -> Vec<&ModelSpec> {
        let t = self.tier_models(tier);
        let mut ids = vec![t.asr_live.as_str(), t.asr_quality.as_str()];
        ids.extend(self.common.iter().map(String::as_str));
        ids.push(t.llm.as_str());
        ids.dedup();
        ids.into_iter().filter_map(|id| self.get(id)).collect()
    }
}

pub fn model_path(models_dir: &Path, spec: &ModelSpec) -> PathBuf {
    models_dir.join(&spec.file)
}

/// Present and byte-size matches (a full hash check is done at download time).
pub fn is_present(models_dir: &Path, spec: &ModelSpec) -> bool {
    std::fs::metadata(model_path(models_dir, spec)).map(|m| m.len() == spec.size_bytes).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::{Gpu, GpuBackend};

    fn hw(ram: f32, vram: Option<f32>) -> Hardware {
        Hardware {
            cpu_cores: 8,
            cpu_name: "test".into(),
            ram_gb: ram,
            gpu: vram.map(|v| Gpu { name: "g".into(), vram_gb: v, backend: GpuBackend::Cuda }),
            os: "test".into(),
            arch: "x86_64".into(),
        }
    }

    #[test]
    fn tier_selection() {
        assert_eq!(select_tier(&hw(8.0, None)), Tier::CpuLow);
        assert_eq!(select_tier(&hw(16.0, None)), Tier::CpuHigh);
        assert_eq!(select_tier(&hw(16.0, Some(4.0))), Tier::CpuHigh);
        assert_eq!(select_tier(&hw(8.0, Some(10.0))), Tier::Gpu);
    }

    #[test]
    fn catalog_tiers_resolve_to_known_models() {
        let c = Catalog::builtin();
        for t in [Tier::CpuLow, Tier::CpuHigh, Tier::Gpu] {
            let m = c.for_tier(t);
            assert!(m.len() >= 2, "{t:?}");
            assert!(m.iter().any(|s| s.kind == ModelKind::Asr));
            assert!(m.iter().any(|s| s.kind == ModelKind::Llm));
            assert_eq!(m.iter().filter(|s| s.kind == ModelKind::Diarize).count(), 2);
        }
        let gpu: Vec<&str> = c.for_tier(Tier::Gpu).iter().map(|m| m.id.as_str()).collect();
        assert!(gpu.contains(&"whisper-large-v3-turbo"));
        assert!(gpu.contains(&"qwen3-8b-q4"));
        assert!(c.all().iter().all(|m| m.sha256.len() == 64 && m.size_bytes > 0));
    }

    #[test]
    fn presence_checks_size() {
        let d = tempfile::tempdir().unwrap();
        let spec = ModelSpec {
            id: "x".into(),
            kind: ModelKind::Asr,
            name: "x".into(),
            file: "x.bin".into(),
            url: String::new(),
            sha256: String::new(),
            size_bytes: 3,
            note: String::new(),
        };
        assert!(!is_present(d.path(), &spec));
        std::fs::write(model_path(d.path(), &spec), b"abc").unwrap();
        assert!(is_present(d.path(), &spec));
        std::fs::write(model_path(d.path(), &spec), b"ab").unwrap();
        assert!(!is_present(d.path(), &spec));
    }
}
