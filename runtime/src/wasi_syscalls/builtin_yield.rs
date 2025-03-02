use wasmtime::Caller;
use crate::runtime::process::{ProcessData, ProcessState};
use log::{info, error, debug};


pub fn wasi_builtin_rt_yield(caller: Caller<'_, ProcessData>) {
    {
        let mut st = caller.data().state.lock().unwrap();
        if *st == ProcessState::Running {
            info!("fd_read: Setting process state to Ready");
            *st = ProcessState::Ready;
        }
        // Notify the scheduler that we’re now waiting.
        caller.data().cond.notify_all();
    }

    // Now wait until the state changes.
    let mut state = caller.data().state.lock().unwrap();
    while *state == ProcessState::Ready {
        state = caller.data().cond.wait(state).unwrap();
    }
}
