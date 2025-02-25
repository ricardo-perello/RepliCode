use anyhow::Result;
use std::fs;

// Declare your modules so Rust knows where to find them.
mod consensus_input;
mod runtime;
mod wasi_syscalls;

use runtime::process::start_process;
use runtime::scheduler::run_scheduler;

fn main() -> Result<()> {
    // Folder containing WASM modules.
    let wasm_folder = "../wasm_programs/build";
    let mut processes = Vec::new();
    let mut next_id = 1; // Unique process IDs starting from 1

    // Discover and spawn processes.
    for entry in fs::read_dir(wasm_folder)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("wasm") {
            println!("Found WASM: {:?}", path);
            // Spawn the process with a unique ID.
            let process = start_process(path, next_id)?; //TODO sometimes spawns the thread before all the wasms are found
            next_id += 1;
            processes.push(process);
        }
    }

    // Process consensus input from a binary file.
    // This file contains records for multiple processes.
    // let consensus_file = "../consensus/consensus_input.bin";
    // process_consensus_file(consensus_file, &mut processes)?;
    // TODO

    // Run the scheduler (which will, for example, unblock processes waiting for input).
    run_scheduler(processes)?;

    Ok(())
}
