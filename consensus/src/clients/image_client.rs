use std::env;
use std::io::{self, Read, Write};
use std::net::TcpStream;

fn usage() {
    eprintln!("Usage: image_client <host> <port> <SEND|GET> <filename>");
    std::process::exit(1);
}

pub fn start_image_client() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 5 {
        usage();
    }
    let host = &args[1];
    let port = &args[2];
    let _cmd = &args[3];
    let _filename = &args[4];
    let addr = format!("{}:{}", host, port);

    let mut stream = TcpStream::connect(&addr)?;
    let mut stream_clone = stream.try_clone()?;

    // Spawn a thread to read from stdin and send to the socket
    let stdin_handle = std::thread::spawn(move || {
        let mut stdin = io::stdin();
        let mut buffer = [0u8; 4096];
        loop {
            let n = match stdin.read(&mut buffer) {
                Ok(0) => break, // EOF
                Ok(n) => n,
                Err(_) => break,
            };
            if stream.write_all(&buffer[..n]).is_err() {
                break;
            }
        }
    });

    // Main thread: read from socket and write to stdout
    let mut stdout = io::stdout();
    let mut buffer = [0u8; 4096];
    loop {
        let n = match stream_clone.read(&mut buffer) {
            Ok(0) => break, // Connection closed
            Ok(n) => n,
            Err(_) => break,
        };
        if stdout.write_all(&buffer[..n]).is_err() {
            break;
        }
        stdout.flush().ok();
    }

    stdin_handle.join().ok();
    Ok(())
} 