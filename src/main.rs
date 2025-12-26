mod daemon;
mod ui;

use std::env;
use std::process::{Command, Stdio};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    
    // Check if we're running as the daemon
    if args.len() > 1 && args[1] == "--daemon" {
        return daemon::start_daemon();
    }
    
    // Spawn the daemon process
    println!("[Main] Spawning daemon process...");
    let mut daemon_child = Command::new(&args[0])
        .arg("--daemon")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;
    
    let daemon_pid = daemon_child.id();
    println!("[Main] Daemon spawned with PID: {}", daemon_pid);
    
    // Give the daemon a moment to start up
    std::thread::sleep(std::time::Duration::from_millis(100));
    
    // Test the IPC connection
    println!("[Main] Testing IPC connection...");
    match daemon::IpcClient::send_message(&daemon::IpcMessage::Ping) {
        Ok(daemon::IpcResponse::Pong) => println!("[Main] IPC connection successful!"),
        Ok(response) => println!("[Main] Unexpected response: {:?}", response),
        Err(e) => println!("[Main] IPC connection failed: {}", e),
    }
    
    // Start the UI
    println!("[Main] Starting UI...");
    let ui_result = ui::run();
    
    // UI has exited, shut down the daemon
    println!("[Main] UI exited, shutting down daemon...");
    match daemon::IpcClient::send_message(&daemon::IpcMessage::Shutdown) {
        Ok(daemon::IpcResponse::ShuttingDown) => {
            println!("[Main] Daemon acknowledged shutdown");
        }
        Ok(response) => {
            println!("[Main] Unexpected shutdown response: {:?}", response);
        }
        Err(e) => {
            eprintln!("[Main] Failed to send shutdown to daemon: {}", e);
        }
    }
    
    // Wait for daemon to exit (with timeout)
    let wait_start = std::time::Instant::now();
    loop {
        match daemon_child.try_wait() {
            Ok(Some(status)) => {
                println!("[Main] Daemon exited with status: {}", status);
                break;
            }
            Ok(None) => {
                if wait_start.elapsed().as_secs() > 5 {
                    eprintln!("[Main] Daemon did not exit in time, killing it");
                    let _ = daemon_child.kill();
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => {
                eprintln!("[Main] Error waiting for daemon: {}", e);
                break;
            }
        }
    }
    
    ui_result
}
