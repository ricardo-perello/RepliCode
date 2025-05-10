use std::io::{self, Write, Read, BufReader};
use std::fs::OpenOptions;
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use log::{error, info, debug};
use bincode;

// use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

use crate::record::write_record;
use crate::commands::{parse_command, Command, NetworkOperation};
use crate::nat::NatTable;
use crate::http_server::HttpServer;

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
    let shared_buffer: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));

    // Clone the shared buffer and stream for the flush thread.
    let flush_buffer: Arc<Mutex<Vec<u8>>> = Arc::clone(&shared_buffer);
    let shared_buffer_clone: Arc<Mutex<Vec<u8>>> = Arc::clone(&shared_buffer);
    let mut flush_stream = runtime_stream.try_clone()?;

    // Create NAT table for handling network operations
    let nat_table: Arc<Mutex<NatTable>> = Arc::new(Mutex::new(NatTable::new()));

    // Start HTTP server for status information
    let http_server = HttpServer::new(Arc::clone(&nat_table));
    thread::spawn(move || {
        if let Err(e) = http_server.start(8080) {
            error!("HTTP server error: {}", e);
        }
    });
    info!("HTTP status server started on port 8080");

    // Set the flush interval (e.g., every 10 seconds).
    let flush_interval = Duration::from_secs(10);
    thread::spawn(move || {
        loop {
            thread::sleep(flush_interval);
            let mut buf = flush_buffer.lock().unwrap();
            
            // Create a clock command (10 seconds = 10_000_000_000 nanoseconds)
            if let Ok(clock_record) = write_record(&Command::Clock(10_000_000_000)) {
                let original_size = buf.len();
                debug!("Appending clock record to batch");
                buf.extend(clock_record.clone());
                
                if let Err(e) = flush_stream.write_all(&buf) {
                    error!("Error writing to runtime: {}", e);
                } else {
                    if original_size > 0 {
                        info!("Flushed {} bytes to runtime and clock record.", buf.len());
                    } else {
                        debug!("Sent clock update to runtime");
                    }
                }
                buf.clear();
            } else {
                error!("Failed to create clock record");
            }
        }
    });

    // Add a thread to read from runtime and handle network operations
    let runtime_reader = runtime_stream.try_clone()?;
    let nat_table_clone: Arc<Mutex<NatTable>> = Arc::clone(&nat_table);
    thread::spawn(move || {
        let mut reader = BufReader::new(runtime_reader);
        loop {
            // Read message type (1 byte)
            let mut msg_type_buf = [0u8; 1];
            if reader.read_exact(&mut msg_type_buf).is_err() {
                error!("Lost connection to runtime");
                break;
            }
            
            // If it's a NetworkOut message (type 5)
            if msg_type_buf[0] == 5 {
                // Read process ID (8 bytes)
                let mut pid_buf = [0u8; 8];
                if reader.read_exact(&mut pid_buf).is_err() {
                    error!("Failed to read process ID from runtime");
                    break;
                }
                let pid = u64::from_le_bytes(pid_buf);
                
                // Read payload length (4 bytes)
                let mut len_buf = [0u8; 4];
                if reader.read_exact(&mut len_buf).is_err() {
                    error!("Failed to read payload length from runtime");
                    break;
                }
                let payload_len = u32::from_le_bytes(len_buf) as usize;
                
                // Read payload
                let mut payload = vec![0u8; payload_len];
                if reader.read_exact(&mut payload).is_err() {
                    error!("Failed to read payload from runtime");
                    break;
                }
                
                // Deserialize network operation
                debug!("Attempting to deserialize network operation...");
                match bincode::deserialize::<NetworkOperation>(&payload) {
                    Ok(op) => {
                        debug!("Successfully deserialized network operation: {:?}", op);
                        // Get source port, new port, and operation type before moving op
                        let (src_port, new_port, is_accept, is_recv) = match &op {
                            NetworkOperation::Connect { src_port, .. } => (*src_port, 0, false, false),
                            NetworkOperation::Send { src_port, .. } => (*src_port, 0, false, false),
                            NetworkOperation::Listen { src_port } => (*src_port, 0, false, false),
                            NetworkOperation::Accept { src_port, new_port, .. } => (*src_port, *new_port, true, false),
                            NetworkOperation::Close { src_port } => (*src_port, 0, false, false),
                            NetworkOperation::Recv { src_port } => (*src_port, 0, false, true),
                        };
                        debug!("Operation details - src_port: {}, new_port: {}, is_accept: {}, is_recv: {}", 
                               src_port, new_port, is_accept, is_recv);
                        
                        debug!("Attempting to acquire NAT table lock...");
                        // Store whether the connection exists for recv operations
                        let mut connection_exists = true;
                        if is_recv {
                            // Get a mutable reference to the NAT table
                            let mut nat_ref = nat_table_clone.lock().unwrap();
                            // Check connection status before the operation
                            connection_exists = nat_ref.has_connection(pid, src_port);
                            debug!("Connection check before operation for process {} port {}: {}", pid, src_port, connection_exists);
                            // Now perform the operation
                            let op_result = nat_ref.handle_network_operation(pid, op);
                            // Release the lock before sending the status back
                            drop(nat_ref);
                            match op_result {
                                Ok(success) => {
                                    debug!("Network operation completed with success={}", success);
                                    // Send status back to runtime
                                    debug!("Attempting to acquire shared buffer lock...");
                                    let mut buf = shared_buffer_clone.lock().unwrap();
                                    debug!("Acquired shared buffer lock");
                                    
                                    let status = if !success {
                                        // Check if we should wait or report connection closed
                                        if connection_exists {
                                            debug!("Process {} still waiting for recv on port {}", pid, src_port);
                                            2 // Still waiting
                                        } else {
                                            error!("Connection closed for process {} on port {}", pid, src_port);
                                            0 // Failure - connection closed
                                        }
                                    } else {
                                        info!("Network operation succeeded for process {} on port {}", pid, src_port);
                                        1 // Success
                                    };
                                    
                                    debug!("Preparing to send response back to runtime for process {}:{} with status {}", 
                                          pid, src_port, status);
                                    if let Ok(record) = write_record(&Command::NetworkIn(pid, 0, vec![
                                        status, 
                                        src_port as u8, (src_port >> 8) as u8,
                                        new_port as u8, (new_port >> 8) as u8
                                    ])) {
                                        debug!("Response record created, length: {}", record.len());
                                        buf.extend(record);
                                        debug!("Response record added to buffer, new buffer size: {}", buf.len());
                                    } else {
                                        error!("Failed to create response record for process {}:{}", pid, src_port);
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to handle network operation for process {} on port {}: {}", 
                                        pid, src_port, e);
                                    // Send error status back to runtime
                                    debug!("Attempting to acquire shared buffer lock for error response...");
                                    let mut buf = shared_buffer_clone.lock().unwrap();
                                    debug!("Acquired shared buffer lock for error response");
                                    if let Ok(record) = write_record(&Command::NetworkIn(pid, 0, vec![0])) {
                                        debug!("Error response record created, length: {}", record.len());
                                        buf.extend(record);
                                        debug!("Error response record added to buffer");
                                    }
                                }
                            }
                        } else {
                            // For non-recv operations, handle normally
                            match nat_table_clone.lock().unwrap().handle_network_operation(pid, op) {
                                Ok(success) => {
                                    debug!("Network operation completed with success={}", success);
                                    // Send status back to runtime
                                    debug!("Attempting to acquire shared buffer lock...");
                                    let mut buf = shared_buffer_clone.lock().unwrap();
                                    debug!("Acquired shared buffer lock");
                                    
                                    let status = if is_accept && !success {
                                        debug!("Process {} still waiting for accept on port {}", pid, src_port);
                                        2 // Still waiting
                                    } else if success {
                                        info!("Network operation succeeded for process {} on port {}", pid, src_port);
                                        1 // Success
                                    } else {
                                        error!("Network operation failed for process {} on port {}", pid, src_port);
                                        0 // Failure
                                    };
                                    
                                    debug!("Preparing to send response back to runtime for process {}:{} with status {}", 
                                          pid, src_port, status);
                                    if let Ok(record) = write_record(&Command::NetworkIn(pid, 0, vec![
                                        status, 
                                        src_port as u8, (src_port >> 8) as u8,
                                        new_port as u8, (new_port >> 8) as u8
                                    ])) {
                                        debug!("Response record created, length: {}", record.len());
                                        buf.extend(record);
                                        debug!("Response record added to buffer, new buffer size: {}", buf.len());
                                    } else {
                                        error!("Failed to create response record for process {}:{}", pid, src_port);
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to handle network operation for process {} on port {}: {}", 
                                        pid, src_port, e);
                                    // Send error status back to runtime
                                    debug!("Attempting to acquire shared buffer lock for error response...");
                                    let mut buf = shared_buffer_clone.lock().unwrap();
                                    debug!("Acquired shared buffer lock for error response");
                                    if let Ok(record) = write_record(&Command::NetworkIn(pid, 0, vec![0])) {
                                        debug!("Error response record created, length: {}", record.len());
                                        buf.extend(record);
                                        debug!("Error response record added to buffer");
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to deserialize network operation: {}", e);
                    }
                }
            }
        }
    });

    // Add a thread to check for incoming data on NAT connections
    let nat_table_clone: Arc<Mutex<NatTable>> = Arc::clone(&nat_table);
    let shared_buffer_clone: Arc<Mutex<Vec<u8>>> = Arc::clone(&shared_buffer);
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_millis(100)); // Check every 100ms
            let messages = nat_table_clone.lock().unwrap().check_for_incoming_data();
            if !messages.is_empty() {
                let mut buf = shared_buffer_clone.lock().unwrap();
                for (pid, port, data, is_connection) in messages {
                    if is_connection {
                        // Send a connection notification (status 1 for success)
                        debug!("Notifying runtime about new connection for process {} port {}", pid, port);
                        if let Ok(record) = write_record(&Command::NetworkIn(pid, 0, vec![
                            1,  // Success status
                            port as u8, (port >> 8) as u8,  // Listening port
                            (port + 1) as u8, ((port + 1) >> 8) as u8  // New port (listening port + 1)
                        ])) {
                            buf.extend(record);
                        }
                    } else if !data.is_empty() {
                        // Send actual data
                        debug!("Received {} bytes from network for process {} port {}", data.len(), pid, port);
                        if let Ok(record) = write_record(&Command::NetworkIn(pid, port, data)) {
                            buf.extend(record);
                        }
                        // Send success status for recv operation
                        if let Ok(record) = write_record(&Command::NetworkIn(pid, 0, vec![
                            1,  // Success status
                            port as u8, (port >> 8) as u8,  // Source port
                            0, 0  // No new port for recv
                        ])) {
                            buf.extend(record);
                        }
                    }
                }
            }
        }
    });

    // Main loop: read commands from stdin.
    loop {
        eprint!("Command (init <wasm_file> | msg <pid> <message>): ");
        io::stderr().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();
        if input.eq_ignore_ascii_case("exit") {
            break;
        }
        if let Some(cmd) = parse_command(input) {
            //debug!("Received command: {:?}", cmd);
            match write_record(&cmd) {
                Ok(record) => {
                    debug!("Encoded command into {} bytes", record.len());
                    // Add the record to the shared batch.
                    let mut buf = shared_buffer.lock().unwrap();
                    buf.extend(record);
                    debug!("Added record to batch, new batch size: {} bytes", buf.len());
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