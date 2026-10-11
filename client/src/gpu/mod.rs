//! GPU Offscreen 3D rendering pipeline for real-time ASCII conversion (Path C).

pub mod context;
pub mod mesh;
pub mod pipeline;
pub mod renderer;
pub mod shaders;
pub mod uniforms;

pub use mesh::{InstanceRaw, SceneBatches};
pub use pipeline::{GpuCell, GpuPipeline, RING_SIZE};
pub use renderer::GpuRenderer;
