use anyhow::Result;
use log::{debug, error, info};
use std::{
    fmt, fs::{self, create_dir_all}, panic::AssertUnwindSafe, path::{Path, PathBuf}, sync::{Arc, Condvar, Mutex}, thread
};
use wasmtime::{Engine, Module, Store, Linker};

use crate::{
    runtime::fd_table::FDTable,
    wasi_syscalls,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProcessState {
    Ready,
    Running,
    Blocked,
    Finished,
}

impl fmt::Display for ProcessState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Clone)]
pub enum BlockReason {
    StdinRead,
    Timeout { resume_after: u64 },
    FileIO,
    NetworkIO,
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

/// Holds all per-process runtime data that your WASM code can access.
#[derive(Clone)]
pub struct ProcessData {
    pub state: Arc<Mutex<ProcessState>>,
    pub cond: Arc<Condvar>,
    pub block_reason: Arc<Mutex<Option<BlockReason>>>,
    pub fd_table: Arc<Mutex<FDTable>>,
    pub root_path: PathBuf,
    pub max_disk_usage: u64,
    pub current_disk_usage: Arc<Mutex<u64>>,
}

pub struct Process {
    pub id: u64, // Unique process ID
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
/// Spawns a new process from a WASM module and assigns it a unique ID.
/// Now also optionally copies a preload directory (`preload_dir`) into the
/// new process sandbox before execution starts.
pub fn start_process(
    wasm_path: PathBuf,
    id: u64,
    preload_dir: Option<&Path>,
    max_disk_bytes: u64,
) -> Result<Process> {
    debug!("Starting process with path: {:?} and id: {}", wasm_path, id);
    let mut config = wasmtime::Config::new();
    config.consume_fuel(true);
    let engine = Engine::new(&config)?;
    let module = Module::from_file(&engine, &wasm_path)?;
    debug!("WASM module loaded from path: {:?}", wasm_path);

    // Initialize process state and FD table
    let state = Arc::new(Mutex::new(ProcessState::Ready));
    let cond = Arc::new(Condvar::new());
    let reason = Arc::new(Mutex::new(None));
    let fd_table = Arc::new(Mutex::new(FDTable::new()));
    {
        let mut table = fd_table.lock().unwrap();
        // Reserve FD=0 for stdin
        table.entries[0] = Some(FDEntry {
            buffer: Vec::new(),
            read_ptr: 0,
            is_directory: false,
            is_preopen: false,
            host_path: None,
        });
    }

    // Create the sandbox directory in "runtime/tmp/pid_<ID>"
    let sandbox_base = PathBuf::from("runtime").join("tmp");
    create_dir_all(&sandbox_base)?;
    let process_root_rel = sandbox_base.join(format!("pid_{}", id));
    create_dir_all(&process_root_rel)?;
    let process_root = fs::canonicalize(&process_root_rel)?;
    info!("Created sandbox for process {} at: {}", id, process_root.display());

    // Optionally preload a directory
    if let Some(src_dir) = preload_dir {
        copy_dir_recursive(src_dir, &process_root)?;
        info!("Preloaded {:?} into sandbox for process {}", src_dir, id);
    }

    // Preopen FD=3 => the root directory
    {
        let mut table = fd_table.lock().unwrap();
        table.entries[3] = Some(FDEntry {
            buffer: Vec::new(),
            read_ptr: 0,
            is_directory: true,
            is_preopen: true,
            host_path: Some(process_root.to_string_lossy().into_owned()),
        });
    }

    let process_data = ProcessData {
        state: state.clone(),
        cond: cond.clone(),
        block_reason: reason,
        fd_table,
        root_path: process_root.clone(),
        max_disk_usage: max_disk_bytes,
        current_disk_usage: Arc::new(Mutex::new(0)),
    };

    let process_data_clone = process_data.clone();
    let thread = thread::Builder::new()
        .name(format!("pid{}", id))
        .spawn(move || {
            // Catch any panic to ensure we remove the sandbox directory.
            let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                debug!(
                    "Thread {:?} starting execution for process id: {}",
                    thread::current().name().unwrap_or("unknown"),
                    id
                );
                let mut store = Store::new(&engine, process_data_clone.clone());
                let _ = store.set_fuel(2_000_000);

                let mut linker: Linker<ProcessData> = Linker::new(&engine);
                wasi_syscalls::register(&mut linker).expect("Failed to register WASI syscalls");
                debug!("WASI syscalls registered for process {}", id);

                // Instantiate the module
                let instance = linker
                    .instantiate(&mut store, &module)
                    .expect("Failed to instantiate module");

                debug!("Process {} instantiated; waiting for state=Running", id);
                {
                    let mut st = store.data().state.lock().unwrap();
                    while *st != ProcessState::Running {
                        st = store.data().cond.wait(st).unwrap();
                    }
                }

                // Call _start
                let start_func = instance
                    .get_typed_func::<(), ()>(&mut store, "_start")
                    .expect("Missing _start function");

                if let Err(e) = start_func.call(&mut store, ()) {
                    error!("Process {}: error executing _start: {:?}", id, e);
                }

                // Mark finished
                {
                    let mut s = store.data().state.lock().unwrap();
                    *s = ProcessState::Finished;
                }
                store.data().cond.notify_all();
            }));

            if let Err(panic_payload) = result {
                // On panic, also remove the directory
                error!("Process {} panicked! Cleaning up sandbox directory...", id);
                {
                    // Update process state to Finished so the scheduler knows it's done.
                    let mut st = process_data_clone.state.lock().unwrap();
                    *st = ProcessState::Finished;
                }
                process_data_clone.cond.notify_all();
                std::panic::resume_unwind(panic_payload);
            }
        })?;

    info!("Started process with id {}", id);
    Ok(Process { id, thread, data: process_data })
}


/// Recursively copy all files & subdirectories from `src` into `dst`.
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            fs::create_dir_all(&dst_path)?;
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
