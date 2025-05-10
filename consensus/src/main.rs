mod commands;
mod record;
mod modes;
mod nat;
mod test_server;
mod test_client;
mod netcat_client;

use std::env;
use std::io;
use log::{info, error};
use std::process;

fn main() -> io::Result<()> {
    env_logger::init();
    info!("Starting consensus node");

    eprintln!("Consensus Input Tool");
    eprintln!("----------------------");
    eprintln!("Record format: [ msg_type: u8 ][ process_id: u64 ][ msg_size: u16 ][ payload: [u8; msg_size] ]");
    eprintln!("Benchmark mode: records are written immediately to a binary file.");
    eprintln!("Hybrid mode: reads an existing binary file and sends batches over TCP (after a clock record is reached).");
    eprintln!("TCP mode: enter commands interactively; every 10 seconds a batch is sent over TCP with an automatic clock record appended.");
    eprintln!("Test server: starts a local echo server on 127.0.0.1:8000 for testing network connections.");
    eprintln!("Test client: starts a test client for testing network connections.");
    eprintln!("Type 'exit' to quit.\n");
    
    
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        error!("Usage: {} <mode>", args[0]);
        process::exit(1);
    }

    let mode = &args[1];
    match mode.as_str() {
        "benchmark" => modes::run_benchmark_mode(),
        // "hybrid" => {
        //     if args.len() < 3 {
        //         eprintln!("Hybrid mode requires an input file path as the second argument.");
        //         std::process::exit(1);
        //     }
        //     let input_file_path = &args[2];
        //     modes::run_hybrid_mode(input_file_path)
        // },
        "tcp" => modes::run_tcp_mode(),
        "test-server" => test_server::start_test_server(),
        "test-client" => {
            test_client::run_test_client();
            Ok(())
        },
        "netcat-client" => {
            netcat_client::start_netcat_client()?;
            Ok(())
        },
        _ => {
            error!("Unknown mode: {}", mode);
            process::exit(1);
        }
    }
}
