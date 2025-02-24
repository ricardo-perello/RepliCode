use anyhow::Result;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Condvar};
use std::thread::{self, JoinHandle};
use wasmtime::{Engine, Store, Module, Linker};

use crate::wasi_syscalls;
use crate::runtime::fd_table::FDTable;

/// ProcessState describes the current state of a process.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProcessState {
    Running,
    Ready,
    Blocked,
    Finished,
}

/// Reasons why a process might block.
#[derive(Debug, Clone)]
pub enum BlockReason {
    StdinRead,
    Timeout { resume_after: std::time::Instant },
}

/// ProcessData is stored inside each Wasmtime store and shared with WASI syscalls.
#[derive(Clone)]
pub struct ProcessData {
    pub state: Arc<Mutex<ProcessState>>,
    pub cond: Arc<Condvar>,
    pub block_reason: Arc<Mutex<Option<BlockReason>>>,
    pub fd_table: Arc<Mutex<FDTable>>,
}

/// Process encapsulates a running process.
pub struct Process {
    pub thread: JoinHandle<()>,
    pub data: ProcessData,
}

/// Spawns a new process by instantiating a WASM module and running its _start function.
/// Each process gets its own FD table (with FD 0 reserved for stdin).
pub fn start_process(path: PathBuf) -> Result<Process> {
    let mut config = wasmtime::Config::new();
    config.consume_fuel(true);
    let engine = Engine::new(&config)?;
    let module = Module::from_file(&engine, &path)?;

    // Initialize process state and the FD table.
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
    let thread = thread::spawn(move || {
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

        // When _start returns, mark the process as Finished.
        {
            let mut s = store.data().state.lock().unwrap();
            *s = ProcessState::Finished;
        }
        store.data().cond.notify_all();
    });

    Ok(Process { thread, data: process_data })
}
