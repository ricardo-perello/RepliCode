use log::info;
use std::env;
use std::fs::File;
use std::io::{self, Read, Write, BufRead, BufReader};
use std::net::TcpStream;
use std::path::Path;

const BUF_SIZE: usize = 4096;

fn usage() {
    eprintln!("Usage: consensus image-client <host> <port>");
    std::process::exit(1);
}

fn send_file(stream: &mut TcpStream, filename: &str) -> io::Result<()> {
    // Open the file
    let mut file = File::open(filename)?;
    let file_size = file.metadata()?.len() as u32;
    println!("[CLIENT] Sending file '{}' ({} bytes)", filename, file_size);
    
    // Send command
    writeln!(stream, "SEND {}", filename)?;
    
    // Send file size
    stream.write_all(&file_size.to_be_bytes())?;
    stream.flush()?;
    
    // Send file data
    let mut buffer = [0u8; BUF_SIZE];
    let mut total_sent: u32 = 0u32;
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        stream.write_all(&buffer[..n])?;
        total_sent += n as u32;
        println!("[CLIENT] Sent {} bytes, {} bytes remaining", n, file_size.saturating_sub(total_sent));
    }
    println!("[CLIENT] Finished sending file '{}'. Total bytes sent: {}", filename, total_sent);
    
    // Read response
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    println!("Server response: {}", response);
    
    Ok(())
}

fn get_file(stream: &mut TcpStream, filename: &str) -> io::Result<()> {
    // Send command
    writeln!(stream, "GET {}", filename)?;
    stream.flush()?;
    
    // Read file size
    let mut size_buf = [0u8; 4];
    stream.read_exact(&mut size_buf)?;
    println!("[CLIENT] Raw size bytes: {:02x} {:02x} {:02x} {:02x}", 
             size_buf[0], size_buf[1], size_buf[2], size_buf[3]);
    let file_size = u32::from_be_bytes(size_buf);
    println!("[CLIENT] Expecting to receive file '{}' ({} bytes)", filename, file_size);
    
    if file_size == 0 {
        println!("File not found on server");
        return Ok(());
    }
    
    // Create output file
    let mut file = File::create(filename)?;
    println!("[CLIENT] Opened file '{}' for writing", filename);
    
    // Read and write file data
    let mut buffer = [0u8; BUF_SIZE];
    let mut remaining = file_size as usize;
    let mut total_received = 0usize;
    
    while remaining > 0 {
        let to_read = std::cmp::min(remaining, BUF_SIZE);
        let n = stream.read(&mut buffer[..to_read])?;
        if n == 0 {
            break;
        }
        file.write_all(&buffer[..n])?;
        remaining -= n;
        total_received += n;
        println!("[CLIENT] Received {} bytes, {} bytes remaining", n, file_size.saturating_sub(total_received as u32));
    }
    
    // Flush the file to ensure all data is written
    file.flush()?;
    println!("[CLIENT] Finished receiving file '{}'. Total bytes received: {}", filename, total_received);
    
    // Send acknowledgment to server
    stream.write_all(b"OK\n")?;
    stream.flush()?;
    
    // Wait for server to shutdown write side
    let mut shutdown_buf = [0u8; 1];
    match stream.read(&mut shutdown_buf) {
        Ok(0) => println!("[CLIENT] Server closed connection normally"),
        Ok(_) => println!("[CLIENT] Unexpected data after file transfer"),
        Err(e) => println!("[CLIENT] Error reading after file transfer: {}", e),
    }
    
    Ok(())
}

pub fn start_image_client() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();
    // Skip the first two arguments (binary name and "image-client")
    if args.len() != 4 {
        usage();
    }
    
    let host = &args[2];
    let port = &args[3];
    let addr = format!("{}:{}", host, port);
    
    info!("Connecting to {}", addr);
    let mut stream = TcpStream::connect(&addr)?;
    info!("Connected to server");
    
    // Create a buffered reader for stdin
    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut input = String::new();
    
    println!("Connected to server. Available commands:");
    println!("  send <filename> - Send a file to the server");
    println!("  get <filename>  - Get a file from the server");
    println!("  quit           - Exit the client");
    
    loop {
        // Clear the input buffer
        input.clear();
        
        // Read a line from stdin
        print!("> ");
        io::stdout().flush()?;
        
        match reader.read_line(&mut input) {
            Ok(_) => {
                // Trim the newline and split the command
                let parts: Vec<&str> = input.trim().split_whitespace().collect();
                
                if parts.is_empty() {
                    continue;
                }
                
                match parts[0] {
                    "quit" => break,
                    "send" => {
                        if parts.len() != 2 {
                            println!("Usage: send <filename>");
                            continue;
                        }
                        if let Err(e) = send_file(&mut stream, parts[1]) {
                            eprintln!("Error sending file: {}", e);
                        }
                    }
                    "get" => {
                        if parts.len() != 2 {
                            println!("Usage: get <filename>");
                            continue;
                        }
                        if let Err(e) = get_file(&mut stream, parts[1]) {
                            eprintln!("Error getting file: {}", e);
                        }
                    }
                    _ => println!("Unknown command. Available commands: send, get, quit"),
                }
            }
            Err(e) => {
                eprintln!("Error reading input: {}", e);
                break;
            }
        }
    }
    
    Ok(())
} 