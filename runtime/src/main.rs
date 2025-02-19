mod runtime;
mod wasi_syscalls;

use anyhow::Result;
use wasmtime::*;
use runtime::scheduler::run_scheduler;
use runtime::process::load_process;

fn main() -> Result<()> {
    // Create the Wasmtime engine and load the module.
    let mut config = Config::new();
    config.consume_fuel(true);
    let engine = Engine::new(&config)?;
    let module = Module::from_file(&engine, "../wasm_programs/build/hello.wasm")?; //TODO in the future we should have a generic one that takes all .wasm files from specified folder 
    let mut store = Store::new(&engine, ());
    let _ = store.set_fuel(20_000);
    
    // Set up the linker and register our custom WASI syscalls.
    let mut linker = Linker::new(&engine);
    wasi_syscalls::register(&mut linker)?;
    
    // Load a process using the module.
    let process_instance = load_process(&mut store, &module, &linker)?;
    
    // Run a simple scheduler (round-robin stub) with the process.
    run_scheduler(vec![process_instance], &mut store)?;
    
    Ok(())
}