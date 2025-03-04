use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::{Arc, Mutex};

/// A simple NAT module for consensus that maps process IDs (destinations)
/// to client connections.
pub struct Nat {
    /// Mapping from process ID to the client TcpStream.
    pub mapping: HashMap<u64, Arc<Mutex<TcpStream>>>,
}

impl Nat {
    /// Create a new NAT mapping.
    pub fn new() -> Self {
        Self {
            mapping: HashMap::new(),
        }
    }

    /// Register a client connection for the given process ID.
    pub fn register(&mut self, pid: u64, stream: Arc<Mutex<TcpStream>>) {
        self.mapping.insert(pid, stream);
    }

    /// Retrieve the client connection for a given process ID.
    pub fn get_client(&self, pid: u64) -> Option<Arc<Mutex<TcpStream>>> {
        self.mapping.get(&pid).cloned()
    }
}
