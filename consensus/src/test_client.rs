use log::info;
use std::io;
use std::net::TcpStream;
use std::io::{Write, Read, BufRead, BufReader};

pub fn run_test_client() {
    info!("Starting test client");
    
    // Connect to the test server
    let mut stream = match TcpStream::connect("127.0.0.1:10000") {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to connect to server: {}", e);
            return;
        }
    };
    info!("Connected to test server");

    // Create a buffered reader for stdin
    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut input = String::new();

    loop {
        // Clear the input buffer
        input.clear();
        
        // Read a line from stdin
        print!("Enter message (or 'quit' to exit): ");
        io::stdout().flush().unwrap();
        
        match reader.read_line(&mut input) {
            Ok(_) => {
                // Trim the newline
                let message = input.trim();
                
                // Check if user wants to quit
                if message == "quit" {
                    break;
                }

                // Send the message
                if let Err(e) = writeln!(stream, "{}", message) {
                    eprintln!("Failed to send message: {}", e);
                    break;
                }
                info!("Sent message to server");

                // Read response
                let mut buffer = [0; 1024];
                match stream.read(&mut buffer) {
                    Ok(n) => {
                        let response = String::from_utf8_lossy(&buffer[..n]);
                        println!("Received response: {}", response);
                    }
                    Err(e) => {
                        eprintln!("Failed to read response: {}", e);
                        break;
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to read input: {}", e);
                break;
            }
        }
    }
} 