use std::collections::HashMap;
use std::net::{TcpStream, TcpListener, SocketAddr};
use std::io::{Write, Read};
use log::{info, error, debug};
use crate::commands::NetworkOperation;

pub struct NatEntry {
    pub process_id: u64,
    pub process_port: u16,
    pub consensus_port: u16,
    pub connection: TcpStream,
}

pub struct NatListener {
    pub process_id: u64,
    pub process_port: u16,
    pub consensus_port: u16,
    pub listener: TcpListener,
}

pub struct NatTable {
    port_mappings: HashMap<u16, NatEntry>, // consensus_port -> entry
    process_ports: HashMap<(u64, u16), u16>, // (pid, process_port) -> consensus_port
    listeners: HashMap<u16, NatListener>, // consensus_port -> listener
    next_port: u16,
}

impl NatTable {
    pub fn new() -> Self {
        info!("Creating new NAT table");
        NatTable {
            port_mappings: HashMap::new(),
            process_ports: HashMap::new(),
            listeners: HashMap::new(),
            next_port: 10000, // Start from a high port number
        }
    }

    fn allocate_port(&mut self) -> u16 {
        let port = self.next_port;
        self.next_port += 1;
        debug!("Allocated new NAT port: {}", port);
        port
    }

    pub fn handle_network_operation(&mut self, pid: u64, op: NetworkOperation) -> Result<(), Box<dyn std::error::Error>> {
        debug!("Handling network operation for process {}: {:?}", pid, op);
        match op {
            NetworkOperation::Listen { src_port } => {
                let consensus_port = self.allocate_port();
                let addr = format!("127.0.0.1:{}", consensus_port);
                
                debug!("Attempting to listen on {}", addr);
                match TcpListener::bind(&addr) {
                    Ok(listener) => {
                        // Set to non-blocking mode
                        if let Err(e) = listener.set_nonblocking(true) {
                            error!("Failed to set non-blocking mode: {}", e);
                        }
                        
                        let entry = NatListener {
                            process_id: pid,
                            process_port: src_port,
                            consensus_port,
                            listener,
                        };
                        
                        self.listeners.insert(consensus_port, entry);
                        self.process_ports.insert((pid, src_port), consensus_port);
                        info!("Created NAT listener: {}:{} -> consensus:{}", 
                            pid, src_port, consensus_port);
                    }
                    Err(e) => {
                        error!("Failed to listen on {}: {}", addr, e);
                        return Err(Box::new(e));
                    }
                }
            }
            NetworkOperation::Accept { src_port } => {
                // Find the listener for this process:port
                if let Some(&consensus_port) = self.process_ports.get(&(pid, src_port)) {
                    if let Some(listener) = self.listeners.get(&consensus_port) {
                        match listener.listener.accept() {
                            Ok((stream, addr)) => {
                                // Set to non-blocking mode
                                if let Err(e) = stream.set_nonblocking(true) {
                                    error!("Failed to set non-blocking mode: {}", e);
                                }
                                
                                let new_consensus_port = self.allocate_port();
                                let entry = NatEntry {
                                    process_id: pid,
                                    process_port: src_port,
                                    consensus_port: new_consensus_port,
                                    connection: stream,
                                };
                                
                                self.port_mappings.insert(new_consensus_port, entry);
                                info!("Accepted connection from {} on {}:{} -> consensus:{}", 
                                    addr, pid, src_port, new_consensus_port);
                                
                                // Return the new consensus port to the process
                                // This will be handled by the runtime
                                return Ok(());
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                // No connection available
                                debug!("No connection available for accept on {}:{}", pid, src_port);
                                return Ok(());
                            }
                            Err(e) => {
                                error!("Failed to accept connection on {}:{}: {}", pid, src_port, e);
                                return Err(Box::new(e));
                            }
                        }
                    } else {
                        error!("No listener found for consensus port {}", consensus_port);
                    }
                } else {
                    error!("No NAT mapping found for process {}:{}", pid, src_port);
                }
            }
            NetworkOperation::Connect { dest_addr, dest_port, src_port } => {
                let consensus_port = self.allocate_port();
                let addr = format!("{}:{}", dest_addr, dest_port);
                
                debug!("Attempting to connect to {}", addr);
                match TcpStream::connect(&addr) {
                    Ok(stream) => {
                        // Set to non-blocking mode
                        if let Err(e) = stream.set_nonblocking(true) {
                            error!("Failed to set non-blocking mode: {}", e);
                        }
                        
                        let entry = NatEntry {
                            process_id: pid,
                            process_port: src_port,
                            consensus_port,
                            connection: stream,
                        };
                        
                        self.port_mappings.insert(consensus_port, entry);
                        self.process_ports.insert((pid, src_port), consensus_port);
                        info!("Created NAT entry: {}:{} -> consensus:{} -> {}:{}", 
                            pid, src_port, consensus_port, dest_addr, dest_port);
                    }
                    Err(e) => {
                        error!("Failed to connect to {}: {}", addr, e);
                        return Err(Box::new(e));
                    }
                }
            }
            NetworkOperation::Send { src_port, data } => {
                info!("Processing send operation for process {}:{} ({} bytes): {:?}", 
                     pid, src_port, data.len(), String::from_utf8_lossy(&data));
                
                // Check if we have a mapping for this process:port
                if let Some(&consensus_port) = self.process_ports.get(&(pid, src_port)) {
                    debug!("Found NAT mapping: process {}:{} -> consensus:{}", pid, src_port, consensus_port);
                    
                    if let Some(entry) = self.port_mappings.get_mut(&consensus_port) {
                        debug!("Found port mapping entry, attempting to write {} bytes", data.len());
                        match entry.connection.write_all(&data) {
                            Ok(_) => {
                                // Explicitly flush the connection
                                if let Err(e) = entry.connection.flush() {
                                    error!("Failed to flush data to connection: {}", e);
                                    return Err(Box::new(e));
                                }
                                info!("Successfully sent and flushed {} bytes to destination", data.len());
                            }
                            Err(e) => {
                                error!("Failed to send data to destination: {}", e);
                                return Err(Box::new(e));
                            }
                        }
                    } else {
                        error!("Inconsistent state: consensus port {} found but no mapping entry exists", consensus_port);
                    }
                } else {
                    error!("No NAT mapping found for process {}:{}", pid, src_port);
                }
            }
            NetworkOperation::Close { src_port } => {
                debug!("Processing close operation for process {}:{}", pid, src_port);
                if let Some(&consensus_port) = self.process_ports.get(&(pid, src_port)) {
                    self.port_mappings.remove(&consensus_port);
                    self.process_ports.remove(&(pid, src_port));
                    info!("Closed NAT entry for {}:{}", pid, src_port);
                } else {
                    error!("No NAT mapping found to close for process {}:{}", pid, src_port);
                }
            }
        }
        Ok(())
    }

    pub fn check_for_incoming_data(&mut self) -> Vec<(u64, u16, Vec<u8>)> {
        //debug!("Checking for incoming data on all NAT connections (total connections: {})", self.port_mappings.len());
        let mut messages = Vec::new();
        let mut to_remove = Vec::new();

        for (consensus_port, entry) in &mut self.port_mappings {
            let mut buf = [0u8; 1024];
            //debug!("Checking for data on NAT port {}", consensus_port);
            match entry.connection.read(&mut buf) {
                Ok(0) => {
                    info!("Connection closed by remote for {}:{}", entry.process_id, entry.process_port);
                    to_remove.push(*consensus_port);
                }
                Ok(n) => {
                    info!("Received {} bytes from connection for process {}:{}: {:?}", 
                         n, entry.process_id, entry.process_port, String::from_utf8_lossy(&buf[..n]));
                    messages.push((
                        entry.process_id,
                        entry.process_port,
                        buf[..n].to_vec()
                    ));
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No data available
                    //debug!("No data available on NAT port {}", consensus_port);
                    continue;
                }
                Err(e) => {
                    error!("Error reading from connection {}:{}: {}", 
                        entry.process_id, entry.process_port, e);
                    to_remove.push(*consensus_port);
                }
            }
        }

        // Clean up closed connections
        for port in to_remove {
            if let Some(entry) = self.port_mappings.remove(&port) {
                self.process_ports.remove(&(entry.process_id, entry.process_port));
                info!("Removed NAT entry for {}:{}", entry.process_id, entry.process_port);
            }
        }

        messages
    }
} 