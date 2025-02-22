use anyhow::Result;
use std::fs;
use std::path::PathBuf;

// Import our modules.
mod runtime {
    pub mod process;
    pub mod scheduler;
}
mod wasi_syscalls;

use runtime::process::start_process;
use runtime::scheduler::run_scheduler;

fn main() -> Result<()> {
    // Folder with .wasm files.
    let wasm_folder = "../wasm_programs/build";
    let mut processes = Vec::new();

    // Scan the folder for .wasm files.
    for entry in fs::read_dir(wasm_folder)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("wasm") {
            println!("Found WASM: {:?}", path);
            // Spawn a process (OS thread) for this WASM file.
            let process = start_process(path)?;
            processes.push(process);
        }
    }

    // Run the scheduler on all the processes.
    run_scheduler(processes)?;

    Ok(())
}