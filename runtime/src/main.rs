mod runtime;
mod wasi_syscalls;

use anyhow::Result;
use wasmtime::*;
use runtime::scheduler::run_scheduler;
use runtime::process::load_process;

fn main() -> Result<()> {
    // Create the Wasmtime engine and load the module.
    let engine = Engine::default();
    let module = Module::from_file(&engine, "../wasm_programs/build/hello.wasm")?;
    let mut store = Store::new(&engine, ());
    
    // Set up the linker and register our custom WASI syscalls.
    let mut linker = Linker::new(&engine);
    wasi_syscalls::register(&mut linker)?;
    
    // Load a process using the module.
    let process_instance = load_process(&mut store, &module, &linker)?;
    
    // Run a simple scheduler (round-robin stub) with the process.
    run_scheduler(vec![process_instance], &mut store)?;
    
    Ok(())
}