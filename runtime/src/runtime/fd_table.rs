/// A single file descriptor entry
pub struct FDEntry {
    pub buffer: Vec<u8>,    // data waiting to be read
    pub read_ptr: usize,    // how far we've read from buffer
    // Possibly flags, like "n_new" bits or capacity, etc.
}

/// A table of file descriptors for one process
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
    /// Helper to get a mutable reference to the FD entry or return an error
    pub fn get_fd_entry_mut(&mut self, fd: i32) -> Option<&mut FDEntry> {
        if fd < 0 || fd as usize >= MAX_FDS {
            return None;
        }
        self.entries[fd as usize].as_mut()
    }
}

const MAX_FDS: usize = 8; // or bigger if needed

