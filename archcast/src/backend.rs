//! Render backend abstraction:
//! - Default `CpuSimdBackend`: SIMD (`memchr` + `wide::f64x4`) + Rayon parallel CPU pipeline.
//! - Optional `WebGpuBackend` (`--features gpu`): `wgpu` adapter probe & Phase 2 GPU raster/canvas hook.

use crate::error::{ArchcastError, Result};
use crate::layout::{LaidOutDiagram, layout_diagrams_parallel};
use crate::render_html::render_diagram_svg;
use crate::schema::DiagramSpec;
use crate::simd_ops::simd_profile;
use clap::ValueEnum;
use rayon::prelude::*;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendChoice {
    /// Prefer GPU if compiled with `--features gpu` and an adapter is present; otherwise CPU SIMD+Rayon.
    #[default]
    Auto,
    /// Pure headless CPU SIMD + Rayon backend.
    Cpu,
    /// Require WebGPU (`--features gpu` + hardware/software adapter).
    Gpu,
}

#[derive(Debug, Clone, Serialize)]
pub struct BackendInfo {
    pub active_backend: String,
    pub simd_profile: String,
    pub rayon_threads: usize,
    pub gpu_feature_compiled: bool,
    pub gpu_adapter: Option<String>,
    pub notes: String,
}

pub trait RenderBackend: Send + Sync {
    fn info(&self) -> BackendInfo;
    fn is_gpu(&self) -> bool {
        false
    }
    fn render_diagrams(&self, diagrams: &[DiagramSpec]) -> Vec<(LaidOutDiagram, String)>;
}

/// Default headless backend combining SIMD (`memchr` + `wide::f64x4`) and Rayon parallelism.
pub struct CpuSimdBackend;

impl RenderBackend for CpuSimdBackend {
    fn info(&self) -> BackendInfo {
        BackendInfo {
            active_backend: "cpu-simd-rayon".to_string(),
            simd_profile: simd_profile().to_string(),
            rayon_threads: rayon::current_num_threads(),
            gpu_feature_compiled: cfg!(feature = "gpu"),
            gpu_adapter: None,
            notes: "Deterministic headless SIMD + Rayon layout and SVG emitter.".to_string(),
        }
    }

    fn render_diagrams(&self, diagrams: &[DiagramSpec]) -> Vec<(LaidOutDiagram, String)> {
        let laid = layout_diagrams_parallel(diagrams);
        if laid.len() <= 1 {
            laid.into_iter()
                .map(|d| {
                    let svg = render_diagram_svg(&d);
                    (d, svg)
                })
                .collect()
        } else {
            laid.into_par_iter()
                .map(|d| {
                    let svg = render_diagram_svg(&d);
                    (d, svg)
                })
                .collect()
        }
    }
}

#[cfg(feature = "gpu")]
pub struct WebGpuBackend {
    adapter_name: String,
}

#[cfg(feature = "gpu")]
impl WebGpuBackend {
    pub fn probe() -> Option<Self> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))?;
        let info = adapter.get_info();
        Some(Self {
            adapter_name: format!("{} ({:?})", info.name, info.backend),
        })
    }
}

#[cfg(feature = "gpu")]
impl RenderBackend for WebGpuBackend {
    fn info(&self) -> BackendInfo {
        BackendInfo {
            active_backend: "webgpu-hybrid".to_string(),
            simd_profile: simd_profile().to_string(),
            rayon_threads: rayon::current_num_threads(),
            gpu_feature_compiled: true,
            gpu_adapter: Some(self.adapter_name.clone()),
            notes: "WebGPU WGSL compute shader active for animated GIF compositing; SVG layout & XML execute via SIMD+Rayon.".to_string(),
        }
    }

    fn is_gpu(&self) -> bool {
        true
    }

    fn render_diagrams(&self, diagrams: &[DiagramSpec]) -> Vec<(LaidOutDiagram, String)> {
        CpuSimdBackend.render_diagrams(diagrams)
    }
}

/// Resolve a concrete backend from CLI preference.
pub fn resolve_backend(choice: BackendChoice) -> Result<Box<dyn RenderBackend>> {
    match choice {
        BackendChoice::Cpu => Ok(Box::new(CpuSimdBackend)),
        BackendChoice::Auto => {
            #[cfg(feature = "gpu")]
            {
                if let Some(gpu) = WebGpuBackend::probe() {
                    return Ok(Box::new(gpu));
                }
            }
            Ok(Box::new(CpuSimdBackend))
        }
        BackendChoice::Gpu => {
            #[cfg(feature = "gpu")]
            {
                WebGpuBackend::probe().map(|b| Box::new(b) as Box<dyn RenderBackend>).ok_or_else(
                    || {
                        ArchcastError::BackendUnavailable(
                            "no WebGPU adapter available on this host; rerun with --backend cpu or --backend auto".to_string(),
                        )
                    },
                )
            }
            #[cfg(not(feature = "gpu"))]
            {
                Err(ArchcastError::BackendUnavailable(
                    "archcast was built without `--features gpu`; rebuild with `cargo build --features gpu` or use `--backend cpu`".to_string(),
                ))
            }
        }
    }
}
