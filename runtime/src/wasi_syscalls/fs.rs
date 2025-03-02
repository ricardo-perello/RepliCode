use std::fs;
use std::io;
use std::path::Path;
use wasmtime::Caller;
use crate::runtime::process::{ProcessData, ProcessState};
use crate::runtime::fd_table::{FDEntry, MAX_FDS};
use crate::runtime::process::BlockReason;

/// A helper to map I/O errors to a WASI-like i32 code (simplified).
fn io_err_to_wasi_errno(e: &io::Error) -> i32 {
    use io::ErrorKind::*;
    match e.kind() {
        NotFound => 2,       // e.g. __WASI_ERRNO_NOENT
        PermissionDenied => 13, // e.g. __WASI_ERRNO_ACCES
        _ => 1,              // catch-all
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
pub fn wasi_path_open(
    mut caller: Caller<'_, ProcessData>,
    dirfd: i32,
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

    // 1) Extract the path string from Wasm memory
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
    println!("path_open: Opening '{}'", path_str);

    // 2) For simplicity, treat the path as absolute or relative to current dir
    //    ignoring 'dirfd' (some WASI calls want you to do relative to dirfd).
    let path = Path::new(path_str);

    // 3) Attempt to fetch metadata to see if it’s a file or directory
    let metadata = match fs::metadata(&path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("path_open: metadata error: {}", e);
            return io_err_to_wasi_errno(&e);
        }
    };

    // 4) Allocate an FD in our table
    let fd = {
        let process_data = caller.data();
        let mut table = process_data.fd_table.lock().unwrap();
        let fd = table.allocate_fd();
        if fd < 0 {
            eprintln!("path_open: No free FD available!");
            return 76; // some WASI "ENFILE" or "EMFILE" code
        }
        fd
    };

    // 5) If the path is a directory, read dir listing. Otherwise, read file.
    let mut buffer: Vec<u8> = Vec::new();
    if metadata.is_dir() {
        // Read entire directory listing into buffer, line by line
        match fs::read_dir(&path) {
            Ok(entries) => {
                for entry_res in entries {
                    match entry_res {
                        Ok(dirent) => {
                            let name = dirent.file_name();
                            // Convert OsString -> String (lossy) or keep as bytes
                            let name_str = name.to_string_lossy();
                            buffer.extend_from_slice(name_str.as_bytes());
                            buffer.push(b'\n');
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
    } else {
        // It's a file - read entire file contents
        match fs::read(&path) {
            Ok(file_data) => {
                // (Optional) if you want to simulate blocking for large files:
                if file_data.len() > 1_000_000 {
                    println!("path_open: File is large => blocking to simulate I/O wait");
                    block_process_for_fileio(&mut caller);
                    // After unblocking, proceed or read in chunks, etc.
                }
                buffer = file_data;
            }
            Err(e) => {
                eprintln!("path_open: Failed to open file: {}", e);
                return io_err_to_wasi_errno(&e);
            }
        }
    }

    // 6) Put this data into the FDTable
    {
        let process_data = caller.data();
        let mut table = process_data.fd_table.lock().unwrap();
        table.entries[fd as usize] = Some(FDEntry {
            buffer,
            read_ptr: 0,
        });
    }

    // 7) Write the newly opened FD back to Wasm memory
    let out_ptr = opened_fd_out as usize;
    let mem_mut = memory.data_mut(&mut caller);
    if out_ptr + 4 > mem_mut.len() {
        eprintln!("path_open: opened_fd_out out of bounds");
        return 1;
    }
    mem_mut[out_ptr..out_ptr + 4].copy_from_slice(&(fd as u32).to_le_bytes());

    println!("path_open: success, new FD = {}", fd);
    0
}



pub fn wasi_fd_readdir(
    mut caller: Caller<'_, ProcessData>,
    fd: i32,
    buf: i32,
    buf_len: i32,
    cookie: i64,
    bufused_out: i32,
) -> i32 {
    println!("fd_readdir(fd={}, buf={}, buf_len={}, cookie={})", fd, buf, buf_len, cookie);

    // 1) Grab the FD entry
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

        // If there's nothing left to read, we can return 0
        if fd_entry.read_ptr >= fd_entry.buffer.len() {
            println!("fd_readdir: End of directory listing, returning 0 used bytes");
            drop(table);
            return set_bufused(&mut caller, bufused_out, 0);
        }

        // Copy out the data. We'll do it the same way as fd_read:
        let slice = &fd_entry.buffer[fd_entry.read_ptr..];
        (slice.to_vec(), fd_entry.read_ptr)
    };

    // 2) Determine how many bytes we can copy
    let n_to_copy = std::cmp::min(data_to_read.len(), buf_len as usize);

    // 3) Copy to the `buf` pointer in Wasm memory
    let memory = match caller.get_export("memory") {
        Some(wasmtime::Extern::Memory(mem)) => mem,
        _ => {
            eprintln!("fd_readdir: no memory export found");
            return 1;
        }
    };

    {
        let mem_mut = memory.data_mut(&mut caller);
        let buf_start = buf as usize;
        let buf_end = buf_start + n_to_copy;
        if buf_end > mem_mut.len() {
            eprintln!("fd_readdir: buf out of bounds");
            return 1;
        }
        mem_mut[buf_start..buf_end].copy_from_slice(&data_to_read[..n_to_copy]);
    }

    // 4) Update FD’s read_ptr
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
    while *state == ProcessState::Blocked {
        state = caller.data().cond.wait(state).unwrap();
    }
}