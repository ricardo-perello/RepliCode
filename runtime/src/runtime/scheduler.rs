use anyhow::Result;
use wasmtime::Store;
use wasmtime::Trap;
use crate::runtime::process::Process;

pub fn run_scheduler(mut processes: Vec<Process>, store: &mut Store<()>) -> Result<()> {
    // Continue scheduling until there are no more processes
    while !processes.is_empty() {
        let mut still_running = Vec::new();

        for process in processes {
            let start = process
                .instance
                .get_typed_func::<(), ()>(&mut *store, "_start")?;

            match start.call(&mut *store, ()) {
                Ok(()) => {
                    // Process finished its work.
                    println!("Process finished");
                    let remainingGas = store.get_fuel().unwrap();
                    println!("gas left ${remainingGas}");
                }
                Err(e) => {
                    // Attempt to downcast the error to a Wasmtime Trap
                    if let Some(trap) = e.downcast_ref::<Trap>() {
                        let _ = store.set_fuel(20_000)?;
                        // Check the trap message
                        let trapmsg = trap.to_string();
                        if trapmsg == "block" {
                            println!("Process blocked");
                            still_running.push(process);
                        } else if trapmsg.contains("fuel") {
                            println!("Process ran out of fuel!");
                            still_running.push(process);
                        }
                    }
                }
            }
        }
        processes = still_running;
    }
    Ok(())
}
