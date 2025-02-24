// runtime/src/runtime/process.rs
use std::sync::{Arc, Mutex, Condvar};
use crate::runtime::fd_table::FDTable;
use std::thread::JoinHandle;
use std::time::Instant;
use anyhow::Result;
use wasmtime::{Engine, Store, Module, Linker};

use crate::wasi_syscalls;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProcessState {
    Running,
    Ready,
    Blocked,
    Finished,
}

#[derive(Debug, Clone)]
pub enum BlockReason {
    StdinRead,
    Timeout { resume_after: Instant },
}

/// ProcessData is stored inside each Wasmtime store.
#[derive(Clone)]
pub struct ProcessData {
    pub state: Arc<Mutex<ProcessState>>,
    pub cond: Arc<Condvar>,
    pub block_reason: Arc<Mutex<Option<BlockReason>>>,
    pub fd_table: Arc<Mutex<FDTable>>,
}

/// Process encapsulates a running process.
pub struct Process {
    pub id: u64,  // Unique process ID
    pub thread: JoinHandle<()>,
    pub data: ProcessData,
}

/// Spawns a new process from a WASM module and assigns it a unique ID.
pub fn start_process(path: std::path::PathBuf, id: u64) -> Result<Process> {
    let mut config = wasmtime::Config::new();
    config.consume_fuel(true);
    let engine = Engine::new(&config)?;
    let module = Module::from_file(&engine, &path)?;

    // Initialize process state and FD table.
    let state = Arc::new(Mutex::new(ProcessState::Running));
    let cond = Arc::new(Condvar::new());
    let reason = Arc::new(Mutex::new(None));
    let fd_table = Arc::new(Mutex::new(FDTable::new()));
    {
        let mut table = fd_table.lock().unwrap();
        // Reserve FD 0 for stdin.
        table.entries[0] = Some(crate::runtime::fd_table::FDEntry {
            buffer: Vec::new(),
            read_ptr: 0,
        });
    }
    let process_data = ProcessData {
        state: state.clone(),
        cond: cond.clone(),
        block_reason: reason,
        fd_table: fd_table,
    };

    let thread_data = process_data.clone();

    // Spawn a new OS thread to run the WASM module.
    let thread = std::thread::spawn(move || {
        let mut store = Store::new(&engine, thread_data);
        let _ = store.set_fuel(20_000);
        let mut linker: Linker<ProcessData> = Linker::new(&engine);
        wasi_syscalls::register(&mut linker).expect("Failed to register WASI syscalls");

        let instance = linker.instantiate(&mut store, &module)
            .expect("Failed to instantiate module");
        let start_func = instance
            .get_typed_func::<(), ()>(&mut store, "_start")
            .expect("Missing _start function");

        if let Err(e) = start_func.call(&mut store, ()) {
            eprintln!("Error executing wasm: {:?}", e);
        }

        // Mark process as Finished.
        {
            let mut s = store.data().state.lock().unwrap();
            *s = ProcessState::Finished;
        }
        store.data().cond.notify_all();
    });

    Ok(Process { id, thread, data: process_data })
}
