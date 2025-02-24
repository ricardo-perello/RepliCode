use std::io::{self, Write};
use byteorder::{LittleEndian, WriteBytesExt};

fn main() -> io::Result<()> {
    // Print instructions to stderr.
    eprintln!("Consensus Input Tool");
    eprintln!("----------------------");
    eprintln!("This tool creates binary records with the following layout:");
    eprintln!("  [ process_id: u64 ][ msg_size: u16 ][ msg: [u8; msg_size] ]");
    eprintln!("The message must include all necessary info (e.g., \"fd:0,body:Hello World!\")");
    eprintln!("Type 'exit' at the process ID prompt to quit.\n");

    // We'll write binary records to stdout.
    let stdout = io::stdout();
    let mut output = stdout.lock();

    loop {
        eprint!("Enter Process ID: ");
        io::stderr().flush()?; // Ensure prompt appears on stderr.
        let mut pid_input = String::new();
        io::stdin().read_line(&mut pid_input)?;
        let pid_input = pid_input.trim();
        if pid_input.eq_ignore_ascii_case("exit") {
            break;
        }

        let pid: u64 = match pid_input.parse() {
            Ok(num) => num,
            Err(_) => {
                eprintln!("Invalid process ID. Please enter a valid number.");
                continue;
            }
        };

        eprint!("Enter message (include FD info, e.g., \"fd:0,body:Hello World!\"): ");
        io::stderr().flush()?;
        let mut message = String::new();
        io::stdin().read_line(&mut message)?;
        let message = message.trim();
        if message.is_empty() {
            eprintln!("Message cannot be empty.");
            continue;
        }

        let message_bytes = message.as_bytes();
        let msg_size = message_bytes.len();
        if msg_size > u16::MAX as usize {
            eprintln!("Message too long (max {} bytes).", u16::MAX);
            continue;
        }
        let msg_size_u16 = msg_size as u16;

        // Write the binary record to stdout:
        // [ process_id: u64 ][ msg_size: u16 ][ msg: [u8; msg_size] ]
        output.write_u64::<LittleEndian>(pid)?;
        output.write_u16::<LittleEndian>(msg_size_u16)?;
        output.write_all(message_bytes)?;
        output.flush()?; // Ensure the record is written immediately

        eprintln!("Record written for process {}.\n", pid);
    }

    eprintln!("Exiting Consensus Input Tool.");
    Ok(())
}
