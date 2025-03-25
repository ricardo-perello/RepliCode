use anyhow::Result;
use log::{info, error, debug};
use env_logger;
mod consensus_input;
mod runtime;
mod wasi_syscalls;
use std::net::TcpStream;

fn main() -> Result<()> {
    // Initialize the logger (env_logger reads RUST_LOG env variable)
    env_logger::init();

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
    Ok(())
}
