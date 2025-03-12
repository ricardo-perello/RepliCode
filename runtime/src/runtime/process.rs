use anyhow::Result;
use wasmtime::{Engine, Module, Store, Linker};
use std::sync::{Arc, Mutex, Condvar};
use std::{thread, fs};
use std::path::PathBuf;
use log::{debug, error, info};
use crate::{
    runtime::fd_table::{FDEntry, FDTable},
    wasi_syscalls,
};
// In this example we define our own minimal versions of process state and block reasons.
// You can extend these as needed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProcessState {
    Ready,
    Running,
    Blocked,
    Finished,
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
    // Assumes you have an FD table defined in a module called `fd_table`.
    pub fd_table: Arc<Mutex<FDTable>>,
    pub root_path: PathBuf,
}

pub struct Process {
    pub id: u64,
    pub thread: thread::JoinHandle<()>,
    pub data: ProcessData,
}

/// Creates a new process from a WASM binary (passed as a byte vector) and assigns it a unique ID.
pub fn start_process_from_bytes(wasm_bytes: Vec<u8>, id: u64) -> Result<Process> {
    debug!("Starting process {} from WASM bytes", id);
    let mut config = wasmtime::Config::new();
    debug!("WASM config created");
    config.consume_fuel(true);
    let engine = Engine::new(&config)?;
    debug!("WASM engine created");
    // Load the module from the in-memory bytes.
    let module = Module::new(&engine, &wasm_bytes)?;
    debug!("WASM module loaded from bytes");

    // Initialize process state and associated resources.
    let state = Arc::new(Mutex::new(ProcessState::Ready));
    let cond = Arc::new(Condvar::new());
    let block_reason = Arc::new(Mutex::new(None));
    let fd_table = Arc::new(Mutex::new(FDTable::new()));
    let process_root = PathBuf::from(format!("/tmp/wasm_sandbox/pid_{}", id));
    fs::create_dir_all(&process_root)?;

    let process_data = ProcessData {
        state: state.clone(),
        cond: cond.clone(),
        block_reason,
        fd_table,
        root_path: process_root,
    };

    let thread_data = process_data.clone();
    let thread = thread::Builder::new()
        .name(format!("pid{}", id))
        .spawn(move || {
            let mut store = Store::new(&engine, thread_data);
            // Set fuel (or other resource limits) as needed.
            let _ = store.set_fuel(2_000_000);
            let mut linker: Linker<ProcessData> = Linker::new(&engine);
            if let Err(e) = wasi_syscalls::register(&mut linker) {
                error!("Failed to register WASI syscalls: {:?}", e);
                return;
            }
            debug!("WASI syscalls registered");

            let instance = match linker.instantiate(&mut store, &module) {
                Ok(inst) => inst,
                Err(e) => {
                    error!("Failed to instantiate module: {:?}", e);
                    return;
                }
            };
            debug!("WASM module instantiated");

            // Wait until the scheduler sets the process state to Running.
            {
                let mut st = store.data().state.lock().unwrap();
                while *st != ProcessState::Running {
                    st = store.data().cond.wait(st).unwrap();
                }
            }

            // Call the _start function.
            let start_func = match instance.get_typed_func::<(), ()>(&mut store, "_start") {
                Ok(func) => func,
                Err(e) => {
                    error!("Missing _start function: {:?}", e);
                    return;
                }
            };
            if let Err(e) = start_func.call(&mut store, ()) {
                error!("Error executing wasm: {:?}", e);
            }
            // Mark process as Finished.
            {
                let mut s = store.data().state.lock().unwrap();
                *s = ProcessState::Finished;
            }
            store.data().cond.notify_all();
            debug!("Process {} marked as Finished", id);
        })?;

    info!("Started process with id {}", id);
    Ok(Process { id, thread, data: process_data })
}
