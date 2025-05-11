use std::io::{self, Write};
use std::fs::OpenOptions;
use log::{info, error};

use crate::record::write_record;
use crate::commands::{parse_command, Command};

pub fn run_benchmark_mode() -> io::Result<()> {
    let file_path = "consensus/consensus_input.bin";
    let mut output = OpenOptions::new()
        .create(true)
        .append(true)
        .open(file_path)?;

    loop {
        eprint!("Command (init <wasm_file> | msg <pid> <message> | ftp <pid> <ftp_command> | clock <nanoseconds>): ");
        io::stderr().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();
        if input.eq_ignore_ascii_case("exit") {
            break;
        }
        if let Some(cmd) = parse_command(input) {
            let record = write_record(&cmd)?;
            output.write_all(&record)?;
            output.flush()?;
            match &cmd {
                Command::Init { .. } => info!("Initialization record written."),
                Command::FDMsg(pid, _) => info!("Message record for process {} written.", pid),
                Command::Clock(delta) => info!("Clock record ({} ns) written.", delta),
                Command::NetworkIn(pid, port, _) => info!("Network input record for process {} port {} written.", pid, port),
                Command::NetworkOut(pid, _) => info!("Network output record for process {} written.", pid),
            }
        }
    }

    info!("Benchmark mode: Exiting.");
    Ok(())
} 