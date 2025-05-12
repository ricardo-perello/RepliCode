use std::io::{self, Write, Read, BufReader};
use std::net::{TcpStream, TcpListener};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::path::PathBuf;
use log::{error, info, debug, warn};
use bincode;
use chrono::Local;

use crate::record::write_record;
use crate::commands::{parse_command, Command, NetworkOperation};
use crate::nat::NatTable;
use crate::http_server::HttpServer;
use crate::runtime_manager::RuntimeManager;
use crate::batch::{Batch, BatchDirection};
use crate::batch_history::BatchHistory;

pub struct TcpMode {
    runtime_manager: RuntimeManager,
    nat_table: Arc<Mutex<NatTable>>,
    shared_buffer: Arc<Mutex<Vec<u8>>>,
    batch_history: Arc<Mutex<BatchHistory>>,
}

impl TcpMode {
    pub fn new() -> io::Result<Self> {
        info!("Initializing TcpMode");
        
        // Initialize batch history first
        let date = Local::now().format("%Y%m%d-%H%M%S").to_string();
        let history_path = PathBuf::from(format!("session-{}.bin", date));
        let batch_history: Arc<Mutex<BatchHistory>> = Arc::new(Mutex::new(BatchHistory::new(&history_path)?));
        
        let runtime_manager = RuntimeManager::new("127.0.0.1:9000", Arc::clone(&batch_history))?;
        let nat_table = Arc::new(Mutex::new(NatTable::new()));
        let shared_buffer = Arc::new(Mutex::new(Vec::new()));
        
        info!("TcpMode initialized successfully");
        Ok(Self {
            runtime_manager,
            nat_table,
            shared_buffer,
            batch_history,
        })
    }

    pub fn run(&self) -> io::Result<()> {
        info!("Starting TcpMode");
        
        // Start accepting runtime connections
        info!("Starting runtime connection acceptor");
        self.runtime_manager.start_accepting();
        
        // Start the batch sender thread
        info!("Starting batch sender thread");
        self.start_batch_sender()?;
        
        // Start the runtime reader thread
        info!("Starting runtime reader thread");
        self.start_runtime_reader()?;
        
        // Start the NAT checker thread
        info!("Starting NAT checker thread");
        self.start_nat_checker()?;
        
        // Start the HTTP server
        info!("Starting HTTP server");
        self.start_http_server()?;
        
        // Run the main command loop
        info!("Starting main command loop");
        self.run_command_loop()?;
        
        info!("TcpMode shutdown complete");
        Ok(())
    }

    fn start_batch_sender(&self) -> io::Result<()> {
        debug!("Initializing batch sender thread");
        let buffer = Arc::clone(&self.shared_buffer);
        let runtime_manager = self.runtime_manager.clone();
        let batch_history: Arc<Mutex<BatchHistory>> = Arc::clone(&self.batch_history);
        thread::spawn(move || {
            let mut batch_number = 0u64;
            info!("Batch sender thread started");
            loop {
                thread::sleep(Duration::from_secs(10));
                let mut buf = buffer.lock().unwrap();
                batch_number += 1;
                debug!("Creating new batch {} with {} bytes", batch_number, buf.len());
                
                // Append clock record for 10 seconds
                if let Ok(clock_record) = write_record(&Command::Clock(10_000_000_000)) {
                    buf.extend(clock_record);
                    debug!("Added clock record for 10 seconds");
                } else {
                    error!("Failed to create clock record");
                }

                let batch = Batch {
                    number: batch_number,
                    direction: BatchDirection::Incoming,
                    data: buf.clone(),
                };
                
                // Save batch to history
                if let Err(e) = batch_history.lock().unwrap().save_batch(&batch) {
                    error!("Failed to save batch {} to history: {}", batch_number, e);
                }
                
                info!("Broadcasting batch {} to all runtimes", batch.number);
                runtime_manager.broadcast_batch(&batch);
                buf.clear();
                debug!("Batch {} broadcast complete, buffer cleared", batch_number);
            }
        });
        info!("Batch sender thread initialized successfully");
        Ok(())
    }

    fn start_runtime_reader(&self) -> io::Result<()> {
        debug!("Initializing runtime reader thread");
        let runtime_manager = self.runtime_manager.clone();
        let nat_table = Arc::clone(&self.nat_table);
        let shared_buffer = Arc::clone(&self.shared_buffer);
        thread::spawn(move || {
            info!("Runtime reader thread started");
            loop {
                // Get list of runtime IDs
                let runtime_ids: Vec<u64> = {
                    let conns = runtime_manager.runtimes.lock().unwrap();
                    conns.keys().copied().collect()
                };
                
                for runtime_id in runtime_ids {
                    // Get connection for this runtime
                    let conn = {
                        let mut conns = runtime_manager.runtimes.lock().unwrap();
                        if let Some(conn) = conns.get_mut(&runtime_id) {
                            conn.stream.lock().unwrap().try_clone().ok()
                        } else {
                            None
                        }
                    };

                    if let Some(stream) = conn {
                        debug!("Reading from runtime {}", runtime_id);
                        let mut reader = BufReader::new(stream);
                        
                        // Read batch header (8 bytes for batch number, 1 byte for direction)
                        let mut batch_header = [0u8; 9];
                        if reader.read_exact(&mut batch_header).is_err() {
                            error!("Lost connection to runtime {}", runtime_id);
                            // Remove the disconnected runtime
                            let mut conns = runtime_manager.runtimes.lock().unwrap();
                            conns.remove(&runtime_id);
                            continue;
                        }
                        let batch_number = u64::from_le_bytes(batch_header[0..8].try_into().unwrap());
                        let direction = batch_header[8];
                        debug!("Received batch {} with direction {} from runtime {}", batch_number, direction, runtime_id);

                        // Read batch data length (8 bytes)
                        let mut data_len_buf = [0u8; 8];
                        if reader.read_exact(&mut data_len_buf).is_err() {
                            error!("Failed to read batch data length from runtime {}", runtime_id);
                            continue;
                        }
                        let data_len = u64::from_le_bytes(data_len_buf) as usize;
                        debug!("Reading {} bytes of batch data from runtime {}", data_len, runtime_id);

                        // Read the batch data
                        let mut batch_data = vec![0u8; data_len];
                        if reader.read_exact(&mut batch_data).is_err() {
                            error!("Failed to read batch data from runtime {}", runtime_id);
                            continue;
                        }

                        // Process the batch data as a series of records
                        let mut data_reader = std::io::Cursor::new(batch_data);
                        loop {
                            // Read the message type (1 byte)
                            let mut msg_type_buf = [0u8; 1];
                            if data_reader.read_exact(&mut msg_type_buf).is_err() {
                                debug!("No more records in batch {} from runtime {}", batch_number, runtime_id);
                                break; // No more data.
                            }
                            let msg_type = msg_type_buf[0];
                            debug!("Processing record type {} in batch {} from runtime {}", msg_type, batch_number, runtime_id);
                            
                            // If it's a NetworkOut message (type 5)
                            if msg_type == 5 {
                                debug!("Processing NetworkOut message from runtime {}", runtime_id);
                                // Read process ID (8 bytes)
                                let mut pid_buf = [0u8; 8];
                                if data_reader.read_exact(&mut pid_buf).is_err() {
                                    error!("Failed to read process ID from runtime {}", runtime_id);
                                    break;
                                }
                                let pid = u64::from_le_bytes(pid_buf);
                                debug!("NetworkOut message for process {}", pid);
                                
                                // Read payload length (4 bytes)
                                let mut len_buf = [0u8; 4];
                                if data_reader.read_exact(&mut len_buf).is_err() {
                                    error!("Failed to read payload length from runtime {}", runtime_id);
                                    break;
                                }
                                let payload_len = u32::from_le_bytes(len_buf) as usize;
                                debug!("Reading {} bytes of payload", payload_len);
                                
                                // Read payload
                                let mut payload = vec![0u8; payload_len];
                                if data_reader.read_exact(&mut payload).is_err() {
                                    error!("Failed to read payload from runtime {}", runtime_id);
                                    break;
                                }
                                
                                // Handle network operation
                                if let Ok(op) = bincode::deserialize::<NetworkOperation>(&payload) {
                                    info!("Processing network operation from runtime {}: {:?}", runtime_id, op);
                                    let (src_port, new_port, is_accept, is_recv) = match &op {
                                        NetworkOperation::Connect { src_port, .. } => (*src_port, 0, false, false),
                                        NetworkOperation::Send { src_port, .. } => (*src_port, 0, false, false),
                                        NetworkOperation::Listen { src_port } => (*src_port, 0, false, false),
                                        NetworkOperation::Accept { src_port, new_port, .. } => (*src_port, *new_port, true, false),
                                        NetworkOperation::Close { src_port } => (*src_port, 0, false, false),
                                        NetworkOperation::Recv { src_port } => (*src_port, 0, false, true),
                                    };

                                    // Process the network operation
                                    let mut nat_table = nat_table.lock().unwrap();
                                    let status: u8 = match nat_table.handle_network_operation(pid, op.clone()) {
                                        Ok(success) => {
                                            if !success {
                                                0  // Return status 0 for failure
                                            } else {
                                                // Check if operation is waiting
                                                let is_waiting = match &op {
                                                    NetworkOperation::Accept { src_port, .. } => nat_table.is_waiting_for_accept(pid, *src_port),
                                                    NetworkOperation::Recv { src_port } => nat_table.is_waiting_for_recv(pid, *src_port),
                                                    _ => false
                                                };
                                                
                                                if is_waiting {
                                                    debug!("Operation is waiting for process {}:{}", pid, src_port);
                                                    2 // Return status 2 for waiting
                                                } else {
                                                    1 // Return status 1 for success
                                                }
                                            }
                                        },
                                        Err(e) => {
                                            error!("Failed to handle network operation: {}", e);
                                            0
                                        }
                                    };

                                    // Add success/failure message to batch
                                    let mut buf = shared_buffer.lock().unwrap();
                                    if let Ok(record) = write_record(&Command::NetworkIn(pid, 0, vec![
                                        status,  // Use the computed status code
                                        src_port as u8, (src_port >> 8) as u8,  // Source port
                                        if is_accept { new_port as u8 } else { 0 },  // New port for accept
                                        if is_accept { (new_port >> 8) as u8 } else { 0 }  // New port high byte
                                    ])) {
                                        buf.extend(record);
                                        info!("Added network operation result for process {}:{} (status: {})", 
                                            pid, src_port, status);
                                    }
                                } else {
                                    error!("Failed to deserialize network operation from runtime {}", runtime_id);
                                }
                            }
                        }
                    }
                }
                // Sleep briefly to avoid tight loop
                thread::sleep(Duration::from_millis(10));
            }
            warn!("Runtime reader thread ended unexpectedly");
        });
        info!("Runtime reader thread initialized successfully");
        Ok(())
    }

    fn start_nat_checker(&self) -> io::Result<()> {
        debug!("Initializing NAT checker thread");
        let nat_table = Arc::clone(&self.nat_table);
        let shared_buffer = Arc::clone(&self.shared_buffer);
        
        thread::spawn(move || {
            info!("NAT checker thread started");
            loop {
                thread::sleep(Duration::from_millis(100));
                let messages = nat_table.lock().unwrap().check_for_incoming_data();
                if !messages.is_empty() {
                    debug!("Processing {} NAT messages", messages.len());
                    let mut buf = shared_buffer.lock().unwrap();
                    for (pid, port, data, is_connection) in messages {
                        debug!("Processing NAT message for process {}:{} (connection: {})", 
                            pid, port, is_connection);
                        if is_connection {
                            if let Ok(record) = write_record(&Command::NetworkIn(pid, 0, vec![
                                1,  // Success status
                                port as u8, (port >> 8) as u8,  // Listening port
                                (port + 1) as u8, ((port + 1) >> 8) as u8  // New port
                            ])) {
                                buf.extend(record);
                                info!("Added connection notification for process {}:{}", pid, port);
                            }
                        } else if !data.is_empty() {
                            debug!("Adding {} bytes of data for process {}:{}", data.len(), pid, port);
                            if let Ok(record) = write_record(&Command::NetworkIn(pid, port, data)) {
                                buf.extend(record);
                            }
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
        
        info!("NAT checker thread initialized successfully");
        Ok(())
    }

    fn start_http_server(&self) -> io::Result<()> {
        debug!("Initializing HTTP server");
        let http_server = HttpServer::new(Arc::clone(&self.nat_table));
        thread::spawn(move || {
            info!("HTTP server thread started");
            if let Err(e) = http_server.start(8080) {
                error!("HTTP server error: {}", e);
            }
            warn!("HTTP server thread ended unexpectedly");
        });
        info!("HTTP status server started on port 8080");
        Ok(())
    }

    fn run_command_loop(&self) -> io::Result<()> {
        info!("Starting command loop");
        loop {
            eprint!("Command (init <wasm_file> | msg <pid> <message>): ");
            io::stderr().flush()?;
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            let input = input.trim();
            
            if input.eq_ignore_ascii_case("exit") {
                info!("Received exit command");
                break;
            }
            
            debug!("Processing command: {}", input);
            if let Some(cmd) = parse_command(input) {
                //info!("Parsed command: {:?}", cmd);
                if let Ok(record) = write_record(&cmd) {
                    debug!("Writing command record ({} bytes)", record.len());
                    let mut buf = self.shared_buffer.lock().unwrap();
                    buf.extend(record);
                    info!("Command added to shared buffer");
                } else {
                    error!("Failed to write command record");
                }
            } else {
                warn!("Failed to parse command: {}", input);
            }
        }
        
        info!("Command loop ended");
        Ok(())
    }
}

pub fn run_tcp_mode() -> io::Result<()> {
    info!("Starting TCP mode");
    let tcp_mode = TcpMode::new()?;
    tcp_mode.run()
} 