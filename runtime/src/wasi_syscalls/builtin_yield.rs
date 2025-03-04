use wasmtime::Caller;
use crate::runtime::process::{ProcessData, ProcessState};
use log::{info, debug};

#[allow(non_snake_case)]
pub fn wasi__builtin_rt_yield(caller: Caller<'_, ProcessData>) {
    {
        let mut st = caller.data().state.lock().unwrap();
        if *st == ProcessState::Running {
            info!("wasi__builtin_rt_yield: Setting process state to Ready");
            *st = ProcessState::Ready;
        } else {
            debug!("wasi__builtin_rt_yield: Process state is not Running, current state: {:?}", *st);
        }
        // Notify the scheduler that we’re now waiting.
        caller.data().cond.notify_all();
        debug!("wasi__builtin_rt_yield: Notified the scheduler");
    }

    // Now wait until the state changes.
    let mut state = caller.data().state.lock().unwrap();
    while *state == ProcessState::Ready {
        debug!("wasi__builtin_rt_yield: Waiting for state to change from Ready");
        state = caller.data().cond.wait(state).unwrap();
    }
    debug!("wasi__builtin_rt_yield: State changed to {:?}", *state);
}
