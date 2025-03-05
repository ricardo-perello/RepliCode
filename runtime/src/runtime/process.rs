use anyhow::Result;
use log::{debug, error, info};
use std::{
    fmt, fs::create_dir_all, path::PathBuf, sync::{Arc, Condvar, Mutex}, thread
};
use wasmtime::{Engine, Module, Store, Linker};

use crate::{
    runtime::fd_table::{FDEntry, FDTable},
    wasi_syscalls,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProcessState {
    Running,
    Ready,
    Blocked,
    Finished,
}

impl fmt::Display for ProcessState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
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

#[derive(Clone)]
pub struct ProcessData {
    pub state: Arc<Mutex<ProcessState>>,
    pub cond: Arc<Condvar>,
    pub block_reason: Arc<Mutex<Option<BlockReason>>>,
    pub fd_table: Arc<Mutex<FDTable>>,
    pub root_path: PathBuf,
}

pub struct Process {
    pub id: u64, // Unique process ID
    pub thread: thread::JoinHandle<()>,
    pub data: ProcessData,
}

/// Spawns a new process from a WASM module and assigns it a unique ID.
/// The spawned thread is given a name (e.g. "pid3").
pub fn start_process(path: std::path::PathBuf, id: u64) -> Result<Process> {
    debug!("Starting process with path: {:?} and id: {}", path, id);
    let mut config = wasmtime::Config::new();
    config.consume_fuel(true);
    let engine = Engine::new(&config)?;
    let module = Module::from_file(&engine, &path)?;
    debug!("WASM module loaded from path: {:?}", path);

    // Initialize process state and FD table.
    let state = Arc::new(Mutex::new(ProcessState::Ready));
    let cond = Arc::new(Condvar::new());
    let reason = Arc::new(Mutex::new(None));
    let fd_table = Arc::new(Mutex::new(FDTable::new()));
    {
        let mut table = fd_table.lock().unwrap();
        // Reserve FD 0 for stdin.
        table.entries[0] = Some(FDEntry {
            buffer: Vec::new(),
            read_ptr: 0,
        });
        debug!("FD 0 reserved for stdin");
    }
    let process_root = std::path::PathBuf::from(format!("/tmp/wasm_sandbox/pid_{}", id));
    create_dir_all(&process_root)?;

    let process_data = ProcessData {
        state: state.clone(),
        cond: cond.clone(),
        block_reason: reason,
        fd_table,
        root_path: process_root,
    };
    let thread_data = process_data.clone();
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

            // Log that process initialization is complete.
            debug!("Process id: {} initialization complete, ready to execute _start", id);

            let start_func = instance
                .get_typed_func::<(), ()>(&mut store, "_start")
                .expect("Missing _start function");
            debug!("_start function obtained");
        
            {
                let mut st = store.data().state.lock().unwrap();
                while *st != ProcessState::Running {
                    debug!("Waiting until process {} state is Running (current state: {:?})", id, *st);
                    st = store.data().cond.wait(st).unwrap();
                }
            }

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
        //wait for thread to return then execute the next line
    info!("Started process with id {}", id);
    Ok(Process { id, thread, data: process_data })
}