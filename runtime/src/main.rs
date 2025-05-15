use anyhow::Result;
use log::{info, error, debug};
use env_logger;
mod consensus_input;
mod runtime;
mod wasi_syscalls;
use std::net::TcpStream;
use std::path::PathBuf;
use std::fs;
use std::sync::OnceLock;
use ctrlc;

static SANDBOX_ROOT: OnceLock<PathBuf> = OnceLock::new();

fn pick_unique_sandbox_root() -> PathBuf {
    let mut idx = 0;
    loop {
        let candidate = PathBuf::from(format!("wasi_sandbox_{}", idx));
        if !candidate.exists() {
            return candidate;
        }
        idx += 1;
    }
}

fn main() -> Result<()> {
    // Initialize the logger (env_logger reads RUST_LOG env variable)
    env_logger::init();

    // Pick a unique sandbox root and store it globally
    let sandbox_root = pick_unique_sandbox_root();
    fs::create_dir_all(&sandbox_root)?;
    SANDBOX_ROOT.set(sandbox_root.clone()).unwrap();
    info!("Using sandbox root: {}", sandbox_root.display());

    // Ensure cleanup on exit
    let sandbox_root_cleanup = sandbox_root.clone();
    ctrlc::set_handler(move || {
        info!("Cleaning up sandbox root: {}", sandbox_root_cleanup.display());
        let _ = fs::remove_dir_all(&sandbox_root_cleanup);
        std::process::exit(0);
    }).expect("Error setting Ctrl-C handler");

    // Determine execution mode: "benchmark" or "tcp"
    let args: Vec<String> = std::env::args().collect();
    let mode = if args.len() > 1 { &args[1] } else { "benchmark" };
    info!("Runtime: Running in {} mode", mode);
    debug!("Arguments: {:?}", args);

    // Spawn processes from WASM modules.
    let processes = Vec::new();
    //let testdir_path = Path::new("runtime/testdir"); // relative path "testdir"
    //let preload_dir = Some(testdir_path);
    match mode {
        "benchmark" => {
            let consensus_file = "consensus/consensus_input.bin";
            info!("Runtime: Running in benchmark mode with file: {}", consensus_file);
            runtime::scheduler::run_scheduler_with_file(processes, consensus_file)?;
        },
        "tcp" => {
            info!("Runtime: TCP mode: Connecting to consensus server at 127.0.0.1:9000");
            let mut stream = TcpStream::connect("127.0.0.1:9000")?;
            debug!("Connected to TCP server");
            runtime::scheduler::run_scheduler_interactive(processes, &mut stream)?;
        },
        _ => {
            error!("Runtime: Unknown mode: {}. Use benchmark or tcp.", mode);
        }
    }

    info!("Runtime: Exiting.");
    // Clean up sandbox root on normal exit
    info!("Cleaning up sandbox root: {}", SANDBOX_ROOT.get().unwrap().display());
    let _ = fs::remove_dir_all(SANDBOX_ROOT.get().unwrap());
    Ok(())
}
