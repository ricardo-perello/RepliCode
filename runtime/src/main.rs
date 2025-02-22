use anyhow::Result;
use std::fs;
use std::path::PathBuf;
mod runtime {
    pub mod process;
    pub mod scheduler;
}
mod wasi_syscalls;

use runtime::process::start_process;
use runtime::scheduler::run_scheduler;
use std::thread;

fn spawn_input_thread() {
    // Spawn a dedicated thread to read from stdin.
    thread::spawn(|| {
        use std::io::{self, Read};
        use once_cell::sync::Lazy;
        use std::sync::{Mutex, Condvar};
        // GLOBAL_INPUT is defined in wasi_syscalls/fd.rs.
        use crate::wasi_syscalls::fd::GLOBAL_INPUT;
        let (lock, cond) = &*GLOBAL_INPUT;
        let mut stdin = io::stdin();
        loop {
            let mut buffer = [0u8; 1024];
            match stdin.read(&mut buffer) {
                Ok(0) => {
                    eprintln!("Got 0 bytes; treating as EOF. Exiting input loop.");
                    break;
                }
                Ok(n) => {
                    let mut data = lock.lock().unwrap();
                    data.extend_from_slice(&buffer[..n]);
                    cond.notify_all();
                }
                Err(e) => {
                    eprintln!("Error reading stdin: {:?}", e);
                    // Possibly break or keep going
                    break;
                }
            }
        }
    });
}

fn main() -> Result<()> {
    // Spawn the input thread so that user input can be captured.
    spawn_input_thread();

    let wasm_folder = "../wasm_programs/build";
    let mut processes = Vec::new();

    // Find all WASM files.
    for entry in fs::read_dir(wasm_folder)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("wasm") {
            println!("Found WASM: {:?}", path);
            // Spawn a process (this will block until the process first blocks on input).
            let process = start_process(path)?;
            processes.push(process);
        }
    }

    // Run the scheduler over the processes.
    run_scheduler(processes)?;

    Ok(())
}