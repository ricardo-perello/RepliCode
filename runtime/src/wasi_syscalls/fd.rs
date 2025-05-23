use wasmtime::{Caller, Extern};
use std::convert::TryInto;
use crate::runtime::process::{BlockReason, ProcessData, ProcessState};
use crate::runtime::clock::GlobalClock;
use crate::runtime::fd_table::FDEntry;
use log::{info, error};



/// Implementation of fd_fdstat_get: returns file descriptor status information.
pub fn wasi_fd_fdstat_get(mut caller: Caller<'_, ProcessData>, fd: i32, buf: i32) -> i32 {
    info!("Called fd_fdstat_get with fd: {}", fd);
    
    // Get memory export
    let memory = match caller.get_export("memory") {
        Some(wasmtime::Extern::Memory(mem)) => mem,
        _ => {
            error!("fd_fdstat_get: no memory export found");
            return 1;
        }
    };

    // Get FD entry
    let fd_entry = {
        let process_data = caller.data();
        let table = process_data.fd_table.lock().unwrap();
        if fd < 0 || (fd as usize) >= table.entries.len() {
            return 8; // WASI_EBADF
        }
        table.entries[fd as usize].clone()
    };

    // Create fdstat buffer
    let mut fdstat = [0u8; 24]; // WASI fdstat struct size
    
    // Set file type (0=unknown, 1=block device, 2=character device, 3=directory, 4=regular file)
    if let Some(entry) = fd_entry {
        match entry {
            FDEntry::File { is_directory, .. } => {
                fdstat[0] = if is_directory { 3 } else { 4 };
            }
            FDEntry::Socket { .. } => {
                fdstat[0] = 5; // Socket type
            }
        }
    }

    // Set flags (0 for now)
    fdstat[2..4].copy_from_slice(&0u16.to_le_bytes());

    // Set rights (full rights for now)
    fdstat[8..16].copy_from_slice(&u64::MAX.to_le_bytes());  // fs_rights_base
    fdstat[16..24].copy_from_slice(&u64::MAX.to_le_bytes()); // fs_rights_inheriting

    // Write fdstat to memory
    let mem_mut = memory.data_mut(&mut caller);
    let buf_ptr = buf as usize;
    if buf_ptr + 24 > mem_mut.len() {
        error!("fd_fdstat_get: buffer out of bounds");
        return 1;
    }
    mem_mut[buf_ptr..buf_ptr + 24].copy_from_slice(&fdstat);

    0 // Success
}

/// Implementation of fd_seek: changes file position and returns new position.
pub fn wasi_fd_seek(
    mut caller: Caller<'_, ProcessData>,
    fd: i32,
    offset: i64,
    whence: i32,
    newoffset: i32,
) -> i32 {
    info!("Called fd_seek with fd: {}, offset: {}, whence: {}", fd, offset, whence);
    
    // Get memory export
    let memory = match caller.get_export("memory") {
        Some(wasmtime::Extern::Memory(mem)) => mem,
        _ => {
            error!("fd_seek: no memory export found");
            return 1;
        }
    };

    // Get current position and buffer length
    let (current_pos, buffer_len) = {
        let process_data = caller.data();
        let mut table = process_data.fd_table.lock().unwrap();
        if fd < 0 || (fd as usize) >= table.entries.len() {
            return 8; // WASI_EBADF
        }
        match &mut table.entries[fd as usize] {
            Some(FDEntry::File { read_ptr, buffer, .. }) => (*read_ptr as i64, buffer.len() as i64),
            _ => return 8, // WASI_EBADF
        }
    };

    // Calculate new position based on whence
    let new_pos = match whence {
        0 => offset,                    // SEEK_SET
        1 => current_pos + offset,      // SEEK_CUR
        2 => buffer_len + offset,       // SEEK_END
        _ => return 28,                 // WASI_EINVAL
    };

    // Check bounds
    if new_pos < 0 || new_pos > buffer_len {
        return 28; // WASI_EINVAL
    }

    // Update position
    {
        let process_data = caller.data();
        let mut table = process_data.fd_table.lock().unwrap();
        if let Some(FDEntry::File { read_ptr, .. }) = table.get_fd_entry_mut(fd) {
            *read_ptr = new_pos as usize;
        }
    }

    // Write new position to memory if requested
    if newoffset != 0 {
        let mem_mut = memory.data_mut(&mut caller);
        let out_ptr = newoffset as usize;
        if out_ptr + 8 > mem_mut.len() {
            return 1;
        }
        mem_mut[out_ptr..out_ptr + 8].copy_from_slice(&new_pos.to_le_bytes());
    }

    0 // Success
}

pub fn wasi_fd_read(
    mut caller: Caller<'_, ProcessData>,
    fd: i32,
    iovs: i32,
    iovs_len: i32,
    nread: i32,
) -> i32 {
    loop {
        let (data_to_read, _) = {
            let process_data = caller.data();
            let mut table = process_data.fd_table.lock().unwrap();
            match table.get_fd_entry_mut(fd) {
                Some(FDEntry::File { buffer, read_ptr, .. }) => {
                    if *read_ptr >= buffer.len() {
                        drop(table);
                        block_process_for_stdin(&mut caller);
                        continue;
                    }
                    let available_data = &buffer[*read_ptr..];
                    (available_data.to_vec(), available_data.len())
                }
                _ => {
                    error!("fd_read called with invalid FD: {}", fd);
                    return 1;
                }
            }
        };

        // At this point, data is available, so proceed to copy it into the WASM memory.
        let memory = match caller.get_export("memory") {
            Some(Extern::Memory(mem)) => mem,
            _ => {
                error!("fd_read: Failed to find memory export");
                return 1;
            }
        };

        let total_read = {
            // First, determine how many bytes will be read using the iovecs.
            let mut total = 0;
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
                    let to_copy = std::cmp::min(len, data_to_read.len() - total);
                    if to_copy == 0 {
                        break;
                    }
                    total += to_copy;
                    if total >= data_to_read.len() {
                        break;
                    }
                }
            }

            // Now actually write the data into the WASM memory.
            let data_mut = memory.data_mut(&mut caller);
            let mut total = 0;
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
                let to_copy = std::cmp::min(len, data_to_read.len() - total);
                if to_copy == 0 {
                    break;
                }
                data_mut[offset..offset + to_copy]
                    .copy_from_slice(&data_to_read[total..total + to_copy]);
                total += to_copy;
                if total >= data_to_read.len() {
                    break;
                }
            }
            // Write the total number of bytes read into memory.
            let total_read_bytes = (total as u32).to_le_bytes();
            let nread_ptr = nread as usize;
            if nread_ptr + 4 > data_mut.len() {
                error!("nread pointer out of bounds");
                return 1;
            }
            data_mut[nread_ptr..nread_ptr + 4].copy_from_slice(&total_read_bytes);
            total
        };

        // After reading, update the FD's read pointer by the actual bytes read
        {
            let process_data = caller.data();
            let mut table = process_data.fd_table.lock().unwrap();
            if let Some(FDEntry::File { read_ptr, .. }) = table.get_fd_entry_mut(fd) {
                *read_ptr += total_read;
            }
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
        // Notify the scheduler that we're now waiting.
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
        if fd < 0 || (fd as usize) >= table.entries.len() {
            return 8; // invalid FD
        }
        match &table.entries[fd as usize] {
            Some(FDEntry::File { is_preopen, is_directory, .. }) => (*is_preopen, *is_directory),
            _ => return 8,
        }
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
    _fd: i32,
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

/// Implementation for proc_exit: logs and terminates the process.
pub fn wasi_proc_exit(caller: Caller<'_, ProcessData>, code: i32) -> () {
    info!("Called proc_exit with code: {}", code);
    {
        let mut st = caller.data().state.lock().unwrap();
        *st = ProcessState::Finished;
    }
    caller.data().cond.notify_all();
    panic!("Process exited with code {}", code)
}   