//! Headless WGPU context for offscreen 3D rendering to ASCII terminal buffers.

use std::sync::Arc;

pub struct GpuContext {
    pub instance: wgpu::Instance,
    pub adapter: Arc<wgpu::Adapter>,
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
}

impl GpuContext {
    /// Attempt to initialize headless WGPU context.
    pub fn init() -> Result<Self, String> {
        let instance = wgpu::Instance::default();
        let adapter_opt =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            }));

        let adapter = match adapter_opt {
            Some(a) => a,
            None => {
                // Fallback to low-power or software adapter if high-performance unavailable
                pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::LowPower,
                    compatible_surface: None,
                    force_fallback_adapter: true,
                }))
                .ok_or_else(|| "No compatible WGPU graphics adapter found".to_string())?
            }
        };

        let adapter_info = adapter.get_info();
        log_adapter_info(&adapter_info);

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("ASCII MMORPG Offscreen Device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
            },
            None,
        ))
        .map_err(|e| format!("Failed to create WGPU device: {e}"))?;

        Ok(Self {
            instance,
            adapter: Arc::new(adapter),
            device: Arc::new(device),
            queue: Arc::new(queue),
        })
    }
}

fn log_adapter_info(info: &wgpu::AdapterInfo) {
    let _ = info;
    // Silent in release, or can trace
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gpu_context_init() {
        let ctx = GpuContext::init();
        assert!(
            ctx.is_ok(),
            "GPU context should initialize successfully: {:?}",
            ctx.err()
        );
    }
}
