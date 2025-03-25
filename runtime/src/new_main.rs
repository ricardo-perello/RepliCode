// use anyhow::Result;
// use env_logger;
// use log::{info, debug};
// use std::net::TcpStream;

// use crate::runtime::process::Process;

// mod consensus_input;
// mod process;
// mod scheduler;
// mod wasi_syscalls;

// fn main() -> Result<()> {
//     env_logger::init();

//     // Start with an empty vector of processes.
//     let processes = Vec::new();

//     info!("TCP mode: Connecting to consensus server at 127.0.0.1:9000");
//     let mut stream = TcpStream::connect("127.0.0.1:9000")?;
//     debug!("Connected to TCP server");

//     // Run the dynamic scheduler (which will idle until new processes arrive).
//     scheduler::run_scheduler_dynamic(processes, &mut stream)?;

//     info!("Runtime: Exiting.");
//     Ok(())
// }


