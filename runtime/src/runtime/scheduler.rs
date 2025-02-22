use anyhow::Result;
use crate::runtime::process::{Process, ProcessState};
use crate::wasi_syscalls::fd::GLOBAL_INPUT;
use std::thread::sleep;
use std::time::{Duration, Instant};

use super::process::BlockReason;

/// A simple round-robin scheduler. It loops over all processes,
/// checks their shared state, and if a process is Blocked (for example, waiting on input),
/// the scheduler “unblocks” it (simulating that data has arrived).
/// Finished processes are joined and dropped.
pub fn run_scheduler(mut processes: Vec<Process>) -> Result<()> {
    while !processes.is_empty() {
        let mut still_running = Vec::new();
        for process in processes {
            // Copy the current state.
            let state_copy = {
                let guard = process.data.state.lock().unwrap();
                *guard
            };
            match state_copy {
                ProcessState::Finished => {
                    // Process is done: join its thread.
                    let _ = process.thread.join();
                }
                ProcessState::Blocked => {

                    let reason = { 
                        let reason_guard = process.data.block_reason.lock().unwrap();
                        reason_guard.clone() 
                    };

                    // // For demonstration we “unblock” a blocked process.
                    if let Some(BlockReason::StdinRead) = reason {
                        // Check if there is data in the input buffer.
                        if input_buffer_has_data() {
                            // Unblock the process.
                            let mut st = process.data.state.lock().unwrap();
                            *st = ProcessState::Running;
                            let mut reason = process.data.block_reason.lock().unwrap();
                            *reason = None;
                            process.data.cond.notify_all();
                        }
                    } else if let Some(BlockReason::Timeout { resume_after }) = reason {
                        // Check if the timeout has elapsed.
                        if Instant::now() >= resume_after {
                            // Unblock the process.
                            let mut st = process.data.state.lock().unwrap();
                            *st = ProcessState::Running;
                            let mut reason = process.data.block_reason.lock().unwrap();
                            *reason = None;
                            process.data.cond.notify_all();
                        }
                    }
                    still_running.push(process);
                }
                ProcessState::Running => {
                    // Let the process run for a time slice.
                    sleep(Duration::from_millis(10));
                    // Then simulate that it is blocking (for example, waiting on I/O).
                    {
                        let mut guard = process.data.state.lock().unwrap();
                        if let ProcessState::Running = *guard {
                            *guard = ProcessState::Blocked;
                        }
                    }
                    process.data.cond.notify_all();
                    still_running.push(process);
                }
            }
        }
        processes = still_running;
    }
    Ok(())
}

/// Returns `true` if there's data in the global buffer.
pub fn input_buffer_has_data() -> bool {
    // Access the global input buffer.
    let (global_lock, global_cond) = &*GLOBAL_INPUT;
    let global_buf = global_lock.lock().unwrap();
    !global_buf.is_empty()
}
