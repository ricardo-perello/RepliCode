use wasmtime::{Caller, Extern};
use std::io::{self, Write};
use std::convert::TryInto;
use crate::runtime::process::{BlockReason, ProcessData, ProcessState};
use crate::runtime::clock::GlobalClock;
use log::{info, error};



/// Dummy implementation for fd_fdstat_get: logs the call.
pub fn wasi_fd_fdstat_get(_caller: Caller<'_, ProcessData>, fd: i32, _buf: i32) -> i32 {
    info!("Called fd_fdstat_get with fd: {}", fd);
    0
}

/// Dummy implementation for fd_seek: logs the call.
pub fn wasi_fd_seek(_caller: Caller<'_, ProcessData>, fd: i32, offset: i64, whence: i32, _newoffset: i32) -> i32 {
    info!("Called fd_seek with fd: {}, offset: {}, whence: {}", fd, offset, whence);
    0
}

pub fn wasi_fd_read(
    mut caller: Caller<'_, ProcessData>,
    fd: i32,
    iovs: i32,
    iovs_len: i32,
    nread: i32,
) -> i32 {
    loop {
        // Lock the FD table and try to extract data.
        let (data_to_read, bytes_to_advance) = {
            let process_data = caller.data();
            let mut table = process_data.fd_table.lock().unwrap();
            let fd_entry = match table.get_fd_entry_mut(fd) {
                Some(entry) => entry,
                _ => {
                    error!("fd_read called with invalid FD: {}", fd);
                    return 1;
                }
            };

            // If no data is available, then we want to block.
            if fd_entry.read_ptr >= fd_entry.buffer.len() {
                // Drop the lock and wait until input is available.
                drop(table);
                block_process_for_stdin(&mut caller);
                continue;
            }
            // Otherwise, clone available data.
            let available_data = &fd_entry.buffer[fd_entry.read_ptr..];
            (available_data.to_vec(), available_data.len())
        };

        // At this point, data is available, so proceed to copy it into the WASM memory.
        let memory = match caller.get_export("memory") {
            Some(Extern::Memory(mem)) => mem,
            _ => {
                error!("fd_read: Failed to find memory export");
                return 1;
            }
        };

        {
            // First, determine how many bytes will be read using the iovecs.
            let mut total_read = 0;
            {
                let data = memory.data(&caller);
                for i in 0..iovs_len {
                    let iovec_addr = (iovs as usize) + (i as usize) * 8;
                    if iovec_addr + 8 > data.len() {
                        error!("iovec out of bounds");
                        return 1;
                    }
                    let offset_bytes: [u8; 4] = data[iovec_addr..iovec_addr + 4].try_into().unwrap();
                    let len_bytes: [u8; 4] = data[iovec_addr + 4..iovec_addr + 8].try_into().unwrap();
                    let offset = u32::from_le_bytes(offset_bytes) as usize;
                    let len = u32::from_le_bytes(len_bytes) as usize;
                    if offset + len > data.len() {
                        error!("data slice out of bounds");
                        return 1;
                    }
                    let to_copy = std::cmp::min(len, data_to_read.len() - total_read);
                    if to_copy == 0 {
                        break;
                    }
                    total_read += to_copy;
                    if total_read >= data_to_read.len() {
                        break;
                    }
                }
            }

            // Now actually write the data into the WASM memory.
            let data_mut = memory.data_mut(&mut caller);
            let mut total_read = 0;
            for i in 0..iovs_len {
                let iovec_addr = (iovs as usize) + (i as usize) * 8;
                if iovec_addr + 8 > data_mut.len() {
                    error!("iovec out of bounds");
                    return 1;
                }
                let offset_bytes: [u8; 4] = data_mut[iovec_addr..iovec_addr + 4].try_into().unwrap();
                let len_bytes: [u8; 4] = data_mut[iovec_addr + 4..iovec_addr + 8].try_into().unwrap();
                let offset = u32::from_le_bytes(offset_bytes) as usize;
                let len = u32::from_le_bytes(len_bytes) as usize;
                if offset + len > data_mut.len() {
                    error!("data slice out of bounds");
                    return 1;
                }
                let to_copy = std::cmp::min(len, data_to_read.len() - total_read);
                if to_copy == 0 {
                    break;
                }
                data_mut[offset..offset + to_copy]
                    .copy_from_slice(&data_to_read[total_read..total_read + to_copy]);
                total_read += to_copy;
                if total_read >= data_to_read.len() {
                    break;
                }
            }
            // Write the total number of bytes read into memory.
            let total_read_bytes = (total_read as u32).to_le_bytes();
            let nread_ptr = nread as usize;
            if nread_ptr + 4 > data_mut.len() {
                error!("nread pointer out of bounds");
                return 1;
            }
            data_mut[nread_ptr..nread_ptr + 4].copy_from_slice(&total_read_bytes);
        }

        // After reading, update the FD's read pointer.
        {
            let process_data = caller.data();
            let mut table = process_data.fd_table.lock().unwrap();
            let fd_entry = table.get_fd_entry_mut(fd).unwrap();
            fd_entry.read_ptr += bytes_to_advance;
        }
        return 0;
    }
}

/// Blocks the process, telling the scheduler we're waiting on stdin.
fn block_process_for_stdin(caller: &mut Caller<'_, ProcessData>) {
    {
        let mut st = caller.data().state.lock().unwrap();
        if *st == ProcessState::Running {
            info!("fd_read: Setting process state to Blocked");
            *st = ProcessState::Blocked;
        }
        let mut reason = caller.data().block_reason.lock().unwrap();
        *reason = Some(BlockReason::StdinRead);
        // Notify the scheduler that we’re now waiting.
        caller.data().cond.notify_all();
    }

    // Now wait until the state changes.
    let mut state = caller.data().state.lock().unwrap();
    while *state != ProcessState::Running {
        state = caller.data().cond.wait(state).unwrap();
    }
}

pub fn wasi_fd_prestat_get(
    mut caller: wasmtime::Caller<'_, ProcessData>,
    fd: i32,
    prestat_ptr: i32,
) -> i32 {
    use wasmtime::Extern;
    // Get memory export.
    let memory = match caller.get_export("memory") {
        Some(Extern::Memory(mem)) => mem,
        _ => return 1,
    };

    // Retrieve the FD entry for fd. We assume that if it's preopen and a directory,
    // we want to treat it as the current working directory.
    let (is_preopen, is_dir) = {
        let pd = caller.data();
        let table = pd.fd_table.lock().unwrap();
        if fd < 0 || (fd as usize) >= crate::runtime::fd_table::MAX_FDS {
            return 8; // invalid FD
        }
        let entry = match &table.entries[fd as usize] {
            Some(e) => e,
            None => return 8,
        };
        (entry.is_preopen, entry.is_directory)
    };

    // Only preopened directories should be returned
    if !is_preopen || !is_dir {
        return 8;
    }

    // For our purposes, we want the "directory name" to be "."
    let name_len: u32 = 1; // "." is 1 byte
    // Build the prestat buffer:
    //   offset 0: type (0 for directory)
    //   offset 4: length of the directory name
    let mut buf = [0u8; 8];
    buf[0] = 0; // __WASI_PREOPENTYPE_DIR
    buf[4..8].copy_from_slice(&name_len.to_le_bytes());

    // Write the prestat struct back to memory.
    let offset = prestat_ptr as usize;
    let mem_mut = memory.data_mut(&mut caller);
    if offset + 8 > mem_mut.len() {
        return 1;
    }
    mem_mut[offset..offset+8].copy_from_slice(&buf);
    0
}


pub fn wasi_fd_prestat_dir_name(
    mut caller: wasmtime::Caller<'_, ProcessData>,
    fd: i32,
    path_ptr: i32,
    path_len: i32,
) -> i32 {
    use wasmtime::Extern;
    use log::error;
    let memory = match caller.get_export("memory") {
        Some(Extern::Memory(mem)) => mem,
        _ => {
            error!("fd_prestat_dir_name: Memory not found");
            return 1;
        }
    };

    // Return "." so that WASI libc uses FD=3 as the current working directory.
    let dir_str = ".";
    let needed = dir_str.len();
    if (path_len as usize) < needed {
        return 1;
    }

    let mem_mut = memory.data_mut(&mut caller);
    let offset = path_ptr as usize;
    if offset + needed > mem_mut.len() {
        return 1;
    }

    mem_mut[offset..offset+needed].copy_from_slice(dir_str.as_bytes());
    0
}





pub fn wasi_poll_oneoff(
    mut caller: Caller<'_, ProcessData>,
    subscriptions_ptr: i32,
    events_ptr: i32,
    nsubscriptions: i32,
    nevents_ptr: i32,
) -> i32 {
    // Get the memory export.
    let memory = match caller.get_export("memory") {
        Some(Extern::Memory(mem)) => mem,
        _ => {
            error!("poll_oneoff: Failed to find memory export");
            return 1;
        }
    };

    let mem_data = memory.data(&caller);
    let subscription_size = 48;
    let nsubs = nsubscriptions as usize;
    if (subscriptions_ptr as usize) + nsubs * subscription_size > mem_data.len() {
        error!("poll_oneoff: Subscription array out of bounds");
        return 1;
    }

    // For each subscription, extract its parameters and compute the wake time.
    let now = GlobalClock::now();
    let mut subscriptions = Vec::with_capacity(nsubs);
    let mut earliest_wake_time = u64::MAX;
    for i in 0..nsubs {
        let sub_offset = (subscriptions_ptr as usize) + i * subscription_size;
        // Read userdata (u64) from offset 0.
        let userdata_bytes = &mem_data[sub_offset..sub_offset + 8];
        let userdata = u64::from_le_bytes(userdata_bytes.try_into().unwrap());
        // Read type (u16) from offset 8.
        let type_bytes = &mem_data[sub_offset + 8..sub_offset + 10];
        let sub_type = u16::from_le_bytes(type_bytes.try_into().unwrap());
        // Read timeout (u64) from offset 24.
        let timeout_bytes = &mem_data[sub_offset + 24..sub_offset + 32];
        let timeout_nanos = u64::from_le_bytes(timeout_bytes.try_into().unwrap());

        // Use a default of 1 second if timeout is 0.
        let sleep_nanos = if timeout_nanos == 0 { 1_000_000_000 } else { timeout_nanos };
        let wake_time = now + sleep_nanos;
        if wake_time < earliest_wake_time {
            earliest_wake_time = wake_time;
        }
        subscriptions.push((userdata, sub_type, wake_time));
    }

    info!(
        "poll_oneoff: Blocking process until earliest wake time: {} (current: {})",
        earliest_wake_time, now
    );

    // Block the process until the earliest wake time.
    {
        let process_data = caller.data();
        let mut state = process_data.state.lock().unwrap();
        let mut reason = process_data.block_reason.lock().unwrap();
        *reason = Some(BlockReason::Timeout { resume_after: earliest_wake_time });
        *state = ProcessState::Blocked;
        process_data.cond.notify_all();
    }

    // Wait until the scheduler unblocks the process.
    {
        let mut state = caller.data().state.lock().unwrap();
        while *state != ProcessState::Running {
            state = caller.data().cond.wait(state).unwrap();
        }
    } // Lock on state is dropped here.

    // After unblocking, check which subscriptions have reached their wake time.
    let current_time = GlobalClock::now();
    let mut num_events = 0;
    let event_size = 32;
    let events_addr = events_ptr as usize;
    {
        let mem_mut = memory.data_mut(&mut caller);
        if events_addr + nsubs * event_size > mem_mut.len() {
            error!("poll_oneoff: Events area out of bounds");
            return 1;
        }
        // For each subscription, if the current time is at or past its wake time, record an event.
        for (userdata, sub_type, wake_time) in subscriptions.iter() {
            if current_time >= *wake_time {
                let event_offset = events_addr + num_events * event_size;
                // Write userdata (8 bytes).
                mem_mut[event_offset..event_offset + 8].copy_from_slice(&userdata.to_le_bytes());
                // Write error code (0 for success) as u16.
                mem_mut[event_offset + 8..event_offset + 10].copy_from_slice(&0u16.to_le_bytes());
                // Write the event type.
                mem_mut[event_offset + 10..event_offset + 12].copy_from_slice(&sub_type.to_le_bytes());
                // Zero the remaining bytes.
                for byte in &mut mem_mut[event_offset + 12..event_offset + event_size] {
                    *byte = 0;
                }
                num_events += 1;
            }
        }
        // Write the number of events (triggered subscriptions) to nevents_ptr.
        let nevents_addr = nevents_ptr as usize;
        if nevents_addr + 8 > mem_mut.len() {
            error!("poll_oneoff: nevents pointer out of bounds");
            return 1;
        }
        mem_mut[nevents_addr..nevents_addr + 8].copy_from_slice(&((num_events as u64).to_le_bytes()));
    }
    0
}

/// Implementation for fd_write: writes to stdout/stderr.
pub fn wasi_fd_write(
    mut caller: Caller<'_, ProcessData>,
    fd: i32,
    iovs: i32,
    iovs_len: i32,
    nwritten: i32,
) -> i32 {
    let memory = match caller.get_export("memory") {
        Some(Extern::Memory(mem)) => mem,
        _ => {
            error!("fd_write: Failed to find memory export");
            return 1;
        }
    };

    // First, use an immutable borrow to read the iovec information.
    let total_written = {
        let data = memory.data(&caller);
        let mut total_written = 0;
        for i in 0..iovs_len {
            let iovec_addr = (iovs as usize) + (i as usize) * 8;
            if iovec_addr + 8 > data.len() {
                error!("iovec out of bounds");
                return 1;
            }
            let offset_bytes: [u8; 4] = data[iovec_addr..iovec_addr + 4].try_into().unwrap();
            let len_bytes: [u8; 4] = data[iovec_addr + 4..iovec_addr + 8].try_into().unwrap();
            let offset = u32::from_le_bytes(offset_bytes) as usize;
            let len = u32::from_le_bytes(len_bytes) as usize;
            if offset + len > data.len() {
                error!("data slice out of bounds");
                return 1;
            }
            let slice = &data[offset..offset + len];
            match fd {
                1 => { io::stdout().write_all(slice).unwrap(); },
                2 => { io::stderr().write_all(slice).unwrap(); },
                _ => {
                    error!("fd_write called with unsupported fd: {}", fd);
                    return 1;
                }
            }
            total_written += len;
        }
        total_written
    };

    // Then use a mutable borrow to write the nwritten value back into memory.
    {
        let total_written_bytes = (total_written as u32).to_le_bytes();
        let nwritten_ptr = nwritten as usize;
        let mem_mut = memory.data_mut(&mut caller);
        if nwritten_ptr + 4 > mem_mut.len() {
            error!("nwritten pointer out of bounds");
            return 1;
        }
        mem_mut[nwritten_ptr..nwritten_ptr + 4].copy_from_slice(&total_written_bytes);
    }
    0
}

/// Implementation for proc_exit: logs and terminates the process.
pub fn wasi_proc_exit(_caller: Caller<'_, ProcessData>, code: i32) {
    info!("Called proc_exit with code: {}", code);
    std::process::exit(code);
}