mod runtime;
mod wasi_syscalls;

use std::fs;
use std::path::PathBuf;
use anyhow::Result;
use crate::runtime::process::start_process;
use crate::runtime::scheduler::run_scheduler;

fn main() -> Result<()> {
    let wasm_folder = "../wasm_programs/build";

    // Gather .wasm paths
    let mut processes = Vec::new();
    for entry in fs::read_dir(wasm_folder)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().unwrap_or_default() == "wasm" {
            // Spawn a thread/process for this WASM
            let process = start_process(path)?;
            processes.push(process);
        }
    }

    // Now run your scheduler on those processes
    run_scheduler(processes)?;

    Ok(())
}