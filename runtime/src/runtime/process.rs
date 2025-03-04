use std::{fmt, sync::{Arc, Condvar, Mutex}};
use crate::runtime::fd_table::FDTable;
use anyhow::Result;
use wasmtime::{Engine, Store, Module, Linker};
use log::{debug, error, info};
use crate::wasi_syscalls;
use std::thread;

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
    pub thread: thread::JoinHandle<()>,
    pub data: ProcessData,
}

/// Spawns a new process from a WASM module and assigns it a unique ID.
/// The spawned thread is given a name "pid<id>".
pub fn start_process(path: std::path::PathBuf, id: u64) -> Result<Process> {
    debug!("Starting process with path: {:?} and id: {}", path, id);
    let mut config = wasmtime::Config::new();
    config.consume_fuel(true);
    let engine = Engine::new(&config)?;
    let module = Module::from_file(&engine, &path)?;
    debug!("WASM module loaded from path: {:?}", path);

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
        debug!("FD 0 reserved for stdin");
    }
    let process_data = ProcessData {
        state: state.clone(),
        cond: cond.clone(),
        block_reason: reason,
        fd_table: fd_table,
    };
    let thread_data = process_data.clone();

    // Spawn a new OS thread to run the WASM module.
    // Use a Builder to give the thread a name (e.g. "pid3")
    let thread = thread::Builder::new()
        .name(format!("pid{}", id))
        .spawn(move || {
            debug!(
                "Thread {:?} starting execution for process id: {}",
                thread::current().name().unwrap_or("unknown"),
                id
            );
            let mut store = Store::new(&engine, thread_data);
            let _ = store.set_fuel(2_000_000);
            let mut linker: Linker<ProcessData> = Linker::new(&engine);
            wasi_syscalls::register(&mut linker).expect("Failed to register WASI syscalls");
            debug!("WASI syscalls registered");
            let instance = linker
                .instantiate(&mut store, &module)
                .expect("Failed to instantiate module");
            debug!("WASM module instantiated");
            let start_func = instance
                .get_typed_func::<(), ()>(&mut store, "_start")
                .expect("Missing _start function");
            debug!("_start function obtained");
            if let Err(e) = start_func.call(&mut store, ()) {
                error!("Error executing wasm: {:?}", e);
            }
            {
                let mut s = store.data().state.lock().unwrap();
                *s = ProcessState::Finished;
            }
            store.data().cond.notify_all();
            debug!("Process id: {} marked as Finished", id);
        })?;
    info!("Started process with id {}", id);
    Ok(Process { id, thread, data: process_data })
}