use std::fs::{File, OpenOptions};
use std::io::{self, Write, Read, BufReader};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use byteorder::{LittleEndian, WriteBytesExt, ReadBytesExt};

fn main() -> io::Result<()> {
    // Print instructions to stderr.
    eprintln!("Consensus Input Tool");
    eprintln!("----------------------");
    eprintln!("Record format: [ process_id: u64 ][ msg_size: u16 ][ msg: [u8; msg_size] ]");
    eprintln!("Benchmark mode: records are written immediately to a binary file.");
    eprintln!("Hybrid mode: reads an existing binary file and sends batches over TCP (after a clock record is reached).");
    eprintln!("TCP mode: user enters messages (clock messages are disallowed); every 10 seconds a batch is sent over TCP with an automatic clock record appended.");
    eprintln!("Type 'exit' at the process ID prompt to quit.\n");

    let args: Vec<String> = std::env::args().collect();
    // Supported modes: benchmark, hybrid, tcp.
    let mode = if args.len() > 1 { args[1].as_str() } else { "benchmark" };

    match mode {
        "benchmark" => run_benchmark_mode(),
        "hybrid" => {
            if args.len() < 3 {
                eprintln!("Hybrid mode requires an input file path as the second argument.");
                std::process::exit(1);
            }
            let input_file_path = &args[2];
            run_hybrid_mode(input_file_path)
        },
        "tcp" => run_tcp_mode(),
        _ => {
            eprintln!("Unknown mode: {}. Use benchmark, hybrid, or tcp.", mode);
            std::process::exit(1);
        }
    }
}

/// Benchmark mode: each record is immediately appended to a binary file.
fn run_benchmark_mode() -> io::Result<()> {
    let file_path = "consensus/consensus_input.bin";
    let mut output = OpenOptions::new()
        .create(true)
        .append(true)
        .open(file_path)?;

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

        output.write_u64::<LittleEndian>(pid)?;
        output.write_u16::<LittleEndian>(msg_size_u16)?;
        output.write_all(message_bytes)?;
        output.flush()?;

        eprintln!("Record written for process {}.\n", pid);
    }

    eprintln!("Exiting Benchmark Mode.");
    Ok(())
}

/// Hybrid mode: reads records from a provided binary file and sends them in batches over TCP.
/// A batch ends when a clock record (process_id == 0) is encountered. Upon a clock record,
/// the tool sleeps for 5 seconds then sends the accumulated batch over TCP.
fn run_hybrid_mode(input_file_path: &str) -> io::Result<()> {
    let file = File::open(input_file_path)?;
    let mut reader = BufReader::new(file);

    // Connect to runtime via TCP.
    let mut stream = TcpStream::connect("127.0.0.1:9000")?;
    eprintln!("Hybrid mode: Connected to runtime at 127.0.0.1:9000.");

    let mut batch_buffer = Vec::new();

    loop {
        let mut pid_buf = [0u8; 8];
        if reader.read_exact(&mut pid_buf).is_err() {
            break; // End of file.
        }
        let pid = (&pid_buf[..]).read_u64::<LittleEndian>()?;

        let mut size_buf = [0u8; 2];
        reader.read_exact(&mut size_buf)?;
        let msg_size = (&size_buf[..]).read_u16::<LittleEndian>()? as usize;

        let mut msg_buf = vec![0u8; msg_size];
        reader.read_exact(&mut msg_buf)?;

        let mut record = Vec::with_capacity(8 + 2 + msg_size);
        record.write_u64::<LittleEndian>(pid)?;
        record.write_u16::<LittleEndian>(msg_size as u16)?;
        record.write_all(&msg_buf)?;

        batch_buffer.extend(record);

        if pid == 0 {
            let msg_str = String::from_utf8_lossy(&msg_buf);
            eprintln!("Hybrid mode: Clock record encountered: {}", msg_str);
            thread::sleep(Duration::from_secs(5));
            if !batch_buffer.is_empty() {
                stream.write_all(&batch_buffer)?;
                stream.flush()?;
                batch_buffer.clear();
                eprintln!("Hybrid mode: Batch sent over TCP.\n");
            }
        }
    }

    if !batch_buffer.is_empty() {
        stream.write_all(&batch_buffer)?;
        stream.flush()?;
        eprintln!("Hybrid mode: Final batch sent over TCP.\n");
    }
    eprintln!("Exiting Hybrid Mode.");
    Ok(())
}


/// TCP mode: user enters messages (except clock messages) which are buffered and sent over TCP every 10 seconds.
/// Each batch automatically gets a clock record (process_id == 0, message "clock:10000000000") appended.
fn run_tcp_mode() -> io::Result<()> {
    // Act as TCP server.
    let listener = TcpListener::bind("127.0.0.1:9000")?;
    eprintln!("TCP mode: Waiting for connection on 127.0.0.1:9000...");
    let (mut stream, addr) = listener.accept()?;
    eprintln!("TCP mode: Accepted connection from {}", addr);

    // Clone the stream for use in the flush thread.
    let mut stream_flush = stream.try_clone()?;

    let buffer = Arc::new(Mutex::new(Vec::new()));
    let buffer_clone = Arc::clone(&buffer);
    let flush_interval = Duration::from_secs(10);

    // Spawn a thread that flushes the batch every 10 seconds.
    let flush_thread = thread::spawn(move || {
        loop {
            thread::sleep(flush_interval);
            let mut buf = buffer_clone.lock().unwrap();
            if !buf.is_empty() {
                // Automatically append a clock record.
                let mut clock_record = Vec::with_capacity(8 + 2 + 20);
                clock_record.write_u64::<LittleEndian>(0).unwrap();
                let clock_msg = b"clock:10000000000";
                clock_record.write_u16::<LittleEndian>(clock_msg.len() as u16).unwrap();
                clock_record.write_all(clock_msg).unwrap();
                buf.extend(clock_record);
                if let Err(e) = stream_flush.write_all(&buf) {
                    eprintln!("TCP mode: Error sending batch: {}", e);
                }
                if let Err(e) = stream_flush.flush() {
                    eprintln!("TCP mode: Error flushing stream: {}", e);
                }
                eprintln!("TCP mode: Batch sent over TCP.\n");
                buf.clear();
            }
        }
    });

    // Main loop: read user input and accumulate records.
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
                eprintln!("TCP mode: Invalid process ID.");
                continue;
            }
        };

        eprint!("Enter message (clock messages not allowed): ");
        io::stderr().flush()?;
        let mut message = String::new();
        io::stdin().read_line(&mut message)?;
        let message = message.trim();
        if message.is_empty() {
            eprintln!("TCP mode: Message cannot be empty.");
            continue;
        }
        if message.starts_with("clock:") {
            eprintln!("TCP mode: Clock messages are disallowed; they are auto-appended.");
            continue;
        }

        let message_bytes = message.as_bytes();
        let msg_size = message_bytes.len();
        if msg_size > u16::MAX as usize {
            eprintln!("TCP mode: Message too long (max {} bytes).", u16::MAX);
            continue;
        }
        let msg_size_u16 = msg_size as u16;

        let mut record = Vec::with_capacity(8 + 2 + msg_size);
        record.write_u64::<LittleEndian>(pid)?;
        record.write_u16::<LittleEndian>(msg_size_u16)?;
        record.write_all(message_bytes)?;

        {
            let mut buf = buffer.lock().unwrap();
            buf.extend(record);
        }
        eprintln!("TCP mode: Record buffered for process {}.", pid);
    }

    eprintln!("TCP mode: Exiting. Flushing remaining records.");
    {
        let mut buf = buffer.lock().unwrap();
        if !buf.is_empty() {
            let mut clock_record = Vec::with_capacity(8 + 2 + 20);
            clock_record.write_u64::<LittleEndian>(0)?;
            let clock_msg = b"clock:10000000000";
            clock_record.write_u16::<LittleEndian>(clock_msg.len() as u16)?;
            clock_record.write_all(clock_msg)?;
            buf.extend(clock_record);
            stream.write_all(&buf)?;
            stream.flush()?;
            eprintln!("TCP mode: Final batch sent over TCP.");
            buf.clear();
        }
    }
    flush_thread.join().unwrap_or_else(|_| ());
    Ok(())
}
