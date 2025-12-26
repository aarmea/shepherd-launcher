use super::ipc::IpcServer;
use std::time::Duration;

/// Start the daemon process
pub fn start_daemon() -> Result<(), Box<dyn std::error::Error>> {
    println!("[Daemon] Starting shepherd-launcher daemon...");
    
    let mut ipc_server = IpcServer::new()?;
    println!("[Daemon] IPC server listening on socket");
    
    loop {
        // Handle incoming IPC connections
        match ipc_server.accept_and_handle() {
            Ok(should_shutdown) => {
                if should_shutdown {
                    println!("[Daemon] Shutdown requested, exiting...");
                    break;
                }
            }
            Err(e) => eprintln!("[Daemon] Error handling client: {}", e),
        }
        
        // Sleep briefly to avoid busy-waiting
        std::thread::sleep(Duration::from_millis(10));
    }
    
    println!("[Daemon] Daemon shut down cleanly");
    Ok(())
}
