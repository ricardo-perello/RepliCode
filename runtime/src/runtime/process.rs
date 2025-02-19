use anyhow::Result;
use wasmtime::{Module, Linker, Store, Instance};

pub struct Process {
    // Wraps a Wasmtime instance representing a WASM process.
    pub instance: Instance,
}

pub fn load_process(store: &mut Store<()>, module: &Module, linker: &Linker<()>) -> Result<Process> {
    let instance = linker.instantiate(store, module)?;
    Ok(Process { instance })
}