use log::info;
use std::io;
use std::net::TcpStream;
use std::io::{Write, Read};

pub fn start_test_client() -> io::Result<()> {
    info!("Starting test client");
    
    // Connect to the NAT-mapped port
    let mut stream = TcpStream::connect("127.0.0.1:10000")?;
    info!("Connected to test server");

    // Send a test message
    let message = b"Hello from test client!";
    stream.write_all(message)?;
    info!("Sent test message");

    // Read response
    let mut buffer = [0; 1024];
    let bytes_read = stream.read(&mut buffer)?;
    let response = String::from_utf8_lossy(&buffer[..bytes_read]);
    info!("Received response: {}", response);

    Ok(())
} 