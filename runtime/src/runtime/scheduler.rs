use anyhow::Result;
use wasmtime::Store;
use crate::runtime::process::Process;

// A very simple round-robin scheduler stub.
// It simply calls the _start function of each process in sequence.
pub fn run_scheduler(processes: Vec<Process>, store: &mut Store<()>) -> Result<()> {
    for process in processes {
        let start = process.instance.get_typed_func::<(), ()>(&mut *store, "_start")?;
        start.call(&mut *store, ())?;
    }
    Ok(())
}