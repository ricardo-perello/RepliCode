use std::fs::{File, OpenOptions};
use std::io::{self, Write, Read, BufReader};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

use crate::record::{write_record_bytes, write_record};
use crate::commands::{parse_command, INIT_REQUEST, Command};

pub fn run_benchmark_mode() -> io::Result<()> {
    let file_path = "consensus/consensus_input.bin";
    let mut output = OpenOptions::new()
        .create(true)
        .append(true)
        .open(file_path)?;

    loop {
        eprint!("Command (init <file> | msg <pid> <message> | clock <nanoseconds>): ");
        io::stderr().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();
        if input.eq_ignore_ascii_case("exit") {
            break;
        }
        if let Some(cmd) = parse_command(input) {
            let record = match cmd {
                Command::Init(ref wasm_bytes) => {
                    if wasm_bytes.is_empty() {
                        eprintln!("Initialization failed: no WASM data.");
                        continue;
                    }
                    write_record_bytes(INIT_REQUEST, wasm_bytes)?
                },
                Command::Msg(pid, ref msg) => {
                    if msg.is_empty() {
                        eprintln!("Message cannot be empty.");
                        continue;
                    }
                    write_record(pid, msg)?
                },
            };
            output.write_all(&record)?;
            output.flush()?;
            match cmd {
                Command::Init(_) => eprintln!("Initialization record written."),
                Command::Msg(pid, _) => eprintln!("Message record for process {} written.", pid),
            }
        }
    }

    eprintln!("Exiting Benchmark Mode.");
    Ok(())
}

pub fn run_hybrid_mode(input_file_path: &str) -> io::Result<()> {
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

pub fn run_tcp_mode() -> io::Result<()> {
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
                // Append a clock record.
                let clock_record = write_record(0, "clock:10000000000")
                    .expect("Failed to create clock record");
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

    eprintln!("TCP mode: Enter commands. Use 'init <wasm_file_path>' or 'msg <pid> <message>'.");
    loop {
        eprint!("Command: ");
        io::stderr().flush()?;
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        let line = line.trim();
        if line.eq_ignore_ascii_case("exit") {
            break;
        }
        if let Some(cmd) = parse_command(line) {
            let record = match cmd {
                crate::commands::Command::Init(ref wasm_bytes) => {
                    if wasm_bytes.is_empty() {
                        eprintln!("Initialization failed: no WASM data.");
                        continue;
                    }
                    write_record_bytes(INIT_REQUEST, wasm_bytes)?
                },
                crate::commands::Command::Msg(pid, ref msg) => {
                    if msg.is_empty() {
                        eprintln!("Message cannot be empty.");
                        continue;
                    }
                    write_record(pid, msg)?
                },
            };
            {
                let mut buf = buffer.lock().unwrap();
                buf.extend(record);
            }
            match cmd {
                crate::commands::Command::Init(_) => eprintln!("Initialization command buffered."),
                crate::commands::Command::Msg(pid, _) => eprintln!("Message for process {} buffered.", pid),
            }
        }
    }

    eprintln!("TCP mode: Exiting. Flushing remaining records.");
    {
        let mut buf = buffer.lock().unwrap();
        if !buf.is_empty() {
            let clock_record = write_record(0, "clock:10000000000")?;
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
