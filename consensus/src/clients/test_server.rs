use std::net::{TcpListener, TcpStream};
use std::io::{Read, Write};
use std::thread;
use log::{info, error, debug};

pub fn start_test_server() -> std::io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:8000")?;
    info!("Test server listening on 127.0.0.1:8000");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                info!("New connection from {}", stream.peer_addr()?);
                thread::spawn(move || {
                    if let Err(e) = handle_client(stream) {
                        error!("Error handling client: {}", e);
                    }
                });
            }
            Err(e) => {
                error!("Failed to accept connection: {}", e);
            }
        }
    }
    Ok(())
}

fn handle_client(mut stream: TcpStream) -> std::io::Result<()> {
    info!("Starting client handler for connection from {}", stream.peer_addr()?);
    
    // Try setting the stream to non-blocking mode
    if let Err(e) = stream.set_nonblocking(false) {
        error!("Failed to set stream to blocking mode: {}", e);
        // Continue anyway
    }
    
    let mut buffer = [0; 1024];
    loop {
        debug!("Waiting to read data from client...");
        match stream.read(&mut buffer) {
            Ok(0) => {
                info!("Connection closed by client");
                break;
            }
            Ok(n) => {
                let data = &buffer[..n];
                info!("Received {} bytes: {:?}", n, String::from_utf8_lossy(data));
                debug!("Raw data: {:?}", data);
                
                info!("Echoing back {} bytes to client", n);
                match stream.write_all(&buffer[..n]) {
                    Ok(_) => {
                        if let Err(e) = stream.flush() {
                            error!("Failed to flush response: {}", e);
                        } else {
                            debug!("Successfully flushed response");
                        }
                    },
                    Err(e) => {
                        error!("Failed to write to client: {}", e);
                        break;
                    }
                }
            }
            Err(e) => {
                error!("Error reading from client: {}", e);
                break;
            }
        }
    }
    info!("Client handler finished");
    Ok(())
} 