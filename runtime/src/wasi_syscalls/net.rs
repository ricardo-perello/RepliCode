use wasmtime::Caller;
use crate::runtime::process::{BlockReason, ProcessData, ProcessState};

//TODO dummy version, need to talk to gauthier to ensure how it goes through consensus


pub fn wasi_sock_open(
    mut caller: Caller<'_, ProcessData>,
    domain: i32,
    socktype: i32,
    protocol: i32,
    sock_fd_out: i32,
) -> i32 {
    println!("Called sock_open with domain={}, socktype={}, protocol={}", domain, socktype, protocol);

    // Possibly block here if “no more sockets available” or if we want
    // to simulate a blocking condition. For now, we do not block:
    let memory = match caller.get_export("memory") {
        Some(wasmtime::Extern::Memory(mem)) => mem,
        _ => {
            eprintln!("sock_open: no memory export found");
            return 1;
        }
    };

    // Allocate a new FD for this socket in our FD table:
    let new_sock_fd = {
        let process_data = caller.data();
        let mut table = process_data.fd_table.lock().unwrap();
        let fd = table.allocate_fd();
        table.entries[fd as usize] = Some(crate::runtime::fd_table::FDEntry {
            buffer: Vec::new(),
            read_ptr: 0,
        });
        fd
    };

    // Write back the new FD:
    let mem_mut = memory.data_mut(&mut caller);
    let out_ptr = sock_fd_out as usize;
    if out_ptr + 4 > mem_mut.len() {
        eprintln!("sock_open: sock_fd_out pointer out of bounds");
        return 1;
    }
    mem_mut[out_ptr..out_ptr+4].copy_from_slice(&(new_sock_fd as u32).to_le_bytes());
    0
}

/// Example stub for socket listen: 'wasi_sock_listen'
/// This is also not official WASI but helps illustrate how you can block.
pub fn wasi_sock_listen(
    mut caller: Caller<'_, ProcessData>,
    fd: i32,
    backlog: i32,
) -> i32 {
    println!("Called sock_listen on fd={}, backlog={}", fd, backlog);
    // If we want to block (e.g. we cannot listen yet?), do so:
    let can_listen_now = true; // pretend we can listen
    if !can_listen_now {
        block_process_for_network(&mut caller);
        return 0;
    }

    // Otherwise, just print success.
    println!("Socket is now listening!");
    0
}


fn block_process_for_network(caller: &mut Caller<'_, ProcessData>) {
    {
        let mut state = caller.data().state.lock().unwrap();
        if *state == ProcessState::Running {
            println!("Network IO: Setting process state to Blocked");
            *state = ProcessState::Blocked;
        }
        let mut reason = caller.data().block_reason.lock().unwrap();
        *reason = Some(BlockReason::NetworkIO);
        caller.data().cond.notify_all();
    }

    let mut state = caller.data().state.lock().unwrap();
    while *state != ProcessState::Running {
        state = caller.data().cond.wait(state).unwrap();
    }
}
