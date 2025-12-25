mod clock;

use clock::ClockApp;
use smithay_client_toolkit::{
    compositor::CompositorState,
    output::OutputState,
    registry::RegistryState,
    shell::{
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell},
        WaylandSurface,
    },
    shm::Shm,
};
use wayland_client::globals::registry_queue_init;
use wayland_client::Connection;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut event_queue) = registry_queue_init(&conn)?;
    let qh = event_queue.handle();

    let mut app = ClockApp {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        compositor_state: CompositorState::bind(&globals, &qh)?,
        shm_state: Shm::bind(&globals, &qh)?,
        layer_shell: LayerShell::bind(&globals, &qh)?,
        
        pool: None,
        width: 400,
        height: 200,
        layer_surface: None,
        configured: false,
    };

    // Create the layer surface
    let surface = app.compositor_state.create_surface(&qh);
    let layer_surface = app.layer_shell.create_layer_surface(
        &qh,
        surface,
        Layer::Top,
        Some("clock"),
        None,
    );

    layer_surface.set_anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT);
    layer_surface.set_size(app.width, app.height);
    layer_surface.set_exclusive_zone(app.height as i32);
    layer_surface.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer_surface.commit();

    app.layer_surface = Some(layer_surface);

    loop {
        event_queue.blocking_dispatch(&mut app)?;
        
        if app.configured {
            app.draw(&qh)?;
            // Sleep briefly to reduce CPU usage
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }
}
