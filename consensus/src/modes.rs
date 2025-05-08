use std::io::{self, Write, Read};
use std::fs::{OpenOptions, File, create_dir_all};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::path::{Path, PathBuf};
use log::{error, info, debug, warn};
use bincode;
use chrono::Utc;
use std::sync::mpsc;
use byteorder::{WriteBytesExt, ReadBytesExt, LittleEndian};

// use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

use crate::record::write_record;
use crate::commands::{parse_command, Command, NetworkOperation};
use crate::nat::NatTable;

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

/// Structure to hold a runtime connection
struct RuntimeConnection {
    stream: TcpStream,
    address: String,
}

/// Global session file path
fn get_session_file_path() -> PathBuf {
    let now = Utc::now();
    let batches_dir = Path::new("batches");
    if !batches_dir.exists() {
        if let Err(e) = create_dir_all(batches_dir) {
            error!("Failed to create batches directory: {}", e);
        }
    }
    
    batches_dir.join(format!("session-{}.bin", now.format("%Y%m%dT%H%M%S")))
}

/// Opens or creates the session file for appending
fn open_session_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
}

/// Appends a batch to the session file
/// Format: [batch_size: u32][batch_data: [u8; batch_size]]
fn write_batch_to_file(batch: &[u8], session_file: &mut File) -> io::Result<()> {
    // Write batch size as a 4-byte header
    session_file.write_u32::<LittleEndian>(batch.len() as u32)?;
    
    // Write the batch data
    session_file.write_all(batch)?;
    
    // Flush to ensure data is written to disk
    session_file.flush()?;
    
    debug!("Batch of {} bytes appended to session file", batch.len());
    Ok(())
}

/// Reads all batches from the session file
fn read_all_batches(session_file_path: &Path) -> io::Result<Vec<Vec<u8>>> {
    // Check if file exists
    if !session_file_path.exists() {
        return Ok(Vec::new());
    }
    
    let mut file = File::open(session_file_path)?;
    let mut batches = Vec::new();
    
    // Read until end of file
    while let Ok(batch_size) = file.read_u32::<LittleEndian>() {
        // Read the batch data
        let mut batch = vec![0u8; batch_size as usize];
        if let Err(e) = file.read_exact(&mut batch) {
            if e.kind() == io::ErrorKind::UnexpectedEof {
                // End of file reached while reading batch - file may be corrupted
                warn!("Unexpected end of file while reading batch. File may be corrupted.");
                break;
            }
            return Err(e);
        }
        
        batches.push(batch);
    }
    
    debug!("Read {} batches from session file", batches.len());
    Ok(batches)
}

pub fn run_tcp_mode() -> io::Result<()> {
    // Consensus acts as the server: listen on port 9000.
    let listener = TcpListener::bind("127.0.0.1:9000")?;
    info!("TCP mode: Listening for runtimes on 127.0.0.1:9000...");
    
    // Set to non-blocking mode so we can accept connections without blocking
    listener.set_nonblocking(true)?;
    
    // Create the batches directory and session file
    let session_file_path = get_session_file_path();
    let session_file = open_session_file(&session_file_path)?;
    info!("Created session file: {}", session_file_path.display());
    
    // Shared reference to the session file path
    let session_file_path = Arc::new(session_file_path);
    
    // Shared state for runtime connections
    let runtimes: Arc<Mutex<Vec<RuntimeConnection>>> = Arc::new(Mutex::new(Vec::new()));
    
    // Shared buffer for accumulating messages
    let shared_buffer: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));

    // Create NAT table for handling network operations
    let nat_table: Arc<Mutex<NatTable>> = Arc::new(Mutex::new(NatTable::new()));
    
    // Thread-safe channel for handling runtime responses
    let (response_tx, response_rx) = mpsc::channel();
    
    // Clone for use in the listener thread
    let runtimes_listener = Arc::clone(&runtimes);
    let shared_buffer_listener = Arc::clone(&shared_buffer);
    let session_file_path_listener = Arc::clone(&session_file_path);
    
    // Spawn a thread to accept new connections
    thread::spawn(move || {
        loop {
            match listener.accept() {
                Ok((stream, addr)) => {
                    info!("TCP mode: Accepted connection from runtime at {}", addr);
                    
                    // Configure the stream
                    if let Err(e) = stream.set_nodelay(true) {
                        error!("Failed to set TCP_NODELAY: {}", e);
                    }
                    
                    // Create a new runtime connection
                    let mut runtime = RuntimeConnection {
                        stream: stream.try_clone().unwrap(),
                        address: addr.to_string(),
                    };
                    
                    // Send all existing batches to the new runtime
                    match read_all_batches(&session_file_path_listener) {
                        Ok(batches) => {
                            info!("Sending {} existing batches to new runtime at {}", batches.len(), addr);
                            
                            for batch in batches {
                                if let Err(e) = runtime.stream.write_all(&batch) {
                                    error!("Failed to send batch to new runtime at {}: {}", addr, e);
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to read batches from session file: {}", e);
                        }
                    }
                    
                    // Add the runtime to our collection
                    runtimes_listener.lock().unwrap().push(runtime);
                    
                    // If we just connected our first runtime and have pending messages, send them
                    {
                        let buf = shared_buffer_listener.lock().unwrap();
                        if !buf.is_empty() {
                            let mut runtimes = runtimes_listener.lock().unwrap();
                            if runtimes.len() == 1 {
                                if let Err(e) = runtimes[0].stream.write_all(&buf) {
                                    error!("Failed to send pending buffer to new runtime: {}", e);
                                }
                            }
                        }
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    // No connection attempts right now, sleep a bit before polling again
                    thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    error!("TCP mode: Error accepting connection: {}", e);
                    thread::sleep(Duration::from_secs(1));
                }
            }
        }
    });
    
    // Clone for use in the flush thread
    let runtimes_flush = Arc::clone(&runtimes);
    let shared_buffer_flush = Arc::clone(&shared_buffer);
    let response_tx_flush = response_tx.clone();
    
    // Create a mutex-protected session file for the flush thread
    let session_file = Arc::new(Mutex::new(session_file));
    let session_file_flush = Arc::clone(&session_file);
    
    // Set the flush interval (e.g., every 10 seconds)
    let flush_interval = Duration::from_secs(10);
    
    // Batch counter for generating unique batch IDs
    let batch_counter: Arc<Mutex<u64>> = Arc::new(Mutex::new(0));
    let batch_counter_flush = Arc::clone(&batch_counter);
    
    // Spawn a thread to periodically flush the buffer and send to all runtimes
    thread::spawn(move || {
        loop {
            thread::sleep(flush_interval);
            
            // Lock buffer, check if there are any messages
            let mut buf = shared_buffer_flush.lock().unwrap();
            let mut runtimes = runtimes_flush.lock().unwrap();
            
            if runtimes.is_empty() {
                // No runtimes connected, keep accumulating messages
                debug!("No runtimes connected, skipping flush");
                continue;
            }
            
            // Create a clock command (10 seconds = 10_000_000_000 nanoseconds)
            if let Ok(clock_record) = write_record(&Command::Clock(10_000_000_000)) {
                // Only proceed if there's something to flush or we have runtimes that need updating
                if !buf.is_empty() || !runtimes.is_empty() {
                    // Add clock record to buffer
                    let original_size = buf.len();
                    debug!("Appending clock record to batch");
                    buf.extend(clock_record.clone());
                    
                    // Save the batch data
                    let batch_data = buf.clone();
                    
                    // Write to session file
                    let mut session_file = session_file_flush.lock().unwrap();
                    if let Err(e) = write_batch_to_file(&batch_data, &mut session_file) {
                        error!("Failed to write batch to session file: {}", e);
                    } else {
                        // Increment batch counter for this batch
                        let batch_id = {
                            let mut counter = batch_counter_flush.lock().unwrap();
                            *counter += 1;
                            *counter
                        };
                        
                        // Create response trackers for this batch
                        let mut active_runtimes = Vec::new();
                        let mut failed_runtimes = Vec::new();
                        
                        // Send to all connected runtimes
                        for (i, runtime) in runtimes.iter_mut().enumerate() {
                            match runtime.stream.write_all(&batch_data) {
                                Ok(_) => {
                                    if original_size > 0 {
                                        info!("Flushed {} bytes to runtime at {}", 
                                             batch_data.len(), runtime.address);
                                    } else {
                                        debug!("Sent clock update to runtime at {}", runtime.address);
                                    }
                                    
                                    // Add this runtime to the list we're waiting for responses from
                                    active_runtimes.push(i);
                                }
                                Err(e) => {
                                    error!("Error writing to runtime at {}: {}", 
                                          runtime.address, e);
                                    failed_runtimes.push(i);
                                }
                            }
                        }
                        
                        // Remove any runtimes that failed
                        for i in failed_runtimes.into_iter().rev() {
                            let removed = runtimes.remove(i);
                            warn!("Removed disconnected runtime at {}", removed.address);
                        }
                        
                        // Start a thread to listen for the first response
                        if !active_runtimes.is_empty() && original_size > 0 {
                            let runtimes_response = Arc::clone(&runtimes_flush);
                            let tx = response_tx_flush.clone();
                            let batch_id_clone = batch_id;
                            
                            thread::spawn(move || {
                                // Create a channel for each runtime to send its response
                                let (first_response_tx, first_response_rx) = mpsc::channel();
                                
                                // For each active runtime, spawn a thread to read its response
                                for runtime_idx in active_runtimes {
                                    let runtimes_clone = Arc::clone(&runtimes_response);
                                    let tx_clone = first_response_tx.clone();
                                    let thread_batch_id = batch_id_clone;
                                    
                                    thread::spawn(move || {
                                        // Get runtime stream
                                        let mut runtime_stream = {
                                            let runtimes = runtimes_clone.lock().unwrap();
                                            if runtime_idx >= runtimes.len() {
                                                return; // Runtime is gone
                                            }
                                            runtimes[runtime_idx].stream.try_clone().unwrap()
                                        };
                                        
                                        // Try to read a response (simple ACK)
                                        let mut response = [0u8; 1];
                                        match runtime_stream.read_exact(&mut response) {
                                            Ok(_) => {
                                                // Send the response through the channel
                                                let _ = tx_clone.send((thread_batch_id, runtime_idx, response[0] == 1));
                                            }
                                            Err(e) => {
                                                error!("Failed to read response from runtime {}: {}", 
                                                       runtime_idx, e);
                                            }
                                        }
                                    });
                                }
                                
                                // Wait for the first response or timeout
                                match first_response_rx.recv_timeout(Duration::from_secs(5)) {
                                    Ok((batch_id, runtime_idx, success)) => {
                                        // We got a response!
                                        let runtime_addr = {
                                            let runtimes = runtimes_response.lock().unwrap();
                                            if runtime_idx < runtimes.len() {
                                                runtimes[runtime_idx].address.clone()
                                            } else {
                                                "unknown".to_string()
                                            }
                                        };
                                        
                                        info!("Got {} response for batch {} from runtime at {}",
                                              if success { "successful" } else { "failed" },
                                              batch_id, runtime_addr);
                                        
                                        // Forward this to the main response channel
                                        let _ = tx.send((batch_id, success));
                                    }
                                    Err(_) => {
                                        // Timeout waiting for response
                                        warn!("Timeout waiting for response to batch {}", batch_id_clone);
                                        let _ = tx.send((batch_id_clone, false));
                                    }
                                }
                            });
                        }
                    }
                    
                    // Clear the buffer after flushing
                    buf.clear();
                }
            } else {
                error!("Failed to create clock record");
            }
        }
    });
    
    // Add a thread to read from runtime and handle network operations
    let nat_table_clone: Arc<Mutex<NatTable>> = Arc::clone(&nat_table);
    let shared_buffer_network = Arc::clone(&shared_buffer);
    let runtimes_network = Arc::clone(&runtimes);
    
    thread::spawn(move || {
        loop {
            // Sleep to prevent CPU spinning
            thread::sleep(Duration::from_millis(100));
            
            // Get a snapshot of current runtimes
            let mut runtime_streams = Vec::new();
            {
                let runtimes = runtimes_network.lock().unwrap();
                for runtime in runtimes.iter() {
                    if let Ok(stream) = runtime.stream.try_clone() {
                        runtime_streams.push(stream);
                    }
                }
            }
            
            // Nothing to do if no runtimes
            if runtime_streams.is_empty() {
                continue;
            }
            
            // Check each runtime for network operations (non-blocking)
            for mut stream in runtime_streams {
                // Try to read message type
                let mut msg_type_buf = [0u8; 1];
                match stream.read_exact(&mut msg_type_buf) {
                    Ok(_) => {
                        // If it's a NetworkOut message (type 5)
                        if msg_type_buf[0] == 5 {
                            // Set back to blocking mode for reliable reads
                            if let Err(e) = stream.set_nonblocking(false) {
                                error!("Failed to set stream back to blocking mode: {}", e);
                                continue;
                            }
                            
                            // Read process ID (8 bytes)
                            let mut pid_buf = [0u8; 8];
                            if stream.read_exact(&mut pid_buf).is_err() {
                                continue;
                            }
                            let pid = u64::from_le_bytes(pid_buf);
                            
                            // Read payload length (4 bytes)
                            let mut len_buf = [0u8; 4];
                            if stream.read_exact(&mut len_buf).is_err() {
                                continue;
                            }
                            let payload_len = u32::from_le_bytes(len_buf) as usize;
                            
                            // Read payload
                            let mut payload = vec![0u8; payload_len];
                            if stream.read_exact(&mut payload).is_err() {
                                continue;
                            }
                            
                            // Deserialize network operation
                            match bincode::deserialize::<NetworkOperation>(&payload) {
                                Ok(op) => {
                                    debug!("Received network operation from runtime for process {}: {:?}", pid, op);
                                    // Get source port before moving op
                                    let src_port = match &op {
                                        NetworkOperation::Listen { src_port } => *src_port,
                                        NetworkOperation::Accept { src_port, new_port: _ } => *src_port,
                                        NetworkOperation::Connect { src_port, .. } => *src_port,
                                        NetworkOperation::Send { src_port, .. } => *src_port,
                                        NetworkOperation::Close { src_port } => *src_port,
                                    };
                                    match nat_table_clone.lock().unwrap().handle_network_operation(pid, op) {
                                        Ok(success) => {
                                            // Send success status back to runtime
                                            let mut buf = shared_buffer_network.lock().unwrap();
                                            if let Ok(record) = write_record(&Command::NetworkIn(pid, 0, vec![if success { 1 } else { 0 }, src_port as u8, (src_port >> 8) as u8])) {
                                                buf.extend(record);
                                            }
                                        }
                                        Err(e) => {
                                            error!("Failed to handle network operation: {}", e);
                                            // Send error status back to runtime
                                            let mut buf = shared_buffer_network.lock().unwrap();
                                            if let Ok(record) = write_record(&Command::NetworkIn(pid, 0, vec![0])) {
                                                buf.extend(record);
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
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        // No data available, continue to next stream
                        continue;
                    }
                    Err(e) => {
                        // Connection error, will be handled by the flush thread
                        debug!("Error reading from runtime: {}", e);
                        continue;
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
                for (pid, port, data) in messages {
                    debug!("Received {} bytes from network for process {} port {}", data.len(), pid, port);
                    if let Ok(record) = write_record(&Command::NetworkIn(pid, port, data)) {
                        buf.extend(record);
                    }
                }
            }
        }
    });
    
    // Listen for batch responses
    thread::spawn(move || {
        loop {
            match response_rx.recv() {
                Ok((batch_id, success)) => {
                    info!("Batch {} processed: {}", 
                         batch_id, 
                         if success { "success" } else { "failed" });
                }
                Err(e) => {
                    error!("Error receiving batch response: {}", e);
                    break;
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
            debug!("Received command: {:?}", cmd);
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