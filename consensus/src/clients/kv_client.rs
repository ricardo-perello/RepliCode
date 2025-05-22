use std::env;
use std::io::{self, BufRead, Write, Read, BufReader};
use std::net::TcpStream;
use std::time::Duration;

const BUF_SIZE: usize = 4096;

fn usage() {
    eprintln!("Usage: consensus kv-client <host> <port>");
    std::process::exit(1);
}

fn send_command(stream: &mut TcpStream, command: &str) -> io::Result<String> {
    println!("[CLIENT] Sending command: {}", command);
    
    // Ensure command is properly formatted
    let formatted_command = if command.to_lowercase().starts_with("set ") {
        // For SET commands, ensure space between key and value
        let parts: Vec<&str> = command.splitn(3, ' ').collect();
        if parts.len() >= 3 {
            format!("SET {} {}", parts[1], parts[2])
        } else {
            command.to_string()
        }
    } else if command.to_lowercase().starts_with("get ") {
        // Parse GET command (needs at least 2 parts)
        let parts: Vec<&str> = command.splitn(2, ' ').collect();
        if parts.len() >= 2 {
            format!("GET {}", parts[1])
        } else {
            command.to_string()
        }
    } else if command.to_lowercase().starts_with("del ") {
        // Parse DEL command (needs at least 2 parts)
        let parts: Vec<&str> = command.splitn(2, ' ').collect();
        if parts.len() >= 2 {
            format!("DEL {}", parts[1])
        } else {
            command.to_string()
        }
    } else {
        command.to_string()
    };
    
    println!("[CLIENT] Formatted command: {}", formatted_command);
    
    // Send command with newline
    writeln!(stream, "{}", formatted_command)?;
    stream.flush()?;
    
    // Read response into a buffer, one byte at a time until newline
    let mut response = Vec::new();
    let mut buf = [0u8; 1];
    
    println!("[CLIENT] Waiting for response...");
    
    loop {
        match stream.read_exact(&mut buf) {
            Ok(_) => {
                response.push(buf[0]);
                if buf[0] == b'\n' {
                    break;
                }
            },
            Err(e) => {
                println!("[CLIENT] Error reading response: {}", e);
                return Err(e);
            }
        }
    }
    
    // Convert to string
    let response_str = String::from_utf8_lossy(&response).to_string();
    println!("[CLIENT] Received response: {}", response_str.trim());
    
    Ok(response_str)
}

pub fn start_kv_client() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();
    // We expect: binary_name kv-client host port
    if args.len() != 4 {
        usage();
    }
    let host = &args[2];
    let port = &args[3];
    let addr = format!("{}:{}", host, port);

    println!("[CLIENT] Connecting to {}...", addr);
    let mut stream = TcpStream::connect(&addr)?;
    
    // Set socket options
    stream.set_nodelay(true)?; // Disable Nagle's algorithm
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    
    println!("[CLIENT] Connected to server.");
    
    // Create a buffered reader for stdin
    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut input = String::new();
    
    println!("Available commands:");
    println!("  set <key> <value> - Store a key-value pair");
    println!("  get <key>        - Retrieve a value by key");
    println!("  del <key>        - Delete a key-value pair");
    println!("  quit             - Exit the client");
    
    loop {
        // Clear the input buffer
        input.clear();
        
        // Read a line from stdin
        print!("> ");
        io::stdout().flush()?;
        
        match reader.read_line(&mut input) {
            Ok(_) => {
                // Trim the newline and check if empty
                let trimmed = input.trim();
                if trimmed.is_empty() {
                    continue;
                }
                
                // If the command is "quit", exit the loop
                if trimmed.eq_ignore_ascii_case("quit") {
                    println!("[CLIENT] Sending QUIT command");
                    match send_command(&mut stream, "QUIT") {
                        Ok(response) => print!("Server response: {}", response),
                        Err(e) => eprintln!("Error: {}", e),
                    }
                    break;
                }
                
                // Send the command and get response
                match send_command(&mut stream, trimmed) {
                    Ok(response) => print!("Server response: {}", response),
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        break;
                    }
                }
                
                // After each command, reconnect for the next one
                stream = TcpStream::connect(&addr)?;
                stream.set_nodelay(true)?;
                stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                stream.set_write_timeout(Some(Duration::from_secs(5)))?;
                println!("[CLIENT] Reconnected for next command");
            },
            Err(e) => {
                eprintln!("Error reading input: {}", e);
                break;
            }
        }
    }
    
    println!("Disconnected from server.");
    Ok(())
} 