use chrono::Local;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_layer, delegate_output, delegate_registry, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        wlr_layer::{LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use wayland_client::{
    protocol::{wl_output, wl_shm, wl_surface},
    Connection, QueueHandle,
};

pub struct ClockApp {
    pub registry_state: RegistryState,
    pub output_state: OutputState,
    pub compositor_state: CompositorState,
    pub shm_state: Shm,
    pub layer_shell: LayerShell,
    
    pub pool: Option<SlotPool>,
    pub width: u32,
    pub height: u32,
    pub layer_surface: Option<LayerSurface>,
    pub configured: bool,
}

impl ClockApp {
    pub fn draw(&mut self, _qh: &QueueHandle<Self>) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(layer_surface) = &self.layer_surface {
            let width = self.width;
            let height = self.height;
            let stride = width as i32 * 4;

            let pool = self.pool.get_or_insert_with(|| {
                SlotPool::new((width * height * 4) as usize, &self.shm_state).unwrap()
            });

            let (buffer, canvas) = pool
                .create_buffer(width as i32, height as i32, stride, wl_shm::Format::Argb8888)
                .unwrap();

            // Get current time
            let now = Local::now();
            let time_str = now.format("%H:%M:%S").to_string();
            let date_str = now.format("%A, %B %d, %Y").to_string();

            // Draw using cairo
            // Safety: We ensure the buffer lifetime is valid for the cairo surface
            unsafe {
                let surface = cairo::ImageSurface::create_for_data_unsafe(
                    canvas.as_mut_ptr(),
                    cairo::Format::ARgb32,
                    width as i32,
                    height as i32,
                    stride,
                )?;

                let ctx = cairo::Context::new(&surface)?;

                // Background
                ctx.set_source_rgb(0.1, 0.1, 0.15);
                ctx.paint()?;

                // Draw time
                ctx.set_source_rgb(1.0, 1.0, 1.0);
                ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
                ctx.set_font_size(60.0);

                let time_extents = ctx.text_extents(&time_str)?;
                let time_x = (width as f64 - time_extents.width()) / 2.0 - time_extents.x_bearing();
                let time_y = height as f64 / 2.0 - 10.0;
                ctx.move_to(time_x, time_y);
                ctx.show_text(&time_str)?;

                // Draw date
                ctx.set_font_size(20.0);
                ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
                let date_extents = ctx.text_extents(&date_str)?;
                let date_x = (width as f64 - date_extents.width()) / 2.0 - date_extents.x_bearing();
                let date_y = height as f64 / 2.0 + 35.0;
                ctx.move_to(date_x, date_y);
                ctx.show_text(&date_str)?;
            }

            layer_surface
                .wl_surface()
                .attach(Some(buffer.wl_buffer()), 0, 0);
            layer_surface.wl_surface().damage_buffer(0, 0, width as i32, height as i32);
            layer_surface.wl_surface().commit();
        }

        Ok(())
    }
}

impl CompositorHandler for ClockApp {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        let _ = self.draw(qh);
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for ClockApp {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

impl LayerShellHandler for ClockApp {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        std::process::exit(0);
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        if configure.new_size.0 != 0 {
            self.width = configure.new_size.0;
        }
        if configure.new_size.1 != 0 {
            self.height = configure.new_size.1;
        }

        self.configured = true;
        let _ = self.draw(qh);
    }
}

impl ShmHandler for ClockApp {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm_state
    }
}

delegate_compositor!(ClockApp);
delegate_output!(ClockApp);
delegate_shm!(ClockApp);
delegate_layer!(ClockApp);

delegate_registry!(ClockApp);

impl ProvidesRegistryState for ClockApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState];
}
