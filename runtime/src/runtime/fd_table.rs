use std::fmt;

pub struct FDEntry {
    pub buffer: Vec<u8>,    // data waiting to be read
    pub read_ptr: usize,    // how far we've read from buffer
    // Possibly flags, like "n_new" bits or capacity, etc.
}

impl fmt::Display for FDEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // You might want to show the buffer as a string if it is UTF-8,
        // or as hex otherwise.
        let buffer_str = match std::str::from_utf8(&self.buffer) {
            Ok(s) => s.to_string(),
            Err(_) => format!("{:?}", self.buffer),
        };
        write!(f, "FDEntry(buffer: \"{}\", read_ptr: {})", buffer_str, self.read_ptr)
    }
}

pub const MAX_FDS: usize = 8; // or bigger if needed

pub struct FDTable {
    pub entries: [Option<FDEntry>; MAX_FDS],
}

impl FDTable {
    pub fn new() -> Self {
        // Initialize FDTable with all entries = None
        FDTable {
            entries: Default::default(),
        }
    }

    pub fn has_pending_input(&self, fd: i32) -> bool {
        if let Some(Some(entry)) = self.entries.get(fd as usize) {
            entry.read_ptr < entry.buffer.len()
        } else {
            false
        }
    }

    /// Helper to get a mutable reference to the FD entry or return an error.
    pub fn get_fd_entry_mut(&mut self, fd: i32) -> Option<&mut FDEntry> {
        if fd < 0 || fd as usize >= MAX_FDS {
            return None;
        }
        self.entries[fd as usize].as_mut()
    }

    pub fn allocate_fd(&mut self) -> i32 {
        for i in 0..MAX_FDS {
            if self.entries[i].is_none() {
                // We'll fill it later in the actual open call
                return i as i32;
            }
        }
        -1 // no free FD
    }

    /// Mark an FD slot as closed
    pub fn deallocate_fd(&mut self, fd: i32) {
        if fd >= 0 && (fd as usize) < MAX_FDS {
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
