use anyhow::Result;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Condvar};
use std::thread::{self, JoinHandle};
use wasmtime::{Engine, Store, Module, Linker};

use crate::wasi_syscalls;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProcessState {
    Running, // Running normally
    Blocked,   // Waiting (yielded)
    Finished,  // Completed execution
}

// ProcessData is the data stored in the Wasmtime store.
#[derive(Clone)]
pub struct ProcessData {
    pub state: Arc<Mutex<ProcessState>>,
    pub cond: Arc<Condvar>,
}

pub struct Process {
    pub thread: JoinHandle<()>,
    pub data: ProcessData,
}

/// Spawns a new OS thread that sets up Wasmtime, registers our custom WASI syscalls,
/// instantiates the module, and calls _start.
/// Before returning, the main thread waits until the WASM code has performed a blocking action.
pub fn start_process(path: PathBuf) -> Result<Process> {
    // Set up the Wasmtime engine and load the module.
    let mut config = wasmtime::Config::new();
    config.consume_fuel(true);
    let engine = Engine::new(&config)?;
    let module = Module::from_file(&engine, &path)?;
    
    // Initially, the process is Unblocked.
    let state = Arc::new(Mutex::new(ProcessState::Running));
    let cond = Arc::new(Condvar::new());
    let process_data = ProcessData { state: state.clone(), cond: cond.clone() };

    // Clone the process data for the thread.
    let thread_data = process_data.clone();

    // Spawn the OS thread.
    let thread = thread::spawn(move || {
        // Create a store with our ProcessData.
        let mut store = Store::new(&engine, thread_data);
        let _ = store.set_fuel(20_000);

        // Create a linker that uses ProcessData.
        let mut linker: Linker<ProcessData> = Linker::new(&engine);
        // Register our custom WASI syscalls.
        wasi_syscalls::register(&mut linker).expect("Failed to register WASI syscalls");

        // Instantiate the module.
        let instance = linker.instantiate(&mut store, &module)
            .expect("Failed to instantiate module");

        // Retrieve the _start function.
        let start_func = instance
            .get_typed_func::<(), ()>(&mut store, "_start")
            .expect("Missing _start function");

        // Run the WASM code.
        // When a blocking call is encountered (e.g. in fd_read),
        // the custom WASI syscall will set state to Blocked and wait.
        let result = start_func.call(&mut store, ());
        if let Err(e) = result {
            eprintln!("Error executing wasm: {:?}", e);
        }

        // When _start returns (or traps), mark the process as Finished.
        {
            let mut s = store.data().state.lock().unwrap();
            *s = ProcessState::Finished;
        }
        store.data().cond.notify_all();
    });

    // In the main thread, wait until the process performs a blocking action.
    {
        let mut guard = state.lock().unwrap();
        // Wait while state is Unblocked.
        guard = cond.wait_while(guard, |s| *s == ProcessState::Running).unwrap();
    }

    Ok(Process { thread, data: process_data })
}