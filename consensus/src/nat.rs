use std::collections::HashMap;
use std::net::{TcpStream, TcpListener};
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
    connections: HashMap<(u64, u16), u16>, // (pid, process_port) -> connection_consensus_port
    next_port: u16,
    pending_accepts: HashMap<(u64, u16), bool>, // (pid, src_port) -> has_connection
}

impl NatTable {
    pub fn new() -> Self {
        info!("Creating new NAT table");
        NatTable {
            port_mappings: HashMap::new(),
            process_ports: HashMap::new(),
            listeners: HashMap::new(),
            connections: HashMap::new(),
            next_port: 10000, // Start from a high port number
            pending_accepts: HashMap::new(),
        }
    }

    fn allocate_port(&mut self) -> u16 {
        let port = self.next_port;
        self.next_port += 1;
        debug!("Allocated new NAT port: {}", port);
        port
    }

    pub fn handle_network_operation(&mut self, pid: u64, op: NetworkOperation) -> Result<bool, Box<dyn std::error::Error>> {
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
                        Ok(true) // Success
                    }
                    Err(e) => {
                        error!("Failed to listen on {}: {}", addr, e);
                        Err(Box::new(e))
                    }
                }
            }
            NetworkOperation::Accept { src_port, new_port } => {
                // Find the listener for this process:port
                if let Some(&listener_consensus_port) = self.process_ports.get(&(pid, src_port)) {
                    if let Some(listener) = self.listeners.get(&listener_consensus_port) {
                        // Try to accept once
                        match listener.listener.accept() {
                            Ok((stream, addr)) => {
                                // Set to non-blocking mode
                                if let Err(e) = stream.set_nonblocking(true) {
                                    error!("Failed to set non-blocking mode: {}", e);
                                }
                                
                                // Only allocate a new port when we successfully accept
                                let consensus_port = self.allocate_port();
                                let entry = NatEntry {
                                    process_id: pid,
                                    process_port: new_port,  // Use the new port
                                    consensus_port,
                                    connection: stream,
                                };
                                
                                // Keep the listener's port mapping and add new one for accepted connection
                                self.port_mappings.insert(consensus_port, entry);
                                self.connections.insert((pid, new_port), consensus_port);  // Use new_port
                                self.pending_accepts.insert((pid, src_port), true);
                                info!("Accepted connection from {} on {}:{} -> new port {} (listener: {})", 
                                    addr, pid, src_port, new_port, listener_consensus_port);
                                Ok(true)
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                // No connection available yet
                                self.pending_accepts.insert((pid, src_port), false);
                                debug!("No connection available for {}:{}", pid, src_port);
                                Ok(false)
                            }
                            Err(e) => {
                                error!("Failed to accept connection on {}:{}: {}", pid, src_port, e);
                                Err(Box::new(e))
                            }
                        }
                    } else {
                        error!("No listener found for consensus port {}", listener_consensus_port);
                        Ok(false)
                    }
                } else {
                    error!("No NAT mapping found for process {}:{}", pid, src_port);
                    Ok(false)
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
                        self.connections.insert((pid, src_port), consensus_port);  // Add to connections map
                        info!("Created NAT entry: {}:{} -> consensus:{} -> {}:{}", 
                            pid, src_port, consensus_port, dest_addr, dest_port);
                        Ok(true)
                    }
                    Err(e) => {
                        error!("Failed to connect to {}: {}", addr, e);
                        Err(Box::new(e))
                    }
                }
            }
            NetworkOperation::Send { src_port, data } => {
                info!("Processing send operation for process {}:{} ({} bytes): {:?}", 
                     pid, src_port, data.len(), String::from_utf8_lossy(&data));
                
                // First check for an active connection
                if let Some(&consensus_port) = self.connections.get(&(pid, src_port)) {
                    debug!("Found connection mapping: process {}:{} -> consensus:{}", pid, src_port, consensus_port);
                    if let Some(entry) = self.port_mappings.get_mut(&consensus_port) {
                        debug!("Found connection entry, attempting to write {} bytes", data.len());
                        match entry.connection.write_all(&data) {
                            Ok(_) => {
                                if let Err(e) = entry.connection.flush() {
                                    error!("Failed to flush data to connection: {}", e);
                                    return Err(Box::new(e));
                                }
                                info!("Successfully sent and flushed {} bytes to connection", data.len());
                                Ok(true)
                            }
                            Err(e) => {
                                error!("Failed to send data to connection: {}", e);
                                Err(Box::new(e))
                            }
                        }
                    } else {
                        error!("Inconsistent state: consensus port {} found but no mapping entry exists", consensus_port);
                        Ok(false)
                    }
                }
                // If no connection found, check for a listener
                else if let Some(&consensus_port) = self.process_ports.get(&(pid, src_port)) {
                    debug!("Found listener mapping: process {}:{} -> consensus:{}", pid, src_port, consensus_port);
                    if let Some(entry) = self.port_mappings.get_mut(&consensus_port) {
                        debug!("Found listener entry, attempting to write {} bytes", data.len());
                        match entry.connection.write_all(&data) {
                            Ok(_) => {
                                if let Err(e) = entry.connection.flush() {
                                    error!("Failed to flush data to listener: {}", e);
                                    return Err(Box::new(e));
                                }
                                info!("Successfully sent and flushed {} bytes to listener", data.len());
                                Ok(true)
                            }
                            Err(e) => {
                                error!("Failed to send data to listener: {}", e);
                                Err(Box::new(e))
                            }
                        }
                    } else {
                        error!("Inconsistent state: consensus port {} found but no mapping entry exists", consensus_port);
                        Ok(false)
                    }
                } else {
                    error!("No NAT mapping found for process {}:{}", pid, src_port);
                    Ok(false)
                }
            }
            NetworkOperation::Close { src_port } => {
                debug!("Processing close operation for process {}:{}", pid, src_port);
                
                // First check if this is a connection
                if let Some(&consensus_port) = self.connections.get(&(pid, src_port)) {
                    self.port_mappings.remove(&consensus_port);
                    self.connections.remove(&(pid, src_port));
                    info!("Closed connection for {}:{}", pid, src_port);
                    Ok(true)
                }
                // If not a connection, check if it's a listener
                else if let Some(&consensus_port) = self.process_ports.get(&(pid, src_port)) {
                    self.port_mappings.remove(&consensus_port);
                    self.process_ports.remove(&(pid, src_port));
                    self.listeners.remove(&consensus_port);
                    info!("Closed listener for {}:{}", pid, src_port);
                    Ok(true)
                } else {
                    error!("No NAT mapping found for process {}:{}", pid, src_port);
                    Ok(false)
                }
            }
        }
    }

    pub fn has_pending_accept(&self, pid: u64, src_port: u16) -> bool {
        self.pending_accepts.get(&(pid, src_port)).copied().unwrap_or(false)
    }

    pub fn has_port_mapping(&self, pid: u64, src_port: u16) -> bool {
        self.process_ports.contains_key(&(pid, src_port))
    }

    pub fn add_port_mapping(&mut self, pid: u64, src_port: u16) {
        self.process_ports.insert((pid, src_port), 0);
    }

    pub fn clear_pending_accept(&mut self, pid: u64, src_port: u16) {
        self.pending_accepts.remove(&(pid, src_port));
    }

    pub fn set_pending_accept(&mut self, pid: u64, src_port: u16) {
        self.pending_accepts.insert((pid, src_port), true);
    }

    pub fn check_for_incoming_data(&mut self) -> Vec<(u64, u16, Vec<u8>)> {
        let mut messages = Vec::new();
        let mut to_remove = Vec::new();

        for (consensus_port, entry) in &mut self.port_mappings {
            let mut buf = [0u8; 1024];
            match entry.connection.read(&mut buf) {
                Ok(0) => {
                    info!("Connection closed by remote for {}:{}", entry.process_id, entry.process_port);
                    to_remove.push(*consensus_port);
                }
                Ok(n) => {
                    // Check if this is a connection or listener
                    let is_connection = self.connections.contains_key(&(entry.process_id, entry.process_port));
                    let is_listener = self.listeners.contains_key(consensus_port);
                    
                    if is_connection {
                        info!("Received {} bytes from connection for process {}:{}: {:?}", 
                             n, entry.process_id, entry.process_port, String::from_utf8_lossy(&buf[..n]));
                        messages.push((
                            entry.process_id,
                            entry.process_port,
                            buf[..n].to_vec()
                        ));
                    } else if is_listener {
                        info!("Received {} bytes from listener for process {}:{}: {:?}", 
                             n, entry.process_id, entry.process_port, String::from_utf8_lossy(&buf[..n]));
                        messages.push((
                            entry.process_id,
                            entry.process_port,
                            buf[..n].to_vec()
                        ));
                    } else {
                        error!("Received data on unknown socket type for {}:{}", 
                            entry.process_id, entry.process_port);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
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
                // Remove from appropriate mapping
                if self.connections.contains_key(&(entry.process_id, entry.process_port)) {
                    self.connections.remove(&(entry.process_id, entry.process_port));
                } else if self.listeners.contains_key(&port) {
                    self.process_ports.remove(&(entry.process_id, entry.process_port));
                    self.listeners.remove(&port);
                }
                info!("Removed NAT entry for {}:{}", entry.process_id, entry.process_port);
            }
        }

        messages
    }
} 