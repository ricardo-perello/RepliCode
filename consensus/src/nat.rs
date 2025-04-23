use std::collections::HashMap;
use std::net::{TcpStream, SocketAddr};
use std::io::{Write, Read};
use log::{info, error};
use crate::commands::NetworkOperation;

struct NatEntry {
    process_id: u64,
    process_port: u16,
    consensus_port: u16,
    connection: TcpStream,
}

pub struct NatTable {
    port_mappings: HashMap<u16, NatEntry>, // consensus_port -> entry
    process_ports: HashMap<(u64, u16), u16>, // (pid, process_port) -> consensus_port
    next_port: u16,
}

impl NatTable {
    pub fn new() -> Self {
        NatTable {
            port_mappings: HashMap::new(),
            process_ports: HashMap::new(),
            next_port: 10000, // Start from a high port number
        }
    }

    fn allocate_port(&mut self) -> u16 {
        let port = self.next_port;
        self.next_port += 1;
        port
    }

    pub fn handle_network_operation(&mut self, pid: u64, op: NetworkOperation) -> Result<(), Box<dyn std::error::Error>> {
        match op {
            NetworkOperation::Connect { dest_addr, dest_port, src_port } => {
                let consensus_port = self.allocate_port();
                let addr = format!("{}:{}", dest_addr, dest_port);
                
                match TcpStream::connect(&addr) {
                    Ok(stream) => {
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
                if let Some(&consensus_port) = self.process_ports.get(&(pid, src_port)) {
                    if let Some(entry) = self.port_mappings.get_mut(&consensus_port) {
                        if let Err(e) = entry.connection.write_all(&data) {
                            error!("Failed to send data to {}:{}: {}", pid, src_port, e);
                            return Err(Box::new(e));
                        }
                        info!("Sent {} bytes to {}:{}", data.len(), pid, src_port);
                    }
                }
            }
            NetworkOperation::Close { src_port } => {
                if let Some(&consensus_port) = self.process_ports.get(&(pid, src_port)) {
                    self.port_mappings.remove(&consensus_port);
                    self.process_ports.remove(&(pid, src_port));
                    info!("Closed NAT entry for {}:{}", pid, src_port);
                }
            }
        }
        Ok(())
    }

    pub fn check_for_incoming_data(&mut self) -> Vec<(u64, u16, Vec<u8>)> {
        let mut messages = Vec::new();
        let mut to_remove = Vec::new();

        for (consensus_port, entry) in &mut self.port_mappings {
            let mut buf = [0u8; 1024];
            match entry.connection.read(&mut buf) {
                Ok(0) => {
                    // Connection closed by remote
                    to_remove.push(*consensus_port);
                }
                Ok(n) => {
                    messages.push((
                        entry.process_id,
                        entry.process_port,
                        buf[..n].to_vec()
                    ));
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No data available
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