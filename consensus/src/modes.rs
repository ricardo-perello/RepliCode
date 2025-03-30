use std::io::{self, Write}; //, Read, BufReader};
use std::fs::OpenOptions;
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use log::{error, info};

// use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

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
                Command::Init(_) => info!("Initialization record written."),
                Command::FDMsg(pid, _) => info!("Message record for process {} written.", pid),
                Command::Clock(delta) => info!("Clock record ({} ns) written.", delta),
                Command::Ftp(pid, cmd) => info!("FTP command for process {} written: {}", pid, cmd),
            }
        }
    }

    info!("Benchmark mode: Exiting.");
    Ok(())
}

// pub fn run_hybrid_mode(input_file_path: &str) -> io::Result<()> {
//     let file = File::open(input_file_path)?;
//     let mut reader = BufReader::new(file);

//     // Connect to runtime via TCP.
//     let mut stream = TcpStream::connect("127.0.0.1:9000")?;
//     eprintln!("Hybrid mode: Connected to runtime at 127.0.0.1:9000.");

//     let mut batch_buffer = Vec::new();

//     loop {
//         let mut header = [0u8; 11]; // 1 (msg type) + 8 (pid) + 2 (length)
//         if reader.read_exact(&mut header).is_err() {
//             break; // End of file.
//         }
//         let msg_type = header[0];
//         let pid = (&header[1..9]).read_u64::<LittleEndian>()?;
//         let msg_size = (&header[9..11]).read_u16::<LittleEndian>()? as usize;

//         let mut payload = vec![0u8; msg_size];
//         reader.read_exact(&mut payload)?;

//         let mut record = Vec::new();
//         record.push(msg_type);
//         record.write_u64::<LittleEndian>(pid)?;
//         record.write_u16::<LittleEndian>(msg_size as u16)?;
//         record.write_all(&payload)?;

//         batch_buffer.extend(record);

//         // Assume a clock record has type 0.
//         if msg_type == 0 {
//             let msg_str = String::from_utf8_lossy(&payload);
//             eprintln!("Hybrid mode: Clock record encountered: {}", msg_str);
//             thread::sleep(Duration::from_secs(5));
//             if !batch_buffer.is_empty() {
//                 stream.write_all(&batch_buffer)?;
//                 stream.flush()?;
//                 batch_buffer.clear();
//                 eprintln!("Hybrid mode: Batch sent over TCP.\n");
//             }
//         }
//     }

//     if !batch_buffer.is_empty() {
//         stream.write_all(&batch_buffer)?;
//         stream.flush()?;
//         eprintln!("Hybrid mode: Final batch sent over TCP.\n");
//     }
//     eprintln!("Exiting Hybrid Mode.");
//     Ok(())
// }


pub fn run_tcp_mode() -> io::Result<()> {
    // Consensus acts as the server: listen on port 9000.
    let listener = TcpListener::bind("127.0.0.1:9000")?;
    info!("TCP mode: Listening for runtime on 127.0.0.1:9000...");
    
    // Accept a connection from the runtime.
    let (runtime_stream, addr) = listener.accept()?;
    info!("TCP mode: Accepted connection from runtime at {}", addr);

    // Shared buffer for accumulating messages.
    let shared_buffer = Arc::new(Mutex::new(Vec::new()));

    // Clone the shared buffer and stream for the flush thread.
    let flush_buffer = Arc::clone(&shared_buffer);
    let mut flush_stream = runtime_stream.try_clone()?;

    // Set the flush interval (e.g., every 10 seconds).
    let flush_interval = Duration::from_secs(10);
    thread::spawn(move || {
        loop {
            thread::sleep(flush_interval);
            let mut buf = flush_buffer.lock().unwrap();
            if !buf.is_empty() {
                // Create and append a clock command (10 seconds = 10_000_000_000 nanoseconds)
                if let Ok(clock_record) = write_record(&Command::Clock(10_000_000_000)) {
                    buf.extend(clock_record);
                }
                
                if let Err(e) = flush_stream.write_all(&buf) {
                    error!("Error writing to runtime: {}", e);
                } else {
                    info!("Flushed {} bytes to runtime and clock record.", buf.len());
                }
                buf.clear();
            }
        }
    });

    // Main loop: read commands from stdin.
    loop {
        eprint!("Command (init <wasm_file> | msg <pid> <message> | ftp <pid> <ftp_command>): ");
        io::stderr().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();
        if input.eq_ignore_ascii_case("exit") {
            break;
        }
        if let Some(cmd) = parse_command(input) {
            match write_record(&cmd) {
                Ok(record) => {
                    // Add the record to the shared batch.
                    let mut buf = shared_buffer.lock().unwrap();
                    buf.extend(record);
                }
                Err(e) => {
                    error!("Error encoding command: {}", e);
                }
            }
        }
    }

    info!("TCP mode: Exiting.");
    Ok(())
}