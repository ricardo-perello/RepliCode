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

    // Send a test message
    let message = "Hello from test client!\n";
    if let Err(e) = stream.write_all(message.as_bytes()) {
        eprintln!("Failed to send message: {}", e);
        return;
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
        }
    }
} 