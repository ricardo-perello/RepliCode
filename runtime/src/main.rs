use anyhow::Result;
// Declare modules so Rust knows where to find them.
mod consensus_input;
mod runtime;
mod wasi_syscalls;

fn main() -> Result<()> {
    // Determine execution mode: benchmark, hybrid, or interactive.
    let args: Vec<String> = std::env::args().collect();
    let mode = if args.len() > 1 { &args[1] } else { "benchmark" };
    println!("Running in {} mode", mode);

    // Spawn processes from WASM modules.
    let mut processes = Vec::new();
    let mut wasm_files = Vec::new();
    let wasm_folder = "wasm_programs/build";
    let mut next_id = 1;
    for entry in std::fs::read_dir(wasm_folder)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("wasm") {
            println!("Found WASM: {:?}", path);
            wasm_files.push(path);
        }
    }
    for path in wasm_files{
        let process = runtime::process::start_process(path, next_id)?;
        next_id += 1;
        processes.push(process);
    }

    match mode {
        "benchmark" => {
            let consensus_file = "consensus/consensus_input.bin";
            runtime::scheduler::run_scheduler_with_file(processes, consensus_file)?;
        },
        "interactive" => {
            println!("Interactive mode: reading from standard input.");
            // Instead of opening a named pipe, read from standard input.
            let stdin = std::io::stdin();
            let mut consensus_pipe = stdin.lock();
            runtime::scheduler::run_scheduler_interactive(processes, &mut consensus_pipe)?;
        },
        _ => {
            eprintln!("Unknown mode: {}. Use benchmark, hybrid, or interactive.", mode);
        }
    }

    Ok(())
}