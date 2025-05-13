use std::net::{TcpListener, TcpStream};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::thread;
use log::{info, error};
use serde_json::json;
use crate::nat::NatTable;

pub struct HttpServer {
    nat_table: Arc<Mutex<NatTable>>,
}

impl HttpServer {
    pub fn new(nat_table: Arc<Mutex<NatTable>>) -> Self {
        HttpServer { nat_table }
    }

    pub fn start(&self, port: u16) -> std::io::Result<()> {
        let listener = TcpListener::bind(format!("127.0.0.1:{}", port))?;
        info!("HTTP server listening on 127.0.0.1:{}", port);

        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let nat_table = Arc::clone(&self.nat_table);
                    thread::spawn(move || {
                        if let Err(e) = Self::handle_client(stream, nat_table) {
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

    fn handle_client(mut stream: TcpStream, nat_table: Arc<Mutex<NatTable>>) -> std::io::Result<()> {
        let mut buffer = [0; 1024];
        let n = stream.read(&mut buffer)?;
        let request = String::from_utf8_lossy(&buffer[..n]);
        
        // Parse the request path
        let path = request.lines().next().unwrap_or("").split_whitespace().nth(1).unwrap_or("/");
        
        // Generate response based on path
        let response = match path {
            "/status" => {
                let nat_table = nat_table.lock().unwrap();
                let status = json!({
                    "processes": nat_table.get_process_info(),
                    "connections": nat_table.get_connection_info(),
                    "listeners": nat_table.get_listener_info()
                });
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    status.to_string().len(),
                    status
                )
            }
            _ => {
                "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_string()
            }
        };

        stream.write_all(response.as_bytes())?;
        stream.flush()?;
        Ok(())
    }
} 