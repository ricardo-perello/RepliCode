use anyhow::Result;
use crate::runtime::process::{Process, ProcessState};
use std::thread::sleep;
use std::time::Duration;

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
                    // // For demonstration we “unblock” a blocked process.
                    // {
                    //     let mut guard = process.data.state.lock().unwrap();
                    //     if let ProcessState::Blocked = *guard {
                    //         println!("Unblocking process from scheduler");
                    //         *guard = ProcessState::Running;
                    //     }
                    // }
                    // process.data.cond.notify_all();
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