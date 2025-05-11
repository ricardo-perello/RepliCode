use std::env;
use std::io::{self, BufRead, Write, Read};
use std::net::TcpStream;

fn usage() {
    eprintln!("Usage: kv_client <host> <port>");
    std::process::exit(1);
}

pub fn start_kv_client() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        usage();
    }
    let host = &args[1];
    let port = &args[2];
    let addr = format!("{}:{}", host, port);

    let mut stream = TcpStream::connect(&addr)?;
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        stream.write_all(line.as_bytes())?;
        stream.write_all(b"\n")?;
        stream.flush()?;
        let mut response = String::new();
        stream.read_to_string(&mut response)?;
        write!(stdout, "{}", response)?;
        stdout.flush()?;
    }
    Ok(())
} 