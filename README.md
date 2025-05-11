# **RepliCode**  
**POSIX-Compliant Deterministic Execution for WebAssembly**  

RepliCode is a **deterministic WebAssembly runtime** built in **Rust** that supports **POSIX-like I/O operations** and **replicated execution**. The goal is to enable **server applications** (e.g., web servers, databases) to run deterministically across multiple nodes, ensuring **fault tolerance** and **consistent execution** in a high-throughput blockchain environment.  

This project integrates **Wasmtime**, a WebAssembly runtime, and extends it with **system call handling, file operations, networking, and deterministic scheduling** to ensure identical execution across nodes.  

---

## **Project Overview**  

### **Key Features**  
- **Custom WASM runtime in Rust** for executing POSIX-like applications  
- **Deterministic I/O**: File operations, sockets, and system calls behave consistently across all nodes  
- **Replicated Execution**: All nodes execute the same state transitions in lockstep  
- **Integration with Consensus Mechanisms** to ensure verifiable execution  
- **Network Layer**: Deterministic socket operations and connection management
- **Future Work**: Support for **multi-threading and advanced filesystem access** in a deterministic manner  

---

## **Technical Stack**  
- **Programming Language:** Rust  
- **Runtime:** Wasmtime  
- **WebAssembly Compilation:** Clang + WASI SDK  
- **Target Environment:** Linux/macOS  
- **Consensus Mechanism:** Blockchain-based replication  

---

## **Project Structure**
```
RepliCode/
│── consensus/            # Consensus layer implementation
│   ├── src/             # Consensus source files
│   ├── Cargo.toml       # Consensus package configuration
│   └── consensus_input.bin  # Consensus input data
│── runtime/             # Rust runtime implementation
│   ├── src/             # Runtime source files
│   └── Cargo.toml       # Runtime package configuration
│── wasi_sandbox/        # WASI sandbox environment
│   ├── pid_2/          # Process-specific sandbox
│   └── standard1/      # Standard sandbox configuration
│── wasm_programs/       # C programs compiled to WASM
│   ├── hello.c         # Sample C program for testing
│   ├── Makefile        # C to WASM build automation
│   └── build/          # Compiled WASM binaries
│── docs/               # Documentation
│   ├── design.md       # Design decisions and architecture
│   └── research.md     # Notes on deterministic execution  
│── Cargo.toml          # Root package configuration
│── Cargo.lock          # Dependency lock file
│── .gitignore         # Git ignore rules
│── README.md          # Project overview
```

---

## **Installation & Setup**  

### **1️⃣ Install Dependencies**  
#### **MacOS (via Homebrew)**  
```sh
brew install rust wasmtime wasi-sdk llvm
cargo install wasmtime-cli
```
#### **Ubuntu (Linux)**  
```sh
sudo apt update && sudo apt install -y clang lld wasi-sdk
cargo install wasmtime-cli
```

---

### **2️⃣ Compile a Sample C Program to WASM**
```sh
cd wasm_programs
make
```
This generates `build/hello.wasm`.

---

### **3️⃣ Run the WASM Program in RepliCode**

First, start the consensus layer:
```sh
cargo run --bin consensus tcp
```

Then, in separate terminals, start multiple runtime instances:
```sh
# Terminal 1
cargo run --bin runtime tcp

# Terminal 2
cargo run --bin runtime tcp

# Terminal 3 (and so on...)
cargo run --bin runtime tcp
```

This will execute the WASM program inside the RepliCode runtime with multiple replicas.

---

## **Development Status**
### **Phase 1: Core Runtime (Completed)**
✅ Execute a C program compiled to WASM  
✅ Build a minimal Rust-based WASM runtime  
✅ Capture and handle system calls  
✅ Basic consensus layer implementation

### **Phase 2: Extended POSIX Support (Completed)**  
✅ Implement deterministic file I/O  
✅ Integrate socket-based communication  
✅ Develop a replicated system call layer  
✅ TCP-based consensus communication


### **Phase 3: Multi-Threading & Networking (Planned)**  
🔲 Support for POSIX threads (`pthreads`)  
🔲 Advanced network stack integration  
🔲 Performance optimizations  
🔲 Fault tolerance mechanisms

---

## **Network Architecture**

### **Key Components**
- **NAT Table**: Manages port mappings and connection state
- **Socket Operations**: Implements WASI socket functions
- **Network Batching**: Handles message batching for consensus
- **Process Isolation**: Each process has its own network namespace

### **Socket Functions**
The runtime implements the following WASI socket functions:

#### **Socket Creation & Management**
```rust
wasi_sock_open(domain, socktype, protocol) -> fd
wasi_sock_close(fd) -> status
wasi_sock_shutdown(fd, how) -> status
```

#### **Connection Operations**
```rust
wasi_sock_listen(fd, backlog) -> status
wasi_sock_accept(fd, flags) -> new_fd
wasi_sock_connect(fd, addr, addr_len) -> status
```

#### **Data Transfer**
```rust
wasi_sock_send(fd, data, flags) -> bytes_sent
wasi_sock_recv(fd, buffer, flags) -> bytes_received
```

### **Network Operation Flow**
1. **Socket Creation**
   - Process requests new socket via `wasi_sock_open`
   - Runtime allocates local port and FD
   - Socket starts in unconnected state

2. **Listening**
   - Process calls `wasi_sock_listen`
   - Runtime marks socket as listener
   - NAT table creates port mapping

3. **Accepting Connections**
   - Process calls `wasi_sock_accept`
   - Runtime preallocates new FD and port
   - Connection establishment with proper error handling

4. **Data Transfer**
   - Sending: Operations are queued and batched
   - Receiving: Data routed through consensus layer
   - Proper error handling for connection states

### **Error Handling**
Common error codes:
- `EINVAL` (1): Invalid arguments
- `EAGAIN` (11): Resource temporarily unavailable
- `EMFILE` (76): Too many open files

---

## **Contributing**
RepliCode is under active development. Contributions in system architecture, Rust development, and deterministic execution research are welcome. Open an issue or submit a pull request.  

---

## **License**
This project is released under the **MIT License**.  

---

### **✅ Commit & Push**
```sh
git add README.md
git commit -m "docs: initial project documentation"
git push origin main
```

