use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use wasmtime::Caller;

use crate::runtime::process::{ProcessData, ProcessState, BlockReason};
use crate::runtime::fd_table::{FDEntry, MAX_FDS};

//TODO impose limit of disk allowed to avoid untrusted processes zipbombing the server

/// A helper to map I/O errors to a WASI-like i32 code (simplified).
fn io_err_to_wasi_errno(e: &io::Error) -> i32 {
    use io::ErrorKind::*;
    match e.kind() {
        NotFound => 2,           // e.g. __WASI_ERRNO_NOENT
        PermissionDenied => 13,  // e.g. __WASI_ERRNO_ACCES
        AlreadyExists => 20,     // __WASI_ERRNO_EXIST
        _ => 1,                  // catch-all or __WASI_ERRNO_IO
    }
}

/// This function is used to block a process for file I/O simulation, if desired.
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

/// Unlinks (deletes) a non-directory file in the sandbox.
/// Under WASI, `unlink(...)` calls `path_unlink_file`.
pub fn wasi_path_unlink_file(
    mut caller: wasmtime::Caller<'_, ProcessData>,
    dirfd: i32,
    path_ptr: i32,
    path_len: i32,
) -> i32 {
    use wasmtime::Extern;
    use log::error;

    // 1) Extract the path string from Wasm memory
    let memory = match caller.get_export("memory") {
        Some(Extern::Memory(mem)) => mem,
        Some(Extern::SharedMemory(_)) => {
            error!("path_unlink_file: SharedMemory not supported");
            return 1;
        },
        Some(Extern::Func(_)) | Some(Extern::Global(_)) | Some(Extern::Table(_)) | None => {
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

    // 2) As with other calls, figure out the directory FD or just always use
    //    the process's root_path. If your code uses dirfd properly, you'd
    //    look it up in the FD table. For simplicity, we do:
    let root_path = {
        let pd = caller.data();
        pd.root_path.clone()
    };

    // 3) Construct and canonicalize the path in the sandbox
    let joined = root_path.join(path_str.trim_start_matches('/'));
    let canonical = match joined.canonicalize() {
        Ok(c) => c,
        Err(e) => {
            error!("path_unlink_file: canonicalize error: {}", e);
            return 2; // e.g., WASI_ENOENT
        }
    };
    if !canonical.starts_with(&root_path) {
        error!("path_unlink_file: attempt to escape sandbox root!");
        return 13; // e.g. WASI_EACCES
    }

    // 4) Actually remove the file
    match std::fs::remove_file(&canonical) {
        Ok(_) => 0, // success
        Err(e) => {
            error!("path_unlink_file: failed to unlink file: {}", e);
            io_err_to_wasi_errno(&e)
        }
    }
}

/// Removes (deletes) a directory in the sandboxed filesystem.
/// In WASI, `rmdir()` calls `path_remove_directory`.
pub fn wasi_path_remove_directory(
    mut caller: wasmtime::Caller<'_, ProcessData>,
    dirfd: i32,
    path_ptr: i32,
    path_len: i32,
) -> i32 {
    use wasmtime::Extern;
    use log::error;

    // 1) Extract the path string from Wasm memory
    let memory = match caller.get_export("memory") {
        Some(Extern::Memory(mem)) => mem,
        Some(Extern::SharedMemory(_)) => {
            error!("path_remove_directory: SharedMemory not supported");
            return 1; 
        },
        Some(Extern::Func(_)) | Some(Extern::Global(_)) | Some(Extern::Table(_)) | None => {
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

    // 2) For a strict WASI approach, interpret dirfd. If `dirfd` is a preopen, 
    //    we join that path. Or, as your code does, you might just always use
    //    the process's `root_path` to keep it simple:
    let root_path = {
        let pd = caller.data();
        pd.root_path.clone()
    };

    // 3) Construct a sandboxed absolute path
    let joined = root_path.join(path_str.trim_start_matches('/'));
    let canonical = match joined.canonicalize() {
        Ok(c) => c,
        Err(e) => {
            error!("path_remove_directory: canonicalize error: {}", e);
            return 2; // e.g. WASI_ENOENT
        }
    };
    if !canonical.starts_with(&root_path) {
        error!("path_remove_directory: attempt to escape sandbox root!");
        return 13; // e.g. WASI_EACCES
    }

    // 4) Actually remove the directory
    match std::fs::remove_dir(&canonical) {
        Ok(_) => 0, // success
        Err(e) => {
            error!("path_remove_directory: failed to remove dir: {}", e);
            io_err_to_wasi_errno(&e) // same helper you used in path_open
        }
    }
}

/// Creates a new directory in the sandbox.
/// In WASI, signature is something like:
///   __wasi_errno_t path_create_directory(
///       __wasi_fd_t fd,
///       const uint8_t *path,
///       size_t path_len
///   );
///
/// We'll interpret `fd` as the directory FD (often 3 for the preopened root).
pub fn wasi_path_create_directory(
    mut caller: wasmtime::Caller<'_, ProcessData>,
    dirfd: i32,
    path_ptr: i32,
    path_len: i32,
) -> i32 {
    use wasmtime::Extern;
    use log::error;

    // 1) Extract the path string from Wasm memory
    let memory = match caller.get_export("memory") {
        Some(Extern::Memory(mem)) => mem,
        Some(Extern::SharedMemory(_)) => {
            error!("path_create_directory: SharedMemory not supported");
            return 1; // or a WASI errno
        },
        Some(Extern::Func(_)) | Some(Extern::Global(_)) | Some(Extern::Table(_)) | None => {
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

    // 2) For a strict WASI approach, you might check if dirfd == 3, or
    //    interpret dirfd in your FD table. But for simplicity, let's
    //    always use the process's `root_path`:
    let root_path = {
        let pd = caller.data();
        pd.root_path.clone()
    };

    // 3) Build a canonical path in the sandbox
    let joined = root_path.join(path_str.trim_start_matches('/'));
    let canonical = match joined.canonicalize() {
        Ok(c) => c,
        Err(e) => {
            error!("path_create_directory: canonicalize error: {}", e);
            return 2; // e.g. WASI_ENOENT
        }
    };
    // Ensure we don't escape the sandbox
    if !canonical.starts_with(&root_path) {
        error!("path_create_directory: attempt to escape sandbox root!");
        return 13; // e.g. WASI_EACCES
    }

    // 4) Attempt to create the directory
    match std::fs::create_dir(&canonical) {
        Ok(_) => 0, // success
        Err(e) => {
            error!("path_create_directory: failed to create dir: {}", e);
            io_err_to_wasi_errno(&e)  // map to WASI code
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
