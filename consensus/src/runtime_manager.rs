use std::io::{self, Write};
use std::net::{TcpStream, TcpListener};
use std::sync::{Arc, Mutex};
use std::thread;
use std::collections::HashMap;
use log::{error, info, debug, warn};
pub use crate::batch::{Batch, BatchDirection};
use crate::batch_history::BatchHistory;

/// Represents a connected runtime.
#[derive(Clone)]
pub struct RuntimeConnection {
    pub stream: Arc<Mutex<TcpStream>>,
    pub last_processed_batch: u64,
}

/// Manages multiple runtime connections and session batches.
#[derive(Clone)]
pub struct RuntimeManager {
    pub listener: Arc<TcpListener>,
    pub runtimes: Arc<Mutex<HashMap<u64, RuntimeConnection>>>,
    next_runtime_id: Arc<Mutex<u64>>,
    batch_history: Arc<Mutex<BatchHistory>>,
}

impl RuntimeManager {
    pub fn new(addr: &str, batch_history: Arc<Mutex<BatchHistory>>) -> io::Result<Self> {
        info!("Initializing RuntimeManager on {}", addr);
        let listener = Arc::new(TcpListener::bind(addr)?);
        let runtimes = Arc::new(Mutex::new(HashMap::new()));
        let next_runtime_id = Arc::new(Mutex::new(0));
        info!("RuntimeManager: Listening for runtimes on {}...", addr);
        Ok(Self {
            listener,
            runtimes,
            next_runtime_id,
            batch_history,
        })
    }

    /// Accepts new runtime connections and assigns them an ID.
    pub fn start_accepting(&self) {
        info!("Starting runtime connection acceptor");
        let runtimes = Arc::clone(&self.runtimes);
        let next_runtime_id = Arc::clone(&self.next_runtime_id);
        let listener = self.listener.try_clone().expect("Failed to clone listener");
        let batch_history = Arc::clone(&self.batch_history);
        thread::spawn(move || {
            info!("Runtime acceptor thread started");
            for stream in listener.incoming() {
                match stream {
                    Ok(mut stream) => {
                        let mut id_lock = next_runtime_id.lock().unwrap();
                        let runtime_id = *id_lock;
                        *id_lock += 1;
                        drop(id_lock);
                        info!("Accepted runtime {} from {}", runtime_id, stream.peer_addr().unwrap());
                        
                        // Send historical batches to new runtime
                        if let Ok(batches) = batch_history.lock().unwrap().get_batches_since(0) {
                            // Filter to only include incoming batches
                            let incoming_batches: Vec<_> = batches.into_iter()
                                .filter(|batch| matches!(batch.direction, BatchDirection::Incoming))
                                .collect();
                            
                            info!("Sending {} historical incoming batches to new runtime {}", 
                                incoming_batches.len(), runtime_id);
                            
                            for batch in incoming_batches {
                                // Create a new buffer for each batch to ensure clean state
                                let mut serialized = Vec::new();
                                // Write batch number (8 bytes)
                                serialized.extend_from_slice(&batch.number.to_le_bytes());
                                // Write direction (1 byte)
                                serialized.push(0); // Always Incoming (0) since we filtered
                                // Write data length (8 bytes)
                                serialized.extend_from_slice(&(batch.data.len() as u64).to_le_bytes());
                                // Write the actual data
                                serialized.extend_from_slice(&batch.data);
                                
                                // Write the entire batch at once
                                match stream.write_all(&serialized) {
                                    Ok(_) => {
                                        if let Err(e) = stream.flush() {
                                            error!("Failed to flush historical batch {} to runtime {}: {}", batch.number, runtime_id, e);
                                            break;
                                        }
                                        debug!("Successfully sent historical batch {} to runtime {} ({} bytes)", 
                                            batch.number, runtime_id, serialized.len());
                                    }
                                    Err(e) => {
                                        error!("Failed to send historical batch {} to runtime {}: {}", batch.number, runtime_id, e);
                                        break;
                                    }
                                }
                            }
                        }
                        
                        let conn = RuntimeConnection {
                            stream: Arc::new(Mutex::new(stream)),
                            last_processed_batch: batch_history.lock().unwrap().get_current_batch(),
                        };
                        runtimes.lock().unwrap().insert(runtime_id, conn);
                        info!("Runtime {} added to connection pool", runtime_id);
                    }
                    Err(e) => {
                        error!("Failed to accept runtime: {}", e);
                    }
                }
            }
            warn!("Runtime acceptor thread ended unexpectedly");
        });
        info!("Runtime connection acceptor started successfully");
    }

    /// Broadcasts a batch to all connected runtimes that haven't processed it yet.
    pub fn broadcast_batch(&self, batch: &Batch) {
        debug!("Broadcasting batch {} to all runtimes ({} bytes)", batch.number, batch.data.len());
        let conns = self.runtimes.lock().unwrap();
        let mut sent_count = 0;
        let mut error_count = 0;
        
        if conns.is_empty() {
            warn!("No runtimes connected to broadcast batch {}", batch.number);
            return;
        }

        info!("Found {} connected runtimes", conns.len());
        for (runtime_id, conn) in conns.iter() {
            debug!("Runtime {} last processed batch: {}", runtime_id, conn.last_processed_batch);
        }

        // Serialize the batch header and data
        let mut serialized = Vec::new();
        // Write batch number (8 bytes)
        serialized.extend_from_slice(&batch.number.to_le_bytes());
        // Write direction (1 byte)
        serialized.push(match batch.direction {
            BatchDirection::Incoming => 0,
            BatchDirection::Outgoing => 1,
        });
        // Write data length (8 bytes)
        serialized.extend_from_slice(&(batch.data.len() as u64).to_le_bytes());
        // Write the actual data
        serialized.extend_from_slice(&batch.data);

        // Get list of runtimes to process
        let runtimes_to_process: Vec<(u64, Arc<Mutex<TcpStream>>)> = conns.iter()
            .filter(|(_, conn)| conn.last_processed_batch <= batch.number)
            .map(|(id, conn)| (*id, conn.stream.clone()))
            .collect();

        // Release the lock before sending
        drop(conns);

        // Process each runtime
        for (runtime_id, stream) in runtimes_to_process {
            debug!("Sending batch {} to runtime {} (last processed: {})", 
                batch.number, runtime_id, batch.number - 1);
            
            let mut stream_guard = stream.lock().unwrap();
            match stream_guard.write_all(&serialized) {
                Ok(_) => {
                    debug!("Batch {} sent to runtime {}", batch.number, runtime_id);
                    if let Err(e) = stream_guard.flush() {
                        error!("Failed to flush batch {} to runtime {}: {}", batch.number, runtime_id, e);
                        error_count += 1;
                        continue;
                    }
                    // Update last processed batch
                    let mut conns = self.runtimes.lock().unwrap();
                    if let Some(conn) = conns.get_mut(&runtime_id) {
                        conn.last_processed_batch = batch.number;
                    }
                    sent_count += 1;
                    info!("Successfully sent batch {} to runtime {} ({} bytes)", 
                        batch.number, runtime_id, serialized.len());
                }
                Err(e) => {
                    error!("Failed to send batch {} to runtime {}: {}", batch.number, runtime_id, e);
                    error_count += 1;
                }
            }
        }

        info!("Batch {} broadcast complete (sent to {} runtimes, {} errors)", 
            batch.number, sent_count, error_count);
    }

    /// Sends the session file (all previous batches) to a specific runtime.
    pub fn send_session_file(&self, runtime_id: u64, session_data: &[u8], batch_number: u64) -> io::Result<()> {
        info!("Sending session file to runtime {} ({} bytes, up to batch {})", 
            runtime_id, session_data.len(), batch_number);
        let mut conns = self.runtimes.lock().unwrap();
        if let Some(conn) = conns.get_mut(&runtime_id) {
            if let Err(e) = conn.stream.lock().unwrap().write_all(session_data) {
                error!("Failed to send session file to runtime {}: {}", runtime_id, e);
                return Err(e);
            }
            conn.last_processed_batch = batch_number;
            info!("Successfully sent session file to runtime {}", runtime_id);
            Ok(())
        } else {
            error!("Runtime {} not found for session file transfer", runtime_id);
            Err(io::Error::new(io::ErrorKind::NotFound, "Runtime not found"))
        }
    }

    /// Handles an outgoing batch from a runtime. Returns true if the batch was processed, false if it was ignored.
    pub fn handle_outgoing_batch(&self, runtime_id: u64, batch: &Batch) -> bool {
        debug!("Handling outgoing batch {} from runtime {}", batch.number, runtime_id);
        let mut conns = self.runtimes.lock().unwrap();
        if let Some(conn) = conns.get_mut(&runtime_id) {
            if conn.last_processed_batch < batch.number {
                info!("Processing outgoing batch {} from runtime {}", batch.number, runtime_id);
                conn.last_processed_batch = batch.number;
                true
            } else {
                debug!("Ignoring outgoing batch {} from runtime {} (already processed)", batch.number, runtime_id);
                false
            }
        } else {
            error!("Runtime {} not found for outgoing batch", runtime_id);
            false
        }
    }

    /// Returns a clone of the TcpStream for the first runtime in the runtimes map.
    pub fn get_runtime_stream(&self) -> io::Result<TcpStream> {
        debug!("Attempting to get stream for first runtime");
        let conns = self.runtimes.lock().unwrap();
        if let Some((runtime_id, conn)) = conns.iter().next() {
            debug!("Found runtime {} for stream clone", runtime_id);
            conn.stream.lock().unwrap().try_clone().map_err(|e| {
                error!("Failed to clone stream for runtime {}: {}", runtime_id, e);
                io::Error::new(io::ErrorKind::Other, e)
            })
        } else {
            warn!("No runtimes available for stream clone");
            Err(io::Error::new(io::ErrorKind::NotFound, "No runtimes connected"))
        }
    }
} 