use anyhow::Result;
use crate::{consensus_input::process_consensus_file, runtime::process::{Process, ProcessState}};
use crate::runtime::clock::GlobalClock;

use super::process::BlockReason;

pub fn run_scheduler(mut processes: Vec<Process>) -> Result<()> {
    while !processes.is_empty() {
        let unblocked = processes.iter().filter(|p| {
            let state = p.data.state.lock().unwrap();
            *state != ProcessState::Blocked
        }).count();
        if unblocked == 0 {
            // All processes are blocked.
            let _ = process_consensus_file("../consensus/consensus_input.bin", &mut processes);
        }
        let mut still_running = Vec::new();
        for process in processes {
            let state_copy = {
                let guard = process.data.state.lock().unwrap();
                *guard
            };
            match state_copy {
                ProcessState::Finished => {
                    let _ = process.thread.join();
                }
                ProcessState::Blocked => {
                    let reason = {
                        let reason_guard = process.data.block_reason.lock().unwrap();
                        reason_guard.clone()
                    };

                    // Instead of checking a global buffer, inspect the process's FD table.
                    if let Some(BlockReason::StdinRead) = reason {
                        let fd_has_input = {
                            let fd_table = process.data.fd_table.lock().unwrap();
                            fd_table.has_pending_input(0)  // FD 0 (stdin) check
                        };

                        if fd_has_input {
                            let mut st = process.data.state.lock().unwrap();
                            *st = ProcessState::Running;
                            let mut reason = process.data.block_reason.lock().unwrap();
                            *reason = None;
                            process.data.cond.notify_all();
                        }
                    } else if let Some(BlockReason::Timeout { resume_after }) = reason {
                        if GlobalClock::now() >= resume_after {
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
                    still_running.push(process);
                }
                ProcessState::Ready => {
                    {
                        let mut guard = process.data.state.lock().unwrap();
                        *guard = ProcessState::Running;
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
