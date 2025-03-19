use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use wasmtime::Caller;

use crate::runtime::process::{ProcessData, ProcessState, BlockReason};
use crate::runtime::fd_table::{FDEntry, MAX_FDS};

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

/// Increment the process’s tracked usage by `bytes`. If the limit is exceeded,
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
        kill_process(caller);
        // kill_process(...) never returns, because it panics
    }

    Ok(())
}


/// Decrement the process’s tracked usage by `bytes`. 
fn usage_sub(caller: &mut Caller<'_, ProcessData>, bytes: u64) {
    let pd = caller.data();
    let mut usage = pd.current_disk_usage.lock().unwrap();
    *usage = usage.saturating_sub(bytes);
}

/// If you remove a directory, or some other operation, and need to figure out how many
/// bytes were in that directory, you can do a quick naive walk:
fn get_dir_size(path: &Path) -> io::Result<u64> {
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

/// Kill the current process: mark it Finished, remove its directory, and panic.
fn kill_process(caller: &mut Caller<'_, ProcessData>) -> ! {
    {
        let mut st = caller.data().state.lock().unwrap();
        *st = ProcessState::Finished;
    }
    let pd = caller.data();
    let _ = fs::remove_dir_all(&pd.root_path);
    pd.cond.notify_all();
    panic!("Process forcibly killed due to disk quota exceeded");
}

// ----------------------------------------------------------------------------
// File/directory ops below
// ----------------------------------------------------------------------------

pub fn wasi_path_unlink_file(
    mut caller: wasmtime::Caller<'_, ProcessData>,
    dirfd: i32,
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
    let canonical = match joined.canonicalize() {
        Ok(c) => c,
        Err(e) => {
            error!("path_unlink_file: canonicalize error: {}", e);
            return 2;
        }
    };
    if !canonical.starts_with(&root_path) {
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
    dirfd: i32,
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
    let canonical = match joined.canonicalize() {
        Ok(c) => c,
        Err(e) => {
            error!("path_remove_directory: canonicalize error: {}", e);
            return 2;
        }
    };
    if !canonical.starts_with(&root_path) {
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
    dirfd: i32,
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
    let joined = root_path.join(path_str.trim_start_matches('/'));
    let canonical = match joined.canonicalize() {
        Ok(c) => c,
        Err(e) => {
            // If canonicalize fails because it doesn't exist yet, we might 
            // just use `joined.clone()`. For brevity, let's do:
            joined
        }
    };
    if !canonical.starts_with(&root_path) {
        error!("path_create_directory: attempt to escape sandbox root!");
        return 13;
    }

    match fs::create_dir(&canonical) {
        Ok(_) => {
            // For a directory, you can count a small overhead. 
            // Or do metadata().len(). Let’s do that:
            let dir_metadata_size = match fs::metadata(&canonical) {
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


pub fn wasi_fd_close(caller: Caller<'_, ProcessData>, fd: i32) -> i32 {
    println!("fd_close: closing fd {}", fd);
    let process_data = caller.data();
    let mut table = process_data.fd_table.lock().unwrap();
    if fd < 0 || fd as usize >= MAX_FDS {
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
    dirfd: i32,
    dirflags: i32, 
    path_ptr: i32,
    path_len: i32,
    oflags: i32,
    fs_rights_base: i64,
    fs_rights_inheriting: i64,
    fdflags: i32,
    opened_fd_out: i32,
) -> i32 {
    println!(
        "path_open: dirfd={}, oflags={}, base_rights={}, inheriting={}, fdflags={}",
        dirfd, oflags, fs_rights_base, fs_rights_inheriting, fdflags
    );

    //
    // 1) Extract the path from Wasm memory
    //
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
        Ok(s) => s,
        Err(_) => {
            eprintln!("path_open: invalid UTF-8");
            return 1;
        }
    };

    //
    // 2) Clone just the `root_path` from the ProcessData, then drop the reference
    //
    // We *only* hold `pd` momentarily to read out what we need.
    let root_path = {
        let pd = caller.data(); // Immutable borrow
        pd.root_path.clone()    // store locally
    };
    // `pd` reference is now dropped; no more overlap

    println!("path_open: requested path: '{}'", path_str);

    //
    // 3) Build and check canonical path wholly in local variables
    //
    let joined_path = root_path.join(path_str.trim_start_matches('/'));
    let canonical = match joined_path.canonicalize() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("path_open: canonicalize error {}", e);
            return io_err_to_wasi_errno(&e);
        }
    };
    if !canonical.starts_with(&root_path) {
        eprintln!("path_open: attempt to escape sandbox root!");
        return 13; // e.g. WASI_EACCES
    }

    // Metadata check (again, no reference to caller)
    let metadata = match std::fs::metadata(&canonical) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("path_open: metadata error: {}", e);
            return io_err_to_wasi_errno(&e);
        }
    };

    //
    // 4) Read the file or directory into a local buffer
    //
    let buffer = if metadata.is_dir() {
        // read_dir scenario
        let mut buf = Vec::new();
        match std::fs::read_dir(&canonical) {
            Ok(entries) => {
                for entry_res in entries {
                    match entry_res {
                        Ok(dirent) => {
                            let name = dirent.file_name();
                            let name_str = name.to_string_lossy();
                            buf.extend_from_slice(name_str.as_bytes());
                            buf.push(b'\n');
                        }
                        Err(e) => {
                            eprintln!("path_open: error reading directory entry: {}", e);
                            return io_err_to_wasi_errno(&e);
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("path_open: read_dir error: {}", e);
                return io_err_to_wasi_errno(&e);
            }
        }
        buf
    } else {
        // file scenario
        let file_data = match std::fs::read(&canonical) {
            Ok(fd) => fd,
            Err(e) => {
                eprintln!("path_open: Failed to open file: {}", e);
                return io_err_to_wasi_errno(&e);
            }
        };

        // If large, do blocking
        if file_data.len() > 1_000_000 {
            println!("path_open: File is large => blocking to simulate I/O wait");

            // NOW we can block, but only after we've dropped all references.
            // We do not hold a `caller.data()` reference here; no FD table lock.
            block_process_for_fileio(&mut caller);
        }

        file_data
    };

    //
    // 5) Now we re-borrow `caller` to allocate an FD and store the buffer
    //
    let fd = {
        let pd = caller.data(); // re-borrow immutably
        let mut table = pd.fd_table.lock().unwrap();

        let fd = table.allocate_fd();
        if fd < 0 {
            eprintln!("path_open: No free FD available!");
            return 76; // e.g. ENFILE or EMFILE
        }
        table.entries[fd as usize] = Some(FDEntry {
            buffer,
            read_ptr: 0,
            is_directory: metadata.is_dir(),
            is_preopen: false, // normal open is not "preopen"
            host_path: Some(canonical.to_string_lossy().into_owned()),
        });
        fd
    };
    // That scope is dropped, so the FD table lock is released now.

    //
    // 6) Write the newly opened FD back to Wasm memory
    //
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

        let fd_entry = match table.get_fd_entry_mut(fd) {
            Some(entry) => entry,
            None => {
                eprintln!("fd_readdir: invalid FD {}", fd);
                return 8; // WASI_EBADF
            }
        };

        if fd_entry.read_ptr >= fd_entry.buffer.len() {
            println!("fd_readdir: End of directory listing, returning 0 used bytes");
            // We'll set bufused_out to 0. But do that after we drop the lock.
            (Vec::new(), fd_entry.read_ptr)
        } else {
            let slice = &fd_entry.buffer[fd_entry.read_ptr..];
            // Copy to local vec
            let local_copy = slice.to_vec();
            (local_copy, fd_entry.read_ptr)
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
        if let Some(entry) = table.get_fd_entry_mut(fd) {
            entry.read_ptr = read_ptr_before + n_to_copy;
        }
    }

    // 5) Write how many bytes we used into bufused_out
    set_bufused(&mut caller, bufused_out, n_to_copy as u32)
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
