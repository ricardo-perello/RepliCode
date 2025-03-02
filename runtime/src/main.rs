use anyhow::Result;
// Declare modules so Rust knows where to find them.
mod consensus_input;
mod runtime;
mod wasi_syscalls;

use std::net::TcpStream;

fn main() -> Result<()> {
    // Determine execution mode: "benchmark" or "tcp"
    let args: Vec<String> = std::env::args().collect();
    let mode = if args.len() > 1 { &args[1] } else { "benchmark" };
    println!("Running in {} mode", mode);

    // Spawn processes from WASM modules.
    let mut processes = Vec::new();
    let wasm_folder = "wasm_programs/build";
    let mut next_id = 1;
    for entry in std::fs::read_dir(wasm_folder)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("wasm") {
            println!("Found WASM: {:?}", path);
            let process = runtime::process::start_process(path, next_id)?;
            next_id += 1;
            processes.push(process);
        }
    }

    match mode {
        "benchmark" => {
            let consensus_file = "consensus/consensus_input.bin";
            runtime::scheduler::run_scheduler_with_file(processes, consensus_file)?;
        },
        "tcp" => {
            println!("TCP mode: connecting to consensus server at 127.0.0.1:9000");
            let mut stream = TcpStream::connect("127.0.0.1:9000")?;
            runtime::scheduler::run_scheduler_interactive(processes, &mut stream)?;
        },
        _ => {
            eprintln!("Unknown mode: {}. Use benchmark or tcp.", mode);
        }
    }

    Ok(())
}
