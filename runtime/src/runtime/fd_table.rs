use std::fmt;
use std::path::PathBuf;

use log::debug;

#[derive(Debug, Clone)]
pub enum FDEntry {
    File {
        buffer: Vec<u8>,    // data waiting to be read
        read_ptr: usize,    // how far we've read from buffer
        is_directory: bool,
        is_preopen: bool,
        host_path: Option<String>, // the actual host filesystem path
    },
    Socket {
        local_port: u16,
        connected: bool,
        is_listener: bool,  // whether this is a listening socket
        buffer: Vec<u8>,    // data waiting to be read
    },
}

impl fmt::Display for FDEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FDEntry::File { buffer, read_ptr, is_directory, is_preopen, host_path } => {
                let buffer_str = match std::str::from_utf8(&buffer) {
                    Ok(s) => s.to_string(),
                    Err(_) => format!("{:?}", buffer),
                };
                write!(
                    f,
                    "FDEntry(buffer: \"{}\", read_ptr: {}, is_dir={}, is_preopen={}, host_path={:?})",
                    buffer_str, read_ptr, is_directory, is_preopen, host_path
                )
            },
            FDEntry::Socket { local_port, connected, is_listener, buffer } => {
                let buffer_str = match std::str::from_utf8(&buffer) {
                    Ok(s) => s.to_string(),
                    Err(_) => format!("{:?}", buffer),
                };
                write!(f, "Socket(local_port: {}, connected: {}, is_listener: {}, buffer: \"{}\")", 
                       local_port, connected, is_listener, buffer_str)
            },
        }
    }
}

impl FDEntry {
    pub fn new_file(host_path: Option<String>) -> Self {
        FDEntry::File {
            buffer: Vec::new(),
            read_ptr: 0,
            is_directory: false,
            is_preopen: false,
            host_path,
        }
    }

    pub fn new_directory(host_path: String) -> Self {
        FDEntry::File {
            buffer: Vec::new(),
            read_ptr: 0,
            is_directory: true,
            is_preopen: true,
            host_path: Some(host_path),
        }
    }
}

pub struct FDTable {
    pub entries: Vec<Option<FDEntry>>,
}

impl FDTable {
    pub fn new(process_root: PathBuf) -> Self {
        let mut table = FDTable {
            entries: Vec::with_capacity(32), // Start with capacity for 32 entries
        };
        
        // Initialize standard file descriptors (stdin, stdout, stderr)
        table.entries.push(Some(FDEntry::File {  // stdin
            buffer: Vec::new(),
            read_ptr: 0,
            is_directory: false,
            is_preopen: false,
            host_path: None,
        }));
        table.entries.push(Some(FDEntry::File {  // stdout
            buffer: Vec::new(),
            read_ptr: 0,
            is_directory: false,
            is_preopen: false,
            host_path: None,
        }));
        table.entries.push(Some(FDEntry::File {  // stderr
            buffer: Vec::new(),
            read_ptr: 0,
            is_directory: false,
            is_preopen: false,
            host_path: None,
        }));
        table.entries.push(Some(FDEntry::File {
            buffer: Vec::new(),
            read_ptr: 0,
            is_directory: true,
            is_preopen: true,
            host_path: Some(process_root.to_string_lossy().into_owned()),
        }));
        table
    }

    pub fn has_pending_input(&self, fd: i32) -> bool {
        debug!("Checking FD {} for pending input", fd);
        if let Some(Some(entry)) = self.entries.get(fd as usize) {
            match entry {
                FDEntry::File { buffer, read_ptr, .. } => *read_ptr < buffer.len(),
                FDEntry::Socket { buffer, .. } => !buffer.is_empty(),
            }
        } else {
            false
        }
    }

    /// Helper to get a mutable reference to the FD entry or return an error.
    pub fn get_fd_entry_mut(&mut self, fd: i32) -> Option<&mut FDEntry> {
        if fd < 0 {
            return None;
        }
        self.entries.get_mut(fd as usize).and_then(|e| e.as_mut())
    }

    pub fn allocate_fd(&mut self) -> i32 {
        // First try to find an existing empty slot
        for (i, entry) in self.entries.iter().enumerate() {
            if entry.is_none() {
                return i as i32;
            }
        }
        
        // If no empty slots, grow the vector and return the new index
        let new_fd = self.entries.len() as i32;
        self.entries.push(None);
        new_fd
    }

    /// Mark an FD slot as closed
    pub fn deallocate_fd(&mut self, fd: i32) {
        if fd >= 0 && (fd as usize) < self.entries.len() {
            self.entries[fd as usize] = None;
        }
    }
}

impl fmt::Display for FDTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, entry) in self.entries.iter().enumerate() {
            match entry {
                Some(e) => writeln!(f, "FD {}: {}", i, e)?,
                None => writeln!(f, "FD {}: None", i)?,
            }
        }
        Ok(())
    }
}
