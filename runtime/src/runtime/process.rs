use anyhow::Result;
use std::sync::{Arc, Mutex, Condvar};
use std::thread::{JoinHandle, self};
use std::path::PathBuf;
use wasmtime::{Engine, Store, Module, Linker};
use crate::wasi_syscalls::register;

#[derive(Debug, Clone, Copy)] //TODO check with Gauthier if this is ok to do.
pub enum ProcessState {
    Running,
    Blocked,
    Finished,
}

pub struct Process {
    pub thread: JoinHandle<()>,
    pub state: Arc<Mutex<ProcessState>>,
    pub cond: Arc<Condvar>,
    // Possibly store additional info here, like the wasmtime::Store, etc.
}

pub fn start_process(path: PathBuf) -> Result<Process> {
    // 1) Build engine, linker, store, etc.
    let mut config = wasmtime::Config::new();
    config.consume_fuel(true);
    let engine = Engine::new(&config)?;

    let module = Module::from_file(&engine, &path)?;
    
    // For blocked/unblocked tracking
    let state = Arc::new(Mutex::new(ProcessState::Running));
    let cond = Arc::new(Condvar::new());

    // 2) Spawn the OS thread
    let thread_state = Arc::clone(&state);
    let thread_cond = Arc::clone(&cond);
    let thread = thread::spawn(move || {
        let mut store = Store::new(&engine, ());
        let _ = store.set_fuel(20_000);

        let mut linker = Linker::new(&engine);
        let _ = register(&mut linker); // register custom WASI

        // Instantiate the module
        let instance = linker.instantiate(&mut store, &module)
            .expect("Failed to instantiate WASM");

        // Call _start
        let start = instance
            .get_typed_func::<(), ()>(&mut store, "_start")
            .expect("Missing _start function");
        
        let result = start.call(&mut store, ());
        if let Err(e) = result {
            // If a trap => handle block, OOM, etc. or just log
            eprintln!("Trap or error: {e}");
        }

        // Mark it as finished so the scheduler knows
        let mut s = thread_state.lock().unwrap();
        *s = ProcessState::Finished;
        thread_cond.notify_all();
    });

    Ok(Process {
        thread,
        state,
        cond,
    })
}
