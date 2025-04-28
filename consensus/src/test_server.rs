use std::net::{TcpListener, TcpStream};
use std::io::{Read, Write};
use std::thread;
use log::{info, error};

pub fn start_test_server() -> std::io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:8000")?;
    info!("Test server listening on 127.0.0.1:8000");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                info!("New connection from {}", stream.peer_addr()?);
                thread::spawn(move || {
                    handle_client(stream)
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
    let mut buffer = [0; 1024];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => {
                info!("Connection closed by client");
                break;
            }
            Ok(n) => {
                info!("Received {} bytes, echoing back", n);
                if let Err(e) = stream.write_all(&buffer[..n]) {
                    error!("Failed to write to client: {}", e);
                    break;
                }
            }
            Err(e) => {
                error!("Error reading from client: {}", e);
                break;
            }
        }
    }
    Ok(())
} 