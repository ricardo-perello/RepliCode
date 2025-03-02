use std::{fmt, sync::{Arc, Condvar, Mutex}};
use crate::runtime::fd_table::FDTable;
use std::thread::JoinHandle;
use anyhow::Result;
use wasmtime::{Engine, Store, Module, Linker};
use crate::wasi_syscalls;
use log::{info, error, debug};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProcessState {
    Running,
    Ready,
    Blocked,
    Finished,
}

impl fmt::Display for BlockReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BlockReason::StdinRead => write!(f, "StdinRead"),
            BlockReason::Timeout { resume_after } => write!(f, "Timeout until {:?}", resume_after),
            BlockReason::FileIO => write!(f, "FileIO"),
            BlockReason::NetworkIO => write!(f, "NetworkIO"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum BlockReason {
    StdinRead,
    Timeout { resume_after: u64 },
    FileIO,             
    NetworkIO, 
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
        let _ = store.set_fuel(2_000_000);
        let mut linker: Linker<ProcessData> = Linker::new(&engine);
        wasi_syscalls::register(&mut linker).expect("Failed to register WASI syscalls");

        let instance = linker.instantiate(&mut store, &module)
            .expect("Failed to instantiate module");
        let start_func = instance
            .get_typed_func::<(), ()>(&mut store, "_start")
            .expect("Missing _start function");

        if let Err(e) = start_func.call(&mut store, ()) { //TODO this might have to be moved so that call is only called from the scheduler so all processes start at same time
            error!("Error executing wasm: {:?}", e);
        }

        // Mark process as Finished.
        {
            let mut s = store.data().state.lock().unwrap();
            *s = ProcessState::Finished;
        }
        store.data().cond.notify_all();
    });

    info!("Started process with id {}", id);
    Ok(Process { id, thread, data: process_data })
}