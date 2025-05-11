use std::env;
use std::io::{self, Read, Write};
use std::net::TcpStream;

fn usage() {
    eprintln!("Usage: dircopy_client <host> <port> <src> <dst>");
    std::process::exit(1);
}

pub fn start_dircopy_client() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 5 {
        usage();
    }
    let host = &args[1];
    let port = &args[2];
    let src = &args[3];
    let dst = &args[4];
    let addr = format!("{}:{}", host, port);

    let mut stream = TcpStream::connect(&addr)?;
    let cmd = format!("COPY {} {}\n", src, dst);
    stream.write_all(cmd.as_bytes())?;
    stream.flush()?;

    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    print!("{}", response);
    Ok(())
} 