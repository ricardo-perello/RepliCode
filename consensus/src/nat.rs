use std::collections::HashMap;
use std::net::{TcpStream, TcpListener};
use std::io::{Write, Read};
use log::{info, error, debug};
use crate::commands::NetworkOperation;
use serde_json::json;

#[allow(dead_code)]
pub struct NatEntry {
    pub process_id: u64,
    pub process_port: u16,
    pub consensus_port: u16,
    pub connection: TcpStream,
    pub buffer: Vec<u8>,  // Add buffer for received data
}

#[allow(dead_code)]
pub struct NatListener {
    pub process_id: u64,
    pub process_port: u16,
    pub consensus_port: u16,
    pub listener: TcpListener,
    pub pending_accepts: Vec<TcpStream>,
}

pub struct NatTable {
    port_mappings: HashMap<u16, NatEntry>, // consensus_port -> entry
    process_ports: HashMap<(u64, u16), u16>, // (pid, process_port) -> consensus_port
    listeners: HashMap<(u64, u16), NatListener>, // (pid, process_port) -> listener
    connections: HashMap<(u64, u16), u16>, // (pid, process_port) -> connection_consensus_port
    next_port: u16,
    waiting_accepts: HashMap<(u64, u16), u16>, // (pid, src_port) -> requested new_port
    waiting_recvs: HashMap<(u64, u16), bool>, // (pid, src_port) -> is_waiting
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
            waiting_accepts: HashMap::new(),
            waiting_recvs: HashMap::new(),
        }
    }

    fn allocate_port(&mut self) -> u16 {
        let port = self.next_port;
        self.next_port += 1;
        debug!("Allocated new NAT port: {}", port);
        port
    }

    pub fn handle_network_operation(
        &mut self,
        pid: u64,
        op: NetworkOperation,
        messages: &mut Vec<(u64, u16, Vec<u8>, bool)>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let _start_time = std::time::Instant::now();
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
                            pending_accepts: Vec::new(),
                        };
                        
                        self.listeners.insert((pid, src_port), entry);
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
                // First check if we have a listener
                if !self.listeners.contains_key(&(pid, src_port)) {
                    error!("No NAT mapping found for process {}:{}", pid, src_port);
                    return Ok(false);
                }

                // Try to accept any pending connections
                let accept_result = {
                    let listener = self.listeners.get_mut(&(pid, src_port)).unwrap();
                    listener.listener.accept()
                };

                match accept_result {
                    Ok((stream, addr)) => {
                        debug!("Accepted connection from {} on {}:{} -> new port {} (listener: {})", 
                            addr, pid, src_port, new_port, self.listeners.get(&(pid, src_port)).unwrap().consensus_port);
                        
                        // Set non-blocking mode
                        if let Err(e) = stream.set_nonblocking(true) {
                            error!("Failed to set non-blocking mode: {}", e);
                        }

                        // Create a new NAT entry for the accepted connection
                        let consensus_port = self.allocate_port();
                        let entry = NatEntry {
                            process_id: pid,
                            process_port: new_port,  // Use the new_port from the runtime
                            consensus_port,
                            connection: stream,
                            buffer: Vec::new(),
                        };
                        
                        // Add the new connection to our tables
                        self.port_mappings.insert(consensus_port, entry);
                        self.process_ports.insert((pid, new_port), consensus_port);
                        self.connections.insert((pid, new_port), consensus_port);
                        
                        info!("Created NAT entry for accepted connection: {}:{} -> consensus:{}", 
                            pid, new_port, consensus_port);
                        
                        // Clear waiting state since we have a connection
                        self.waiting_accepts.remove(&(pid, src_port));
                        Ok(true)
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        // No connection available, set waiting state with the requested port
                        self.set_waiting_accept(pid, src_port, new_port);
                        debug!("No connection available for {}:{}, process will wait for port {}", 
                            pid, src_port, new_port);
                        Ok(true) // Return true to indicate this is a valid waiting state
                    }
                    Err(e) => {
                        error!("Error accepting connection: {}", e);
                        Err(Box::new(e))
                    }
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
                            buffer: Vec::new(),
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
                let start_time = std::time::Instant::now();
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
                                info!("Send operation completed in {:?} with {} bytes", 
                                     start_time.elapsed(), data.len());
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
            NetworkOperation::Recv { src_port } => {
                let start_time = std::time::Instant::now();
                // Only check the buffer, do not read from the socket here
                if let Some(&consensus_port) = self.connections.get(&(pid, src_port)) {
                    if let Some(entry) = self.port_mappings.get_mut(&consensus_port) {
                        if !entry.buffer.is_empty() {
                            // Data is available in the buffer
                            let data = entry.buffer.clone();
                            entry.buffer.clear();
                            self.waiting_recvs.remove(&(pid, src_port));
                            info!("Recv operation completed in {:?} with {} bytes", 
                                 start_time.elapsed(), data.len());
                            messages.push((pid, src_port, data, false));
                            Ok(true)
                        } else {
                            // No data available, mark as waiting
                            self.waiting_recvs.insert((pid, src_port), true);
                            debug!("No buffered data for {}:{}, process will wait", pid, src_port);
                            Ok(true)
                        }
                    } else {
                        error!("No connection entry found for consensus port {}", consensus_port);
                        Ok(false)
                    }
                } else {
                    error!("No connection found for process {}:{}", pid, src_port);
                    Ok(false)
                }
            }
            NetworkOperation::Close { src_port } => {
                debug!("Processing close operation for process {}:{}", pid, src_port);
                
                // First check if this is a connection
                if let Some(&consensus_port) = self.connections.get(&(pid, src_port)) {
                    if let Some(entry) = self.port_mappings.get_mut(&consensus_port) {
                        // Shutdown the socket
                        if let Err(e) = entry.connection.shutdown(std::net::Shutdown::Both) {
                            error!("Failed to shutdown socket: {}", e);
                        }
                    }
                    self.port_mappings.remove(&consensus_port);
                    self.connections.remove(&(pid, src_port));
                    info!("Closed connection for {}:{}", pid, src_port);
                    Ok(true)
                }
                // If not a connection, check if it's a listener
                else if let Some(&consensus_port) = self.process_ports.get(&(pid, src_port)) {
                    if let Some(entry) = self.port_mappings.get_mut(&consensus_port) {
                        // Shutdown the socket
                        if let Err(e) = entry.connection.shutdown(std::net::Shutdown::Both) {
                            error!("Failed to shutdown socket: {}", e);
                        }
                    }
                    self.port_mappings.remove(&consensus_port);
                    self.process_ports.remove(&(pid, src_port));
                    self.listeners.remove(&(pid, src_port));
                    info!("Closed listener for {}:{}", pid, src_port);
                    Ok(true)
                } else {
                    error!("No NAT mapping found for process {}:{}", pid, src_port);
                    Ok(false)
                }
            }
        }
    }

    pub fn is_waiting_for_accept(&self, pid: u64, src_port: u16) -> bool {
        self.waiting_accepts.contains_key(&(pid, src_port))
    }

    pub fn is_waiting_for_recv(&self, pid: u64, src_port: u16) -> bool {
        self.waiting_recvs.get(&(pid, src_port)).copied().unwrap_or(false)
    }

    pub fn set_waiting_accept(&mut self, pid: u64, src_port: u16, new_port: u16) {
        self.waiting_accepts.insert((pid, src_port), new_port);
        debug!("Process {}:{} is now waiting for accept on port {}", pid, src_port, new_port);
    }

    #[allow(dead_code)]
    pub fn set_waiting_recv(&mut self, pid: u64, src_port: u16) {
        self.waiting_recvs.insert((pid, src_port), true);
        debug!("Process {}:{} is now waiting for recv", pid, src_port);
    }

    pub fn clear_waiting_accept(&mut self, pid: u64, src_port: u16) {
        self.waiting_accepts.remove(&(pid, src_port));
        debug!("Process {}:{} is no longer waiting for accept", pid, src_port);
    }

    #[allow(dead_code)]
    pub fn process_pending_accept(&mut self, pid: u64, src_port: u16) -> bool {
        debug!("Processing pending accept for process {}:{}", pid, src_port);
        
        // Get the pending connection if any
        let pending_connection = {
            if let Some(listener) = self.listeners.get_mut(&(pid, src_port)) {
                debug!("Found listener for {}:{} with {} pending accepts", 
                    pid, src_port, listener.pending_accepts.len());
                listener.pending_accepts.pop()
            } else {
                debug!("No listener found for {}:{}", pid, src_port);
                None
            }
        };

        // If we have a pending connection, create the NAT entry
        if let Some(stream) = pending_connection {
            let consensus_port = self.allocate_port();
            debug!("Allocated consensus port {} for connection from {}:{}", 
                consensus_port, pid, src_port);
            
            let entry = NatEntry {
                process_id: pid,
                process_port: src_port,
                consensus_port,
                connection: stream,
                buffer: Vec::new(),
            };
            
            self.port_mappings.insert(consensus_port, entry);
            self.connections.insert((pid, src_port), consensus_port);
            info!("Created NAT entry for connection from {}:{} on consensus port {}", 
                pid, src_port, consensus_port);
            true
        } else {
            debug!("No pending connection found for {}:{}", pid, src_port);
            false
        }
    }

    #[allow(dead_code)]
    pub fn clear_waiting_recv(&mut self, pid: u64, src_port: u16) {
        self.waiting_recvs.remove(&(pid, src_port));
        debug!("Process {}:{} is no longer waiting for recv", pid, src_port);
    }

    #[allow(dead_code)]
    pub fn has_pending_accept(&self, pid: u64, src_port: u16) -> bool {
        if let Some(listener) = self.listeners.get(&(pid, src_port)) {
            !listener.pending_accepts.is_empty()
        } else {
            false
        }
    }

    #[allow(dead_code)]
    pub fn has_port_mapping(&self, pid: u64, src_port: u16) -> bool {
        self.process_ports.contains_key(&(pid, src_port))
    }

    #[allow(dead_code)]
    pub fn add_port_mapping(&mut self, pid: u64, src_port: u16) {
        let consensus_port = self.next_port;
        self.next_port += 1;
        self.process_ports.insert((pid, src_port), consensus_port);
        debug!("Added port mapping: {}:{} -> consensus:{}", pid, src_port, consensus_port);
    }

    pub fn check_for_incoming_data(&mut self) -> Vec<(u64, u16, Vec<u8>, bool)> {
        let mut messages = Vec::new();
        let mut to_remove = Vec::new();
        let start_time = std::time::Instant::now();

        // First check all listeners for new connections
        let waiting_listeners: Vec<(u64, u16)> = self.listeners.keys()
            .filter(|(pid, src_port)| self.is_waiting_for_accept(*pid, *src_port))
            .cloned()
            .collect();

        // First collect all waiting recv operations
        let waiting_recvs: Vec<(u64, u16)> = self.connections.keys()
            .filter(|(pid, src_port)| self.is_waiting_for_recv(*pid, *src_port))
            .cloned()
            .collect();

        // Then check which of these have closed connections
        for (pid, src_port) in waiting_recvs {
            if let Some(&consensus_port) = self.connections.get(&(pid, src_port)) {
                if self.port_mappings.get_mut(&consensus_port).is_none() {
                    // No entry found, treat as closed
                    debug!("Adding status 0 for missing connection with waiting recv operation {}:{}", pid, src_port);
                    messages.push((pid, src_port, vec![0], false));
                    self.waiting_recvs.remove(&(pid, src_port));
                }
                // Otherwise, do nothing: let the main read loop handle data and closure
            } else {
                // No connection found, treat as closed
                debug!("Adding status 0 for missing connection with waiting recv operation {}:{}", pid, src_port);
                messages.push((pid, src_port, vec![0], false));
                self.waiting_recvs.remove(&(pid, src_port));
            }
        }

        for (pid, src_port) in waiting_listeners {
            if let Some(listener) = self.listeners.get_mut(&(pid, src_port)) {
                debug!("Attempting to accept connection on listener {}:{} (consensus port: {})", 
                    pid, src_port, listener.consensus_port);
                match listener.listener.accept() {
                    Ok((stream, addr)) => {
                        debug!("Accepted connection from {} on {}:{} (listener: {})", 
                            addr, pid, src_port, listener.consensus_port);
                        
                        // Set non-blocking mode
                        if let Err(e) = stream.set_nonblocking(true) {
                            error!("Failed to set non-blocking mode: {}", e);
                        }

                        // Get the requested port from waiting_accepts without removing it
                        let new_port = match self.peek_waiting_port(pid, src_port) {
                            Some(port) => port,
                            None => {
                                error!("No waiting accept entry for {}:{}", pid, src_port);
                                continue;
                            }
                        };

                        // Create a new NAT entry for the accepted connection
                        let consensus_port = self.allocate_port();
                        let entry = NatEntry {
                            process_id: pid,
                            process_port: new_port,  // Use the stored requested port
                            consensus_port,
                            connection: stream,
                            buffer: Vec::new(),
                        };
                        
                        // Add the new connection to our tables
                        self.port_mappings.insert(consensus_port, entry);
                        self.process_ports.insert((pid, new_port), consensus_port);
                        self.connections.insert((pid, new_port), consensus_port);
                        
                        info!("Created NAT entry for accepted connection: {}:{} -> consensus:{}", 
                            pid, new_port, consensus_port);

                        // Notify runtime about the new connection
                        debug!("Adding connection notification to messages queue for {}:{}, {}:{}", pid, src_port, pid, new_port);
                        messages.push((pid, src_port, Vec::new(), true));
                        debug!("Added connection notification to messages queue");
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        debug!("No connection available for {}:{} (WouldBlock)", pid, src_port);
                        continue;
                    }
                    Err(e) => {
                        error!("Error accepting connection on {}:{}: {}", pid, src_port, e);
                    }
                }
            }
        }

        // Then check all connections for incoming data
        for (consensus_port, entry) in &mut self.port_mappings {
            let mut buf = [0u8; 1024];
            match entry.connection.read(&mut buf) {
                Ok(0) => {
                    info!("Connection closed by remote for {}:{}", entry.process_id, entry.process_port);
                    to_remove.push(*consensus_port);
                }
                Ok(n) => {
                    // Always append received data to the buffer
                    entry.buffer.extend_from_slice(&buf[..n]);
                    // Only push to messages if this process is waiting for recv
                    let is_waiting = self.waiting_recvs.contains_key(&(entry.process_id, entry.process_port));
                    if is_waiting {
                        info!("Delivered {} bytes to process {}:{} in {:?}", 
                             entry.buffer.len(), entry.process_id, entry.process_port, start_time.elapsed());
                        messages.push((
                            entry.process_id,
                            entry.process_port,
                            entry.buffer.clone(),
                            false
                        ));
                        entry.buffer.clear();
                        self.waiting_recvs.remove(&(entry.process_id, entry.process_port));
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
                // Check if this was a connection and if it was waiting for recv BEFORE removing it
                let was_connection = self.connections.contains_key(&(entry.process_id, entry.process_port));
                let was_waiting_recv = self.is_waiting_for_recv(entry.process_id, entry.process_port);

                // Remove from appropriate mapping
                if was_connection {
                    self.connections.remove(&(entry.process_id, entry.process_port));
                    debug!("Removed connection mapping for {}:{}", entry.process_id, entry.process_port);
                } else if self.listeners.contains_key(&(entry.process_id, entry.process_port)) {
                    self.process_ports.remove(&(entry.process_id, entry.process_port));
                    self.listeners.remove(&(entry.process_id, entry.process_port));
                    debug!("Removed listener mapping for {}:{}", entry.process_id, entry.process_port);
                }
                info!("Removed NAT entry for {}:{}", entry.process_id, entry.process_port);

                // If this was a connection and it was waiting for recv, send status 0
                if was_connection && was_waiting_recv {
                    debug!("Connection closed while waiting for recv, sending status 0 for {}:{}", 
                        entry.process_id, entry.process_port);
                    messages.push((entry.process_id, entry.process_port, vec![0], false));
                    self.waiting_recvs.remove(&(entry.process_id, entry.process_port));
                }
            }
        }

        messages
    }

    pub fn has_connection(&self, pid: u64, port: u16) -> bool {
        self.connections.contains_key(&(pid, port))
    }

    pub fn get_process_info(&self) -> serde_json::Value {
        let mut processes = HashMap::new();
        
        // Collect all unique process IDs
        for &(pid, _) in self.process_ports.keys() {
            if !processes.contains_key(&pid) {
                let mut ports = Vec::new();
                let mut listeners = Vec::new();
                let mut connections = Vec::new();
                
                // Get all ports for this process
                for &(p, port) in self.process_ports.keys() {
                    if p == pid {
                        ports.push(port);
                        
                        // Check if it's a listener
                        if self.listeners.contains_key(&(pid, port)) {
                            listeners.push(port);
                        }
                        
                        // Check if it's a connection
                        if self.connections.contains_key(&(pid, port)) {
                            connections.push(port);
                        }
                    }
                }
                
                processes.insert(pid, json!({
                    "ports": ports,
                    "listeners": listeners,
                    "connections": connections
                }));
            }
        }
        
        json!(processes)
    }

    pub fn get_connection_info(&self) -> serde_json::Value {
        let mut connections = Vec::new();
        
        for (consensus_port, entry) in &self.port_mappings {
            if self.connections.contains_key(&(entry.process_id, entry.process_port)) {
                connections.push(json!({
                    "process_id": entry.process_id,
                    "process_port": entry.process_port,
                    "consensus_port": consensus_port,
                    "buffer_size": entry.buffer.len()
                }));
            }
        }
        
        json!(connections)
    }

    pub fn get_listener_info(&self) -> serde_json::Value {
        let mut listeners = Vec::new();
        
        for ((pid, port), listener) in &self.listeners {
            listeners.push(json!({
                "process_id": pid,
                "process_port": port,
                "consensus_port": listener.consensus_port,
                "pending_accepts": listener.pending_accepts.len()
            }));
        }
        
        json!(listeners)
    }

    pub fn get_port_mappings(&self) -> Vec<(u64, u16, u16, &'static str)> {
        let mut mappings = Vec::new();
        
        for ((pid, process_port), &consensus_port) in &self.process_ports {
            let mapping_type = if self.listeners.contains_key(&(*pid, *process_port)) {
                "listener"
            } else if self.connections.contains_key(&(*pid, *process_port)) {
                "connection"
            } else {
                "unknown"
            };
            
            mappings.push((*pid, *process_port, consensus_port, mapping_type));
        }
        
        mappings
    }

    pub fn get_waiting_port(&self, pid: u64, src_port: u16) -> Option<u16> {
        self.waiting_accepts.get(&(pid, src_port)).copied()
    }

    pub fn peek_waiting_port(&self, pid: u64, src_port: u16) -> Option<u16> {
        self.waiting_accepts.get(&(pid, src_port)).copied()
    }
} 