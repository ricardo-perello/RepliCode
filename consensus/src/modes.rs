use std::fs::{File, OpenOptions};
use std::io::{self, Write, Read, BufReader};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

use crate::record::write_record;
use crate::commands::{parse_command, Command};
use crate::nat::Nat;

pub fn run_benchmark_mode() -> io::Result<()> {
    let file_path = "consensus/consensus_input.bin";
    let mut output = OpenOptions::new()
        .create(true)
        .append(true)
        .open(file_path)?;

    loop {
        eprint!("Command (init <file> | msg <pid> <message> | clock <nanoseconds> | net <src> <dst> <payload>): ");
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
                Command::Init(_) => eprintln!("Initialization record written."),
                Command::FDMsg(pid, _) => eprintln!("Message record for process {} written.", pid),
                Command::Clock(delta) => eprintln!("Clock record ({} ns) written.", delta),
                Command::NetMsg(net_msg) => eprintln!("Network message from {} to {} written.", net_msg.src, net_msg.dst),
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
        let mut header = [0u8; 11]; // 1 (msg type) + 8 (pid) + 2 (length)
        if reader.read_exact(&mut header).is_err() {
            break; // End of file.
        }
        let msg_type = header[0];
        let pid = (&header[1..9]).read_u64::<LittleEndian>()?;
        let msg_size = (&header[9..11]).read_u16::<LittleEndian>()? as usize;

        let mut payload = vec![0u8; msg_size];
        reader.read_exact(&mut payload)?;

        let mut record = Vec::new();
        record.push(msg_type);
        record.write_u64::<LittleEndian>(pid)?;
        record.write_u16::<LittleEndian>(msg_size as u16)?;
        record.write_all(&payload)?;

        batch_buffer.extend(record);

        // Assume a clock record has type 0.
        if msg_type == 0 {
            let msg_str = String::from_utf8_lossy(&payload);
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

/// TCP mode: consensus acts as a mediator between the runtime and external clients.
/// Listens on:
/// - Port 9000 for a connection from the runtime.
/// - Port 9001 for external client connections.
pub fn run_tcp_mode() -> io::Result<()> {
    // Shared NAT mapping to keep track of client connections.
    let nat = Arc::new(Mutex::new(Nat::new()));

    // Listen for the runtime connection.
    let runtime_listener = TcpListener::bind("127.0.0.1:9000")?;
    eprintln!("TCP mode: Waiting for runtime connection on 127.0.0.1:9000...");
    let (mut runtime_stream, runtime_addr) = runtime_listener.accept()?;
    eprintln!("TCP mode: Connected to runtime from {}", runtime_addr);

    // Spawn a thread to listen for external client connections on port 9001.
    let nat_for_clients = Arc::clone(&nat);
    thread::spawn(move || -> io::Result<()> {
        let client_listener = TcpListener::bind("127.0.0.1:9001")?;
        eprintln!("TCP mode: Listening for client connections on 127.0.0.1:9001...");
        for client in client_listener.incoming() {
            match client {
                Ok(client_stream) => {
                    eprintln!("TCP mode: New client connected: {:?}", client_stream.peer_addr());
                    let client_stream = Arc::new(Mutex::new(client_stream));
                    // Here you could read an initial handshake from the client that indicates
                    // which process ID (pid) the client wishes to talk to.
                    // For simplicity, we assume the client sends a text line: "to:<pid>"
                    let nat_for_this = Arc::clone(&nat_for_clients);
                    thread::spawn(move || {
                        let mut stream = client_stream.lock().unwrap();
                        let mut buffer = [0u8; 1024];
                        loop {
                            match stream.read(&mut buffer) {
                                Ok(0) => {
                                    eprintln!("TCP mode: Client disconnected.");
                                    break;
                                },
                                Ok(n) => {
                                    let text = String::from_utf8_lossy(&buffer[..n]);
                                    if let Some(rest) = text.strip_prefix("to:") {
                                        let parts: Vec<&str> = rest.splitn(2, ' ').collect();
                                        if let Ok(target_pid) = parts[0].trim().parse::<u64>() {
                                            eprintln!("TCP mode: Client requests connection to pid {}", target_pid);
                                            // Register this client for the target process.
                                            nat_for_this.lock().unwrap().register(target_pid, Arc::clone(&client_stream));
                                        }
                                    }
                                    // In a full implementation, you would also encapsulate the client's message
                                    // into a NetMsg record and buffer it to send to the runtime.
                                },
                                Err(e) => {
                                    eprintln!("TCP mode: Error reading from client: {}", e);
                                    break;
                                }
                            }
                        }
                    });
                },
                Err(e) => {
                    eprintln!("TCP mode: Failed to accept client: {}", e);
                }
            }
        }
        Ok(())
    });

    // A buffer for messages to be sent to the runtime.
    let runtime_buffer = Arc::new(Mutex::new(Vec::new()));

    // Spawn a thread to read from the runtime connection and forward network messages to clients.
    let nat_for_runtime = Arc::clone(&nat);
    let runtime_stream_clone = runtime_stream.try_clone()?;
    thread::spawn(move || {
        let mut stream = runtime_stream_clone;
        loop {
            // Read record header (1 byte msg type, 8 bytes pid, 2 bytes length).
            let mut header = [0u8; 11];
            if let Err(e) = stream.read_exact(&mut header) {
                eprintln!("TCP mode: Error reading from runtime: {}", e);
                break;
            }
            let msg_type = header[0];
            let _pid = (&header[1..9]).read_u64::<LittleEndian>().unwrap_or(0);
            let payload_len = (&header[9..11]).read_u16::<LittleEndian>().unwrap_or(0) as usize;
            let mut payload = vec![0u8; payload_len];
            if let Err(e) = stream.read_exact(&mut payload) {
                eprintln!("TCP mode: Error reading payload from runtime: {}", e);
                break;
            }
            // If this is a network message (type 3), decapsulate and forward to client.
            if msg_type == 3 {
                if payload.len() >= 8 {
                    let dst = (&payload[0..8]).read_u64::<LittleEndian>().unwrap_or(0);
                    let net_payload = &payload[8..];
                    eprintln!("TCP mode: Received network message for pid {} ({} bytes)", dst, net_payload.len());
                    let nat_map = nat_for_runtime.lock().unwrap();
                    if let Some(client_stream) = nat_map.get_client(dst) {
                        let mut client = client_stream.lock().unwrap();
                        if let Err(e) = client.write_all(net_payload) {
                            eprintln!("TCP mode: Failed to write to client: {}", e);
                        } else {
                            eprintln!("TCP mode: Forwarded network message to client for pid {}", dst);
                        }
                    } else {
                        eprintln!("TCP mode: No client registered for pid {}", dst);
                    }
                } else {
                    eprintln!("TCP mode: Invalid network message payload length");
                }
            }
            // For other message types, simply log them.
        }
    });

    // Main loop: read consensus operator commands from stdin and send them to runtime.
    eprintln!("TCP mode: Enter commands (init, msg, clock, net):");
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
            let record = write_record(&cmd)?;
            {
                let mut buf = runtime_buffer.lock().unwrap();
                buf.extend(record);
            }
            // For simplicity, send the buffered message immediately.
            let mut buf = runtime_buffer.lock().unwrap();
            if !buf.is_empty() {
                runtime_stream.write_all(&buf)?;
                runtime_stream.flush()?;
                buf.clear();
            }
            match &cmd {
                Command::Init(_) => eprintln!("Initialization command sent."),
                Command::FDMsg(pid, _) => eprintln!("Message for process {} sent.", pid),
                Command::Clock(delta) => eprintln!("Clock record ({} ns) sent.", delta),
                Command::NetMsg(net_msg) => eprintln!("Network message from {} to {} sent.", net_msg.src, net_msg.dst),
            }
        }
    }

    eprintln!("TCP mode: Exiting.");
    Ok(())
}
