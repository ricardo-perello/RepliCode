use anyhow::Result;
use std::fs;
use std::io::{self, Read};
mod runtime {
    pub mod process;
    pub mod scheduler;
    pub mod fd_table;
}
mod wasi_syscalls;

use runtime::process::start_process;
use runtime::scheduler::run_scheduler;


fn main() -> Result<()> {
    // Folder containing the compiled WASM programs.
    let wasm_folder = "../wasm_programs/build";
    let mut processes = Vec::new();

    // Discover all WASM files.
    for entry in fs::read_dir(wasm_folder)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("wasm") {
            println!("Found WASM: {:?}", path);
            // Spawn the process. In our design, each process
            // has its own FD table (FD 0 for stdin, etc.).
            let process = start_process(path)?;
            processes.push(process);
        }
    }

    // Run the round-robin scheduler.
    run_scheduler(processes)?;

    Ok(())
}
