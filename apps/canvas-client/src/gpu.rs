use std::sync::Arc;

use thiserror::Error;
use winit::window::Window;

/// Errors raised while creating or presenting the desktop GPU surface.
#[derive(Debug, Error)]
pub(crate) enum GpuError {
    /// The native window could not be converted into a wgpu surface.
    #[error("could not create GPU surface: {0}")]
    Surface(#[from] wgpu::CreateSurfaceError),
    /// No adapter can present to the native window.
    #[error("could not find a compatible GPU adapter: {0}")]
    Adapter(String),
    /// The selected adapter could not create a device and queue.
    #[error("could not create GPU device: {0}")]
    Device(String),
    /// The surface has no supported configuration for the selected adapter.
    #[error("GPU adapter does not support the Sketchi window surface")]
    UnsupportedSurface,
    /// The temporary executor for wgpu initialization could not be created.
    #[error("could not create GPU initialization runtime: {0}")]
    Runtime(#[from] std::io::Error),
}

/// The wgpu presentation state owned by the native client shell.
pub(crate) struct GpuState {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    adapter_info: wgpu::AdapterInfo,
    egui_renderer: egui_wgpu::Renderer,
}

impl GpuState {
    /// Creates and configures a presentation surface for a native window.
    pub(crate) fn new(window: Arc<Window>, instance: &wgpu::Instance) -> Result<Self, GpuError> {
        let size = window.inner_size();
        let dimensions = surface_dimensions(size.width, size.height).unwrap_or((1, 1));
        let surface = instance.create_surface(window)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let adapter = runtime
            .block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            }))
            .map_err(|error| GpuError::Adapter(error.to_string()))?;
        let adapter_info = adapter.get_info();
        let (device, queue) = runtime
            .block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("Sketchi device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
            }))
            .map_err(|error| GpuError::Device(error.to_string()))?;
        let config = surface
            .get_default_config(&adapter, dimensions.0, dimensions.1)
            .ok_or(GpuError::UnsupportedSurface)?;
        surface.configure(&device, &config);
        let egui_renderer = egui_wgpu::Renderer::new(
            &device,
            config.format,
            egui_wgpu::RendererOptions::default(),
        );

        tracing::debug!(
            width = config.width,
            height = config.height,
            format = ?config.format,
            present_mode = ?config.present_mode,
            "GPU surface configured"
        );

        Ok(Self {
            surface,
            device,
            queue,
            config,
            adapter_info,
            egui_renderer,
        })
    }

    /// Returns information about the adapter selected for presentation.
    pub(crate) const fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.adapter_info
    }

    /// Reconfigures the surface after a native resize.
    pub(crate) fn resize(&mut self, width: u32, height: u32) -> bool {
        let Some((width, height)) = surface_dimensions(width, height) else {
            tracing::debug!(width, height, "skipping zero-sized GPU surface resize");
            return false;
        };
        if self.config.width == width && self.config.height == height {
            return false;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        tracing::debug!(width, height, "GPU surface resized");
        true
    }

    /// Reconfigures the surface with its current dimensions after surface loss.
    pub(crate) fn reconfigure(&self) {
        self.surface.configure(&self.device, &self.config);
    }

    /// Renders one complete egui frame and presents it on the native surface.
    pub(crate) fn render(
        &mut self,
        context: &egui::Context,
        full_output: egui::FullOutput,
    ) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: full_output.pixels_per_point,
        };
        let paint_jobs = context.tessellate(full_output.shapes, full_output.pixels_per_point);
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Sketchi egui frame encoder"),
            });
        for (id, image_delta) in &full_output.textures_delta.set {
            self.egui_renderer
                .update_texture(&self.device, &self.queue, *id, image_delta);
        }
        let user_command_buffers = self.egui_renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &paint_jobs,
            &screen_descriptor,
        );
        let color_attachments = [Some(wgpu::RenderPassColorAttachment {
            view: &view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color {
                    r: 0.04,
                    g: 0.07,
                    b: 0.11,
                    a: 1.0,
                }),
                store: wgpu::StoreOp::Store,
            },
        })];
        {
            let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Sketchi egui frame"),
                color_attachments: &color_attachments,
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.egui_renderer.render(
                &mut render_pass.forget_lifetime(),
                &paint_jobs,
                &screen_descriptor,
            );
        }
        let command_buffer = encoder.finish();
        self.queue
            .submit(user_command_buffers.into_iter().chain([command_buffer]));
        for id in &full_output.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }
        output.present();
        Ok(())
    }
}

fn surface_dimensions(width: u32, height: u32) -> Option<(u32, u32)> {
    (width != 0 && height != 0).then_some((width, height))
}

#[cfg(test)]
mod tests {
    use super::surface_dimensions;

    #[test]
    fn zero_sized_surfaces_are_not_renderable() {
        assert_eq!(surface_dimensions(0, 600), None);
        assert_eq!(surface_dimensions(800, 0), None);
    }

    #[test]
    fn nonzero_surface_dimensions_are_preserved() {
        assert_eq!(surface_dimensions(1024, 768), Some((1024, 768)));
    }
}
