use std::fs;
use std::fs::OpenOptions;
use std::io;
use std::path::Path;
use log::{error, debug};
use wasmtime::{Caller, Extern};
use std::io::Write;

use crate::runtime::process::{ProcessData, ProcessState, BlockReason};
use crate::runtime::fd_table::{FDEntry};
const WASI_ERRNO_NOSPC: i32 = 28;  // __WASI_ERRNO_NOSPC
const WASI_ERRNO_NOSYS: i32 = 52;  // __WASI_ERRNO_NOSYS


fn io_err_to_wasi_errno(e: &io::Error) -> i32 {
    use io::ErrorKind::*;
    match e.kind() {
        NotFound => 2,           // e.g. __WASI_ERRNO_NOENT
        PermissionDenied => 13,  // e.g. __WASI_ERRNO_ACCES
        AlreadyExists => 20,     // __WASI_ERRNO_EXIST
        _ => 1,                  // catch-all or __WASI_ERRNO_IO
    }
}

/// If you want to block for file I/O
fn block_process_for_fileio(caller: &mut Caller<'_, ProcessData>) {
    {
        let mut state = caller.data().state.lock().unwrap();
        if *state == ProcessState::Running {
            println!("path_open / fd_readdir: Setting process state to Blocked (FileIO).");
            *state = ProcessState::Blocked;
        }
        let mut reason = caller.data().block_reason.lock().unwrap();
        *reason = Some(BlockReason::FileIO);
        caller.data().cond.notify_all();
    }
    let mut state = caller.data().state.lock().unwrap();
    while *state != ProcessState::Running {
        state = caller.data().cond.wait(state).unwrap();
    }
}

// ----------------------------------------------------------------------------
// Disk-usage tracking support
// ----------------------------------------------------------------------------

/// Increment the process's tracked usage by `bytes`. If the limit is exceeded,
/// forcibly kill the process.
fn usage_add(caller: &mut Caller<'_, ProcessData>, bytes: u64) -> Result<(), i32> {
    // 1) Figure out if we exceed the limit
    let over_limit = {
        // Borrow immutably but only within this block
        let pd = caller.data();  // &ProcessData
        let mut usage = pd.current_disk_usage.lock().unwrap();
        *usage = usage.saturating_add(bytes);

        // Return boolean so we can decide outside
        *usage > pd.max_disk_usage
    }; // Immutable borrow ends here

    // 2) If over the limit, kill the process
    if over_limit {
        eprintln!("Exceeded disk quota! Killing process...");
        return Err(WASI_ERRNO_NOSPC);//TODO return error code
    }

    Ok(())
}


/// Decrement the process's tracked usage by `bytes`. 
fn usage_sub(caller: &mut Caller<'_, ProcessData>, bytes: u64) {
    let pd = caller.data();
    let mut usage = pd.current_disk_usage.lock().unwrap();
    *usage = usage.saturating_sub(bytes);
}

/// If you remove a directory, or some other operation, and need to figure out how many
/// bytes were in that directory, you can do a quick naive walk:
pub fn get_dir_size(path: &Path) -> io::Result<u64> {
    let mut size = 0;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            size += get_dir_size(&entry.path())?;
        } else {
            size += metadata.len();
        }
    }
    Ok(size)
}

// ----------------------------------------------------------------------------
// File/directory ops below
// ----------------------------------------------------------------------------

/// Write a full 64-byte WASI filestat struct based on our FDEntry.
pub fn wasi_fd_filestat_get(
    mut caller: Caller<ProcessData>,
    fd: u32,
    buf_ptr: u32,
) -> anyhow::Result<u32> {
    debug!("wasi_fd_filestat_get: fd={}, buf_ptr={}", fd, buf_ptr);
    
    // Get FD entry
    let (size, filetype) = {
        let process_data = caller.data();
        let table = process_data.fd_table.lock().unwrap();
        debug!("wasi_fd_filestat_get: checking fd {} in table with {} entries", fd, table.entries.len());
        
        if fd as usize >= table.entries.len() {
            debug!("wasi_fd_filestat_get: fd {} out of bounds", fd);
            return Ok(8); // WASI_EBADF
        }
        
        match &table.entries[fd as usize] {
            Some(FDEntry::File { buffer, is_directory, host_path, .. }) => {
                debug!("wasi_fd_filestat_get: found File entry - buffer.len={}, is_dir={}, host_path={:?}", 
                    buffer.len(), is_directory, host_path);
                
                let size = if !buffer.is_empty() {
                    debug!("wasi_fd_filestat_get: using buffer size {}", buffer.len());
                    buffer.len() as u64
                } else {
                    match host_path {
                        Some(path) => {
                            debug!("wasi_fd_filestat_get: buffer empty, trying metadata for {}", path);
                            match std::fs::metadata(path) {
                                Ok(metadata) => {
                                    let size = metadata.len();
                                    debug!("wasi_fd_filestat_get: got metadata size {}", size);
                                    size
                                },
                                Err(e) => {
                                    debug!("wasi_fd_filestat_get: metadata error: {}", e);
                                    return Ok(8); // WASI_EBADF
                                }
                            }
                        },
                        None => {
                            debug!("wasi_fd_filestat_get: no host_path available");
                            0
                        }
                    }
                };
                (size, if *is_directory { 3u8 } else { 4u8 })
            }
            Some(FDEntry::Socket { .. }) => {
                debug!("wasi_fd_filestat_get: found Socket entry");
                (0, 5u8) // Socket type
            }
            None => {
                debug!("wasi_fd_filestat_get: no entry found for fd {}", fd);
                return Ok(8); // WASI_EBADF
            }
        }
    };

    debug!("wasi_fd_filestat_get: computed size={}, filetype={}", size, filetype);

    // Create filestat buffer (64 bytes)
    let mut buf = [0u8; 64];
    
    // st_dev (8 bytes) - set to 0
    buf[0..8].copy_from_slice(&0u64.to_le_bytes());
    
    // st_ino (8 bytes) - set to 0
    buf[8..16].copy_from_slice(&0u64.to_le_bytes());
    
    // st_filetype (1 byte)
    buf[16] = filetype;
    // 17-23: padding (already zero)
    
    // st_nlink (8 bytes)
    buf[24..32].copy_from_slice(&1u64.to_le_bytes());
    
    // st_size (8 bytes)
    buf[32..40].copy_from_slice(&size.to_le_bytes());
    debug!("wasi_fd_filestat_get: writing size {} to buffer at offset 32", size);
    
    // st_atim (8 bytes) - set to 0
    buf[40..48].copy_from_slice(&0u64.to_le_bytes());
    
    // st_mtim (8 bytes) - set to 0
    buf[48..56].copy_from_slice(&0u64.to_le_bytes());
    
    // st_ctim (8 bytes) - set to 0
    buf[56..64].copy_from_slice(&0u64.to_le_bytes());

    // Write to memory
    let memory = caller.get_export("memory").unwrap().into_memory().unwrap();
    let mem = memory.data_mut(&mut caller);
    let ptr = buf_ptr as usize;
    if ptr + 64 > mem.len() {
        debug!("wasi_fd_filestat_get: buffer out of bounds");
        return Ok(21); // WASI_EFAULT
    }
    mem[ptr..ptr+64].copy_from_slice(&buf);
    debug!("wasi_fd_filestat_get: wrote filestat to memory at offset {}", ptr);
    
    Ok(0)
}

pub fn wasi_path_unlink_file(
    mut caller: wasmtime::Caller<'_, ProcessData>,
    _dirfd: i32,
    path_ptr: i32,
    path_len: i32,
) -> i32 {
    use wasmtime::Extern;
    use log::error;

    let memory = match caller.get_export("memory") {
        Some(Extern::Memory(mem)) => mem,
        _ => {
            error!("path_unlink_file: Memory not found");
            return 1;
        }
    };

    let data = memory.data(&caller);
    let start = path_ptr as usize;
    let end = start + (path_len as usize);
    if end > data.len() {
        error!("path_unlink_file: path out of bounds");
        return 1;
    }
    let path_str = match std::str::from_utf8(&data[start..end]) {
        Ok(s) => s,
        Err(_) => {
            error!("path_unlink_file: invalid UTF-8");
            return 1;
        }
    };

    let root_path = caller.data().root_path.clone();
    let joined = root_path.join(path_str.trim_start_matches('/'));
    
    // Canonicalize paths for security check
    let canonical_root = match root_path.canonicalize() {
        Ok(c) => c,
        Err(e) => {
            error!("path_unlink_file: failed to canonicalize root path: {}", e);
            return io_err_to_wasi_errno(&e);
        }
    };
    
    let canonical = match joined.canonicalize() {
        Ok(c) => c,
        Err(e) => {
            error!("path_unlink_file: canonicalize error: {}", e);
            return 2;
        }
    };
    
    if !canonical.starts_with(&canonical_root) {
        error!("path_unlink_file: attempt to escape sandbox root!");
        return 13;
    }

    // NEW: get the file size before removing
    let file_size = match fs::metadata(&canonical) {
        Ok(m) => m.len(),
        Err(e) => {
            error!("path_unlink_file: metadata error: {}", e);
            return io_err_to_wasi_errno(&e);
        }
    };

    // remove the file
    match fs::remove_file(&canonical) {
        Ok(_) => {
            // Decrement usage
            usage_sub(&mut caller, file_size);
            0
        }
        Err(e) => {
            error!("path_unlink_file: failed to unlink: {}", e);
            io_err_to_wasi_errno(&e)
        }
    }
}

pub fn wasi_path_remove_directory(
    mut caller: wasmtime::Caller<'_, ProcessData>,
    _dirfd: i32,
    path_ptr: i32,
    path_len: i32,
) -> i32 {
    use wasmtime::Extern;
    use log::error;

    let memory = match caller.get_export("memory") {
        Some(Extern::Memory(mem)) => mem,
        _ => {
            error!("path_remove_directory: Memory not found");
            return 1;
        }
    };

    let data = memory.data(&caller);
    let start = path_ptr as usize;
    let end = start + (path_len as usize);
    if end > data.len() {
        error!("path_remove_directory: path out of bounds");
        return 1;
    }
    let path_str = match std::str::from_utf8(&data[start..end]) {
        Ok(s) => s,
        Err(_) => {
            error!("path_remove_directory: invalid UTF-8");
            return 1;
        }
    };

    let root_path = caller.data().root_path.clone();
    let joined = root_path.join(path_str.trim_start_matches('/'));
    
    // Canonicalize paths for security check
    let canonical_root = match root_path.canonicalize() {
        Ok(c) => c,
        Err(e) => {
            error!("path_remove_directory: failed to canonicalize root path: {}", e);
            return io_err_to_wasi_errno(&e);
        }
    };
    
    let canonical = match joined.canonicalize() {
        Ok(c) => c,
        Err(e) => {
            error!("path_remove_directory: canonicalize error: {}", e);
            return 2;
        }
    };
    
    if !canonical.starts_with(&canonical_root) {
        error!("path_remove_directory: attempt to escape sandbox root!");
        return 13;
    }

    // NEW: compute how many bytes were in that directory
    let dir_size = match get_dir_size(&canonical) {
        Ok(s) => s,
        Err(e) => {
            error!("path_remove_directory: cannot compute dir size: {}", e);
            return io_err_to_wasi_errno(&e);
        }
    };

    // remove the directory
    match fs::remove_dir(&canonical) {
        Ok(_) => {
            // Decrement usage
            usage_sub(&mut caller, dir_size);
            0
        }
        Err(e) => {
            error!("path_remove_directory: failed: {}", e);
            io_err_to_wasi_errno(&e)
        }
    }
}

pub fn wasi_path_create_directory(
    mut caller: wasmtime::Caller<'_, ProcessData>,
    _dirfd: i32,
    path_ptr: i32,
    path_len: i32,
) -> i32 {
    use wasmtime::Extern;
    use log::error;

    let memory = match caller.get_export("memory") {
        Some(Extern::Memory(mem)) => mem,
        _ => {
            error!("path_create_directory: Memory not found");
            return 1;
        }
    };

    let data = memory.data(&caller);
    let start = path_ptr as usize;
    let end = start + (path_len as usize);
    if end > data.len() {
        error!("path_create_directory: path out of bounds");
        return 1;
    }
    let path_str = match std::str::from_utf8(&data[start..end]) {
        Ok(s) => s,
        Err(_) => {
            error!("path_create_directory: invalid UTF-8");
            return 1;
        }
    };

    let root_path = caller.data().root_path.clone();
    
    // Join the requested path to the root path
    let joined = root_path.join(path_str.trim_start_matches('/'));
    
    // For security check, we need to canonicalize existing paths or ensure joined path is valid
    // First, check if the parent of joined exists and can be canonicalized
    let parent_path = joined.parent().unwrap_or(&joined);
    if parent_path.exists() {
        let canonical_parent = match parent_path.canonicalize() {
            Ok(c) => c,
            Err(e) => {
                error!("path_create_directory: failed to canonicalize parent path: {}", e);
                return io_err_to_wasi_errno(&e);
            }
        };
        
        // Canonicalize the root path
        let canonical_root = match root_path.canonicalize() {
            Ok(c) => c,
            Err(e) => {
                error!("path_create_directory: failed to canonicalize root path: {}", e);
                return io_err_to_wasi_errno(&e);
            }
        };
        
        // Check if the parent is within the sandbox
        if !canonical_parent.starts_with(&canonical_root) {
            error!("path_create_directory: attempt to escape sandbox root. parent path: {:?}, canonical root: {:?}", canonical_parent, canonical_root);
            return 13;
        }
    } else {
        // If parent doesn't exist, we can just do a simple string-based check
        // Convert both to string and check if joined starts with root_path
        let root_str = root_path.to_string_lossy().to_string();
        let joined_str = joined.to_string_lossy().to_string();
        
        if !joined_str.starts_with(&root_str) {
            error!("path_create_directory: attempt to escape sandbox root with non-existent path");
            return 13;
        }
    }

    // At this point, we've determined the path is safe to create
    match fs::create_dir(&joined) {
        Ok(_) => {
            // For a directory, you can count a small overhead. 
            // Or do metadata().len(). Let's do that:
            let dir_metadata_size = match fs::metadata(&joined) {
                Ok(md) => md.len(),
                Err(_) => 4096, // fallback
            };
            if let Err(errno) = usage_add(&mut caller, dir_metadata_size) {
                return errno; // process got killed
            }
            0
        }
        Err(e) => {
            error!("path_create_directory: failed: {}", e);
            io_err_to_wasi_errno(&e)
        }
    }
}

/// Implementation of the symbolic link syscall.
/// Since symlink is not supported, we simply log an error,
/// mark the process as Finished, notify the scheduler,
/// and then loop indefinitely until the scheduler joins the thread.
pub fn wasi_path_symlink(
    caller: Caller<'_, ProcessData>,
    _old_path_ptr: i32,
    _old_path_len: i32,
    _new_dirfd: i32,
    _new_path_ptr: i32,
    _new_path_len: i32,
) -> i32 {
    eprintln!("path_symlink: not yet implemented");
    return WASI_ERRNO_NOSYS;
}


pub fn wasi_fd_close(caller: Caller<'_, ProcessData>, fd: i32) -> i32 {
    println!("fd_close: closing fd {}", fd);
    let process_data = caller.data();
    let mut table = process_data.fd_table.lock().unwrap();
    if fd < 0 || fd as usize >= table.entries.len() {
        eprintln!("fd_close: invalid fd {}", fd);
        return 8; // e.g., WASI_EBADF
    }
    table.deallocate_fd(fd);
    0
}

/// Implementation of WASI's 'path_open'
///   (dirfd, path_ptr, path_len, etc. are per the normal WASI call signature).
///
/// This version ensures that all file operations are restricted to the
/// process's `root_path`.
pub fn wasi_path_open(
    mut caller: Caller<'_, ProcessData>,
    _dirfd: i32,      // not used in this simplified implementation
    _dirflags: i32,   // not used
    path_ptr: i32,
    path_len: i32,
    oflags: i32,
    _fs_rights_base: i64,
    _fs_rights_inheriting: i64,
    _fdflags: i32,
    opened_fd_out: i32,
) -> i32 {
    println!(
        "path_open: oflags={}, opened_fd_out={}",
        oflags, opened_fd_out
    );

    // 1) Extract path string from WASM memory.
    let memory = match caller.get_export("memory") {
        Some(wasmtime::Extern::Memory(mem)) => mem,
        _ => {
            eprintln!("path_open: no memory export found");
            return 1;
        }
    };
    let mem_data = memory.data(&caller);
    let start = path_ptr as usize;
    let end = start + (path_len as usize);
    if end > mem_data.len() {
        eprintln!("path_open: path out of bounds");
        return 1;
    }
    let path_str = match std::str::from_utf8(&mem_data[start..end]) {
        Ok(s) => s.trim(),  // Trim whitespace and newlines
        Err(_) => {
            eprintln!("path_open: invalid UTF-8");
            return 1;
        }
    };
    println!("path_open: requested path: '{}'", path_str);

    // 2) Get sandbox (fake root) from ProcessData.
    let root_path = caller.data().root_path.clone();

    // 3) Join relative path to fake root.
    let joined_path = root_path.join(path_str.trim_start_matches('/'));
    
    // 4) Security check: ensure the path is inside the fake root.
    // Canonicalize the root path
    let canonical_root = match root_path.canonicalize() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("path_open: failed to canonicalize root path: {}", e);
            return io_err_to_wasi_errno(&e);
        }
    };
    
    // If the path exists, canonicalize it for comparison
    let canonical = if joined_path.exists() {
        match joined_path.canonicalize() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("path_open: canonicalize error: {}", e);
                return io_err_to_wasi_errno(&e);
            }
        }
    } else {
        // If the path doesn't exist, check its parent
        let parent = joined_path.parent().unwrap_or(&joined_path);
        if parent.exists() {
            let parent_canonical = match parent.canonicalize() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("path_open: failed to canonicalize parent: {}", e);
                    return io_err_to_wasi_errno(&e);
                }
            };
            
            // Check if parent is inside sandbox
            if !parent_canonical.starts_with(&canonical_root) {
                eprintln!("path_open: attempt to escape sandbox root!");
                return 13;
            }
            
            // Use the joined path for further operations
            joined_path.clone()
        } else {
            // If even parent doesn't exist, do simple string check
            let root_str = root_path.to_string_lossy().to_string();
            let joined_str = joined_path.to_string_lossy().to_string();
            
            if !joined_str.starts_with(&root_str) {
                eprintln!("path_open: attempt to escape sandbox root with non-existent path");
                return 13;
            }
            
            joined_path.clone()
        }
    };
    
    // If we have a canonicalized path, check it
    if canonical.exists() && !canonical.starts_with(&canonical_root) {
        eprintln!("path_open: attempt to escape sandbox root!");
        return 13;
    }

    // 5) Get metadata or create file if it does not exist and O_CREAT is set.
    // Let's assume that O_CREAT is indicated by bit 0x1.
    let o_creat = (oflags & 1) != 0;
    let is_readable = (oflags & 0x1) == 0; // O_RDONLY or O_RDWR
    let is_writable = (oflags & 0x2) != 0; // O_WRONLY or O_RDWR

    let (is_dir, file_data) = match fs::metadata(&canonical) {
        Ok(md) => {
            if md.is_dir() {
                // It's a directory: read directory entries.
                let mut buf = Vec::new();
                match fs::read_dir(&canonical) {
                    Ok(entries) => {
                        for entry_res in entries {
                            if let Ok(dirent) = entry_res {
                                let name = dirent.file_name();
                                let name_str = name.to_string_lossy().into_owned();
                                buf.extend_from_slice(name_str.as_bytes());
                                buf.push(b'\n');
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("path_open: read_dir error: {}", e);
                        return io_err_to_wasi_errno(&e);
                    }
                }
                (true, buf)
            } else {
                // It's a file: read file content if readable
                let file_data = if is_readable {
                    match fs::read(&canonical) {
                        Ok(data) => {
                            debug!("DEBUG: file_data.len() = {}", data.len());
                            debug!("DEBUG: host_path = {:?}", canonical);
                            if data.len() > 1_000_000 {
                                debug!("path_open: File is large => blocking to simulate I/O wait");
                                block_process_for_fileio(&mut caller);
                            }
                            data
                        },
                        Err(e) => {
                            eprintln!("path_open: Failed to read file: {}", e);
                            return io_err_to_wasi_errno(&e);
                        }
                    }
                } else {
                    Vec::new()
                };
                (false, file_data)
            }
        }
        Err(e) => {
            if o_creat {
                // File doesn't exist, and O_CREAT is set: create it.
                match OpenOptions::new()
                    .write(true)
                    .create(true)
                    .open(&canonical)
                {
                    Ok(_f) => {
                        // File is now created (empty).
                        let file_data = if is_readable {
                            fs::read(&canonical).unwrap_or_default()
                        } else {
                            Vec::new()
                        };
                        (false, file_data)
                    }
                    Err(e) => {
                        eprintln!("path_open: Failed to create file: {}", e);
                        return io_err_to_wasi_errno(&e);
                    }
                }
            } else {
                eprintln!("path_open: metadata error: {}", e);
                return io_err_to_wasi_errno(&e);
            }
        }
    };

    // 6) Allocate a new FD and store the buffer.
    let fd = {
        let pd = caller.data();
        let mut table = pd.fd_table.lock().unwrap();
        let fd = table.allocate_fd();
        if fd < 0 {
            eprintln!("path_open: No free FD available!");
            return 76;
        }
        table.entries[fd as usize] = Some(FDEntry::File {
            buffer: file_data,
            read_ptr: 0,
            is_directory: is_dir,
            is_preopen: false,
            host_path: Some(canonical.to_string_lossy().into_owned()),
        });
        fd
    };

    // 7) Write the FD back to WASM memory.
    {
        let mem_mut = memory.data_mut(&mut caller);
        let out_ptr = opened_fd_out as usize;
        if out_ptr + 4 > mem_mut.len() {
            eprintln!("path_open: opened_fd_out out of bounds");
            return 1;
        }
        mem_mut[out_ptr..out_ptr + 4].copy_from_slice(&(fd as u32).to_le_bytes());
    }

    println!("path_open: success, new FD = {}", fd);
    0
}



/// Implementation of WASI's `fd_readdir`.
/// Also ensures that it can't escape the sandbox, though in this simplified
/// approach we treat it as reading from a single FD that was presumably
/// opened within the sandbox already.
pub fn wasi_fd_readdir(
    mut caller: Caller<'_, ProcessData>,
    fd: i32,
    buf: i32,
    buf_len: i32,
    cookie: i64,
    bufused_out: i32,
) -> i32 {
    println!("fd_readdir(fd={}, buf={}, buf_len={}, cookie={})", fd, buf, buf_len, cookie);

    // 1) Grab the data from the FD table in its own scope.
    //    We'll copy it into a local buffer so we don't keep
    //    locking the FD table or referencing caller while writing to memory.
    let (data_to_read, read_ptr_before) = {
        let process_data = caller.data();
        let mut table = process_data.fd_table.lock().unwrap();
        match table.get_fd_entry_mut(fd) {
            Some(FDEntry::File { buffer, read_ptr, .. }) => {
                if *read_ptr >= buffer.len() {
                    println!("fd_readdir: End of directory listing, returning 0 used bytes");
                    (Vec::new(), *read_ptr)
                } else {
                    let slice = &buffer[*read_ptr..];
                    let local_copy = slice.to_vec();
                    (local_copy, *read_ptr)
                }
            }
            _ => (Vec::new(), 0)
        }
    };

    // If the local buffer is empty, we know read_ptr was at the end.
    if data_to_read.is_empty() {
        // Set bufused_out = 0
        return set_bufused(&mut caller, bufused_out, 0);
    }

    // 2) Determine how many bytes to copy
    let n_to_copy = std::cmp::min(data_to_read.len(), buf_len as usize);

    // 3) Write that many bytes into the Wasm memory
    {
        let memory = match caller.get_export("memory") {
            Some(wasmtime::Extern::Memory(mem)) => mem,
            _ => {
                eprintln!("fd_readdir: no memory export found");
                return 1;
            }
        };
        let mem_mut = memory.data_mut(&mut caller);

        let buf_start = buf as usize;
        let buf_end = buf_start + n_to_copy;
        if buf_end > mem_mut.len() {
            eprintln!("fd_readdir: buf out of bounds");
            return 1;
        }
        mem_mut[buf_start..buf_end].copy_from_slice(&data_to_read[..n_to_copy]);
    }

    // 4) Update the read_ptr in FD table in a separate scope
    {
        let process_data = caller.data();
        let mut table = process_data.fd_table.lock().unwrap();
        if let Some(FDEntry::File { read_ptr, .. }) = table.get_fd_entry_mut(fd) {
            *read_ptr = read_ptr_before + n_to_copy;
        }
    }

    // 5) Write how many bytes we used into bufused_out
    set_bufused(&mut caller, bufused_out, n_to_copy as u32)
}


pub fn wasi_fd_write(
    mut caller: wasmtime::Caller<'_, ProcessData>,
    fd: i32,
    iovs: i32,
    iovs_len: i32,
    nwritten: i32,
) -> i32 {
    use std::cmp::min;
    use std::convert::TryInto;
    use std::io::Write;
    
    let memory = match caller.get_export("memory") {
        Some(wasmtime::Extern::Memory(mem)) => mem,
        _ => {
            error!("fd_write: Failed to find memory export");
            return 1;
        }
    };
    
    // Gather data to write.
    let data_to_write = {
        let data = memory.data(&caller);
        let mut buf = Vec::new();
        for i in 0..iovs_len {
            let iovec_addr = (iovs as usize) + (i as usize) * 8;
            if iovec_addr + 8 > data.len() {
                error!("fd_write: iovec out of bounds");
                return 1;
            }
            let offset_bytes: [u8; 4] = data[iovec_addr..iovec_addr + 4].try_into().unwrap();
            let len_bytes: [u8; 4] = data[iovec_addr + 4..iovec_addr + 8].try_into().unwrap();
            let offset = u32::from_le_bytes(offset_bytes) as usize;
            let len = u32::from_le_bytes(len_bytes) as usize;
            if offset + len > data.len() {
                error!("fd_write: data slice out of bounds");
                return 1;
            }
            buf.extend_from_slice(&data[offset..offset + len]);
        }
        buf
    };
    
    let total_written = if fd == 1 {
        // Handle stdout.
        io::stdout()
            .write_all(&data_to_write)
            .map(|_| data_to_write.len())
            .map_err(|e| io_err_to_wasi_errno(&e))
    } else if fd == 2 {
        // Handle stderr.
        io::stderr()
            .write_all(&data_to_write)
            .map(|_| data_to_write.len())
            .map_err(|e| io_err_to_wasi_errno(&e))
    } else {
        // For sandbox file writes, look up the host path.
        let host_path_opt = {
            let pd = caller.data();
            let table = pd.fd_table.lock().unwrap();
            match table.entries.get(fd as usize) {
                Some(Some(FDEntry::File { host_path, is_directory, .. })) if host_path.is_some() && !is_directory => {
                    host_path.clone()
                }
                _ => None,
            }
        };
    
        if let Some(host_path) = host_path_opt {
            // Account for the total bytes.
            if let Err(errno) = usage_add(&mut caller, data_to_write.len() as u64) {
                return errno;
            }
            let total = data_to_write.len();
            let mut offset = 0;
            while offset < total {
                // Check free capacity.
                let available = {
                    let write_buf = caller.data().write_buffer.lock().unwrap();
                    caller.data().max_write_buffer.saturating_sub(write_buf.len())
                };
    
                if available == 0 {
                    // Buffer is full and there is still data to write.
                    {
                        let mut state = caller.data().state.lock().unwrap();
                        *state = ProcessState::Blocked;
                    }
                    {
                        let mut reason = caller.data().block_reason.lock().unwrap();
                        // Save the host path in the block reason.
                        *reason = Some(BlockReason::WriteIO(host_path.clone()));
                    }
                    caller.data().cond.notify_all();
                    {
                        let mut state = caller.data().state.lock().unwrap();
                        while *state != ProcessState::Running {
                            state = caller.data().cond.wait(state).unwrap();
                        }
                    }
                    // Once unblocked (scheduler should flush), continue the loop.
                    continue;
                } else {
                    let chunk = min(available, total - offset);
                    {
                        let mut write_buf = caller.data().write_buffer.lock().unwrap();
                        write_buf.extend_from_slice(&data_to_write[offset..offset + chunk]);
                    }
                    offset += chunk;
                    // After appending, if the buffer is full:
                    let current_size = { caller.data().write_buffer.lock().unwrap().len() };
                    if current_size == caller.data().max_write_buffer {
                        if offset < total {
                            // Buffer full with more data pending: block.
                            {
                                let mut state = caller.data().state.lock().unwrap();
                                *state = ProcessState::Blocked;
                            }
                            {
                                let mut reason = caller.data().block_reason.lock().unwrap();
                                *reason = Some(BlockReason::WriteIO(host_path.clone()));
                            }
                            caller.data().cond.notify_all();
                            {
                                let mut state = caller.data().state.lock().unwrap();
                                while *state != ProcessState::Running {
                                    state = caller.data().cond.wait(state).unwrap();
                                }
                            }
                            continue;
                        } else {
                            // Buffer full but no data remains: flush immediately.
                            if let Err(errno) = flush_write_buffer(&mut caller, &host_path) {
                                return errno;
                            }
                        }
                    }
                }
            }
            // Flush any remaining data.
            if !caller.data().write_buffer.lock().unwrap().is_empty() {
                if let Err(errno) = flush_write_buffer(&mut caller, &host_path) {
                    return errno;
                }
            }
            Ok(total)
        } else {
            error!("fd_write: unsupported fd: {}", fd);
            Err(1)
        }
    };
    
    let bytes_written = match total_written {
        Ok(n) => n,
        Err(errno) => return errno,
    };
    
    // Write the number of bytes written into WASM memory.
    {
        let total_written_bytes = (bytes_written as u32).to_le_bytes();
        let nwritten_ptr = nwritten as usize;
        let mem_mut = memory.data_mut(&mut caller);
        if nwritten_ptr + 4 > mem_mut.len() {
            error!("fd_write: nwritten pointer out of bounds");
            return 1;
        }
        mem_mut[nwritten_ptr..nwritten_ptr + 4].copy_from_slice(&total_written_bytes);
    }
    0
}


/// Flush the process write buffer to the file at `host_path`.
/// This writes out the entire buffer and then clears it.
fn flush_write_buffer(
    caller: &mut Caller<'_, ProcessData>,
    host_path: &str,
) -> Result<usize, i32> {
    let mut buf = caller.data().write_buffer.lock().unwrap();
    if buf.is_empty() {
        return Ok(0);
    }
    match OpenOptions::new().append(true).open(host_path) {
        Ok(mut file) => {
            if let Err(e) = file.write_all(&buf) {
                error!("flush_write_buffer: failed to write to file {}: {}", host_path, e);
                return Err(io_err_to_wasi_errno(&e));
            }
            let bytes = buf.len();
            buf.clear();
            Ok(bytes)
        }
        Err(e) => {
            error!("flush_write_buffer: failed to open file {}: {}", host_path, e);
            Err(io_err_to_wasi_errno(&e))
        }
    }
}


/// flush_write_buffer_for_scheduler flushes all data currently stored in
/// the process's write buffer (data is stored in an Arc<Mutex<Vec<u8>>> within ProcessData)
/// by appending it to the file at the given host_path. It then clears the buffer.
/// Returns the number of bytes flushed, or an errno on failure.
pub fn flush_write_buffer_for_scheduler(
    data: &ProcessData,
    host_path: &str,
) -> Result<usize, i32> {
    let mut buf = data.write_buffer.lock().unwrap();
    if buf.is_empty() {
        return Ok(0);
    }
    match OpenOptions::new().append(true).open(host_path) {
        Ok(mut file) => {
            if let Err(e) = file.write_all(&buf) {
                error!("flush_write_buffer_for_scheduler: failed to write to file {}: {}", host_path, e);
                return Err(io_err_to_wasi_errno(&e));
            }
            let bytes = buf.len();
            buf.clear();
            Ok(bytes)
        }
        Err(e) => {
            error!("flush_write_buffer_for_scheduler: failed to open file {}: {}", host_path, e);
            Err(io_err_to_wasi_errno(&e))
        }
    }
}


pub fn wasi_file_create(
    mut caller: Caller<'_, ProcessData>,
    path_ptr: i32,
    path_len: i32,
    opened_fd_out: i32,
) -> i32 {
    let memory = match caller.get_export("memory") {
        Some(Extern::Memory(mem)) => mem,
        _ => {
            error!("file_create: No memory export found");
            return 1;
        }
    };

    let mem_data = memory.data(&caller);
    let start = path_ptr as usize;
    let end = start + (path_len as usize);
    if end > mem_data.len() {
        error!("file_create: path out of bounds");
        return 1;
    }
    let path_str = match std::str::from_utf8(&mem_data[start..end]) {
        Ok(s) => s,
        Err(_) => {
            error!("file_create: invalid UTF-8");
            return 1;
        }
    };

    // Build the full path inside the sandbox.
    let root_path = caller.data().root_path.clone();
    let joined_path = root_path.join(path_str.trim_start_matches('/'));

    // Security check: ensure the parent directory is inside the sandbox.
    let parent = joined_path.parent().unwrap_or(&joined_path);
    let canonical_parent = match parent.canonicalize() {
        Ok(c) => c,
        Err(e) => {
            error!("file_create: failed to canonicalize parent: {}", e);
            return io_err_to_wasi_errno(&e);
        }
    };
    let canonical_root = match root_path.canonicalize() {
        Ok(c) => c,
        Err(e) => {
            error!("file_create: failed to canonicalize root: {}", e);
            return io_err_to_wasi_errno(&e);
        }
    };
    if !canonical_parent.starts_with(&canonical_root) {
        error!("file_create: attempt to escape sandbox root");
        return 13;
    }

    // Create the new file; use create_new(true) to fail if the file exists.
    match OpenOptions::new().write(true).create_new(true).open(&joined_path) {
        Ok(_file) => {
            // Retrieve metadata size (or use a fallback overhead, e.g. 4096 bytes).
            let metadata_size = match fs::metadata(&joined_path) {
                Ok(md) => md.len(),
                Err(_) => 4096,
            };
            // Update disk usage with the metadata overhead.
            if let Err(errno) = usage_add(&mut caller, metadata_size) {
                return errno;
            }
            // Allocate a new FD.
            let fd = {
                let pd = caller.data();
                let mut table = pd.fd_table.lock().unwrap();
                let fd = table.allocate_fd();
                if fd < 0 {
                    error!("file_create: No free FD available!");
                    return 76;
                }
                table.entries[fd as usize] = Some(FDEntry::File {
                    buffer: Vec::new(),
                    read_ptr: 0,
                    is_directory: false,
                    is_preopen: false,
                    host_path: Some(joined_path.to_string_lossy().into_owned()),
                });
                fd
            };

            // Write the new FD back into WASM memory.
            {
                let mem_mut = memory.data_mut(&mut caller);
                let out_ptr = opened_fd_out as usize;
                if out_ptr + 4 > mem_mut.len() {
                    error!("file_create: opened_fd_out pointer out of bounds");
                    return 1;
                }
                mem_mut[out_ptr..out_ptr + 4].copy_from_slice(&(fd as u32).to_le_bytes());
            }
            0
        }
        Err(e) => {
            error!("file_create: Failed to create file: {}", e);
            io_err_to_wasi_errno(&e)
        }
    }
}


/// Utility to write the "bytes used" result into memory
fn set_bufused(caller: &mut Caller<'_, ProcessData>, ptr: i32, value: u32) -> i32 {
    let memory = match caller.get_export("memory") {
        Some(wasmtime::Extern::Memory(mem)) => mem,
        _ => {
            eprintln!("fd_readdir: no memory export found (for bufused_out)");
            return 1;
        }
    };
    let mem_mut = memory.data_mut(caller);
    let out_ptr = ptr as usize;
    if out_ptr + 4 > mem_mut.len() {
        eprintln!("fd_readdir: bufused_out pointer out of bounds");
        return 1;
    }
    mem_mut[out_ptr..out_ptr + 4].copy_from_slice(&value.to_le_bytes());
    0
}
