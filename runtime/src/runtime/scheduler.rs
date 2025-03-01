use anyhow::Result;
use crate::{consensus_input::process_consensus_file, runtime::process::{Process, ProcessState}};
use crate::runtime::clock::GlobalClock;

use super::process::BlockReason;

pub fn run_scheduler(mut processes: Vec<Process>) -> Result<()> {
    while !processes.is_empty() {
        // 1. If all processes are blocked, try loading more input (consensus) or break.
        let unblocked_count = processes
            .iter()
            .filter(|p| {
                let state = p.data.state.lock().unwrap();
                *state != ProcessState::Blocked
            })
            .count();
        if unblocked_count == 0 {
            println!("all processes blocked");
            // All processes are blocked -> attempt to read more input
            let _ = process_consensus_file("../consensus/consensus_input.bin", &mut processes);
        }

        // 2. We'll do one pass over the processes to handle finishing/blocking states.
        let mut found_running = false;  // did we see a process that's *already* Running?
        let mut next_round = Vec::with_capacity(processes.len());

        for process in processes {
            let state_copy = {
                let guard = process.data.state.lock().unwrap();
                *guard
            };

            match state_copy {
                ProcessState::Finished => {
                    // The thread ended; join and discard it.
                    let _ = process.thread.join();
                }
                ProcessState::Blocked => {
                    // Possibly un-block if FD input or timed out
                    let reason = {
                        let r = process.data.block_reason.lock().unwrap();
                        r.clone()
                    };
                    match reason {
                        Some(BlockReason::StdinRead) => {
                            let fd_has_input = {
                                let fd_table = process.data.fd_table.lock().unwrap();
                                fd_table.has_pending_input(0)
                            };
                            if fd_has_input {
                                let mut st = process.data.state.lock().unwrap();
                                *st = ProcessState::Running;
                                *process.data.block_reason.lock().unwrap() = None;
                                process.data.cond.notify_all();
                                found_running = true;
                            }
                        }
                        Some(BlockReason::Timeout { resume_after }) => {
                            if GlobalClock::now() >= resume_after {
                                let mut st = process.data.state.lock().unwrap();
                                *st = ProcessState::Running;
                                *process.data.block_reason.lock().unwrap() = None;
                                process.data.cond.notify_all();
                                found_running = true;
                            }
                        }
                        None => {}
                    }
                    // Keep it for next round whether or not it was unblocked
                    next_round.push(process);
                }
                ProcessState::Running => {
                    // We already have a Running process.
                    found_running = true;
                    next_round.push(process);
                }
                ProcessState::Ready => {
                    // It's Ready but not Running yet. We won't promote it immediately
                    // if we already have something Running. We'll do that after this pass
                    // if we find no Running process at all.
                    next_round.push(process);
                }
            }
        }

        // 3. If we have *no* Running processes but some are Ready, promote exactly one.
        if !found_running {
            for process in &next_round {
                let mut st = process.data.state.lock().unwrap();
                if *st == ProcessState::Ready {
                    *st = ProcessState::Running;
                    process.data.cond.notify_all();
                    println!("Promoting a Ready process to Running");
                    break; // only promote one
                }
            }
        }

        // 4. Next iteration
        processes = next_round;
    }

    Ok(())
}

