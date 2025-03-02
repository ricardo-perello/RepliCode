use std::fs::{File, OpenOptions};
use std::io::{self, Write, Read, BufReader};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use byteorder::{LittleEndian, WriteBytesExt, ReadBytesExt};

fn main() -> io::Result<()> {
    // Print instructions to stderr.
    eprintln!("Consensus Input Tool");
    eprintln!("----------------------");
    eprintln!("This tool creates binary records with the following layout:");
    eprintln!("  [ process_id: u64 ][ msg_size: u16 ][ msg: [u8; msg_size] ]");
    eprintln!("For FD updates, use a valid process id (>=1) and a message like:");
    eprintln!("  \"fd:0,body:Hello World!\"");
    eprintln!("For global clock updates, use process id 0 and a message like:");
    eprintln!("  \"clock:100000\"  (to increment the clock by 100000 units)");
    eprintln!("Type 'exit' at the process ID prompt to quit.\n");

    // Determine mode: benchmark, interactive, or hybrid.
    let args: Vec<String> = std::env::args().collect();
    let mode = if args.len() > 1 { args[1].as_str() } else { "benchmark" };

    match mode {
        "benchmark" => run_benchmark_mode(),
        "interactive" => run_interactive_mode(),
        "hybrid" => {
            if args.len() < 3 {
                eprintln!("Hybrid mode requires an input file path as the second argument.");
                std::process::exit(1);
            }
            let input_file_path = &args[2];
            run_hybrid_mode(input_file_path)
        },
        _ => {
            eprintln!("Unknown mode: {}. Use benchmark, interactive, or hybrid.", mode);
            std::process::exit(1);
        }
    }
}

/// Benchmark mode: each record is written immediately as the user enters it.
fn run_benchmark_mode() -> io::Result<()> {
    // Open the consensus file in append mode.
    let file_path = "consensus/consensus_input.bin";
    let mut output = OpenOptions::new()
        .create(true)
        .append(true)
        .open(file_path)?;

    loop {
        eprint!("Enter Process ID: ");
        io::stderr().flush()?; // Ensure prompt appears on stderr.
        let mut pid_input = String::new();
        io::stdin().read_line(&mut pid_input)?;
        let pid_input = pid_input.trim();
        if pid_input.eq_ignore_ascii_case("exit") {
            break;
        }
        let pid: u64 = match pid_input.parse() {
            Ok(num) => num,
            Err(_) => {
                eprintln!("Invalid process ID. Please enter a valid number.");
                continue;
            }
        };

        eprint!("Enter message: ");
        io::stderr().flush()?;
        let mut message = String::new();
        io::stdin().read_line(&mut message)?;
        let message = message.trim();
        if message.is_empty() {
            eprintln!("Message cannot be empty.");
            continue;
        }

        let message_bytes = message.as_bytes();
        let msg_size = message_bytes.len();
        if msg_size > u16::MAX as usize {
            eprintln!("Message too long (max {} bytes).", u16::MAX);
            continue;
        }
        let msg_size_u16 = msg_size as u16;

        // Write the binary record immediately.
        // [ process_id: u64 ][ msg_size: u16 ][ msg: [u8; msg_size] ]
        output.write_u64::<LittleEndian>(pid)?;
        output.write_u16::<LittleEndian>(msg_size_u16)?;
        output.write_all(message_bytes)?;
        output.flush()?; // Ensure immediate write

        eprintln!("Record written for process {}.\n", pid);
    }

    eprintln!("Exiting Benchmark Mode.");
    Ok(())
}

/// Interactive mode: records are buffered and flushed every 5 seconds.
fn run_interactive_mode() -> io::Result<()> {
    // Shared buffer for pending records.
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let buffer_clone = Arc::clone(&buffer);
    let flush_interval = Duration::from_secs(5);
    let file_path = "consensus/consensus_input.bin";

    // Spawn a thread to flush the buffer periodically.
    let flush_thread = thread::spawn(move || {
        // Open the consensus file once in append mode.
        let mut out = OpenOptions::new()
            .create(true)
            .append(true)
            .open(file_path)
            .expect("Failed to open consensus file in flush thread");
        loop {
            thread::sleep(flush_interval);
            let mut buf = buffer_clone.lock().unwrap();
            if !buf.is_empty() {
                if let Err(e) = out.write_all(&buf) {
                    eprintln!("Error writing batch to file: {}", e);
                }
                if let Err(e) = out.flush() {
                    eprintln!("Error flushing file: {}", e);
                }
                buf.clear();
            }
        }
    });

    // Main thread reads user input and appends records to the shared buffer.
    loop {
        eprint!("Enter Process ID: ");
        io::stderr().flush()?;
        let mut pid_input = String::new();
        io::stdin().read_line(&mut pid_input)?;
        let pid_input = pid_input.trim();
        if pid_input.eq_ignore_ascii_case("exit") {
            break;
        }
        let pid: u64 = match pid_input.parse() {
            Ok(num) => num,
            Err(_) => {
                eprintln!("Invalid process ID. Please enter a valid number.");
                continue;
            }
        };

        eprint!("Enter message: ");
        io::stderr().flush()?;
        let mut message = String::new();
        io::stdin().read_line(&mut message)?;
        let message = message.trim();
        if message.is_empty() {
            eprintln!("Message cannot be empty.");
            continue;
        }

        let message_bytes = message.as_bytes();
        let msg_size = message_bytes.len();
        if msg_size > u16::MAX as usize {
            eprintln!("Message too long (max {} bytes).", u16::MAX);
            continue;
        }
        let msg_size_u16 = msg_size as u16;

        // Build the binary record.
        let mut record = Vec::with_capacity(8 + 2 + msg_size);
        record.write_u64::<LittleEndian>(pid)?;
        record.write_u16::<LittleEndian>(msg_size_u16)?;
        record.write_all(message_bytes)?;

        // Append the record to the shared buffer.
        {
            let mut buf = buffer.lock().unwrap();
            buf.extend(record);
        }
        eprintln!("Record buffered for process {}.\n", pid);
    }

    eprintln!("Exiting Interactive Mode.");
    // Flush any remaining data.
    {
        let mut out = OpenOptions::new()
            .create(true)
            .append(true)
            .open("consensus/consensus_input.bin")?;
        let mut buf = buffer.lock().unwrap();
        if !buf.is_empty() {
            out.write_all(&buf)?;
            out.flush()?;
            buf.clear();
        }
    }
    // In a complete application we would signal the flush thread to stop.
    // Here we simply exit.
    flush_thread.join().unwrap_or_else(|_| ());
    Ok(())
}

/// Hybrid mode: reads a file of batches and appends each batch to the same consensus file.
/// A batch ends when a clock record is encountered (process_id == 0 and message starts with "clock:").
fn run_hybrid_mode(input_file_path: &str) -> io::Result<()> {
    // Open the provided input file for reading.
    let file = File::open(input_file_path)?;
    let mut reader = BufReader::new(file);

    // Open the consensus file for appending output.
    let out_file_path = "consensus/consensus_input.bin";
    let mut out = OpenOptions::new()
        .create(true)
        .append(true)
        .open(out_file_path)?;

    let mut batch_buffer = Vec::new();

    loop {
        // Read a record header: process_id (8 bytes)
        let mut pid_buf = [0u8; 8];
        if reader.read_exact(&mut pid_buf).is_err() {
            break; // End of file.
        }
        let pid = (&pid_buf[..]).read_u64::<LittleEndian>()?;

        // Read message size (2 bytes)
        let mut size_buf = [0u8; 2];
        reader.read_exact(&mut size_buf)?;
        let msg_size = (&size_buf[..]).read_u16::<LittleEndian>()? as usize;

        // Read the message.
        let mut msg_buf = vec![0u8; msg_size];
        reader.read_exact(&mut msg_buf)?;

        // Build the binary record.
        let mut record = Vec::with_capacity(8 + 2 + msg_size);
        record.write_u64::<LittleEndian>(pid)?;
        record.write_u16::<LittleEndian>(msg_size as u16)?;
        record.write_all(&msg_buf)?;

        // Append the record to the current batch.
        batch_buffer.extend(record);

        // Check if this is a clock record marking the end of a batch.
        if pid == 0 {
            let msg_str = String::from_utf8_lossy(&msg_buf);
            eprintln!("Clock record encountered: {}", msg_str);

            // If you want to parse a clock delta and sleep, do it here.
            // For demonstration, we just sleep for 5 seconds:
            thread::sleep(Duration::from_secs(5));

            // After "sleeping," send the current batch to the consensus file.
            if !batch_buffer.is_empty() {
                out.write_all(&batch_buffer)?;
                out.flush()?;
                batch_buffer.clear();
                eprintln!("Batch sent.\n");
            }
        }
    }

    // Flush any remaining records as a final batch.
    if !batch_buffer.is_empty() {
        out.write_all(&batch_buffer)?;
        out.flush()?;
        eprintln!("Final batch sent.\n");
    }
    eprintln!("Exiting Hybrid Mode.");
    Ok(())
}