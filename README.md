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
- **Future Work**: Support for **multi-threading, networking, and filesystem access** in a deterministic manner  

---

## **Technical Stack**  
- **Programming Language:** Rust  
- **Runtime:** Wasmtime  
- **WebAssembly Compilation:** Clang + WASI SDK  
- **Target Environment:** Linux/macOS  
- **Consensus Mechanism (Future Work):** Blockchain-based replication  

---

## **Project Structure**
```
RepliCode/
│── runtime/              # Rust runtime implementation
│   ├── src/              # Rust source files
│   ├── Cargo.toml        # Rust package configuration
│   ├── main.rs           # Entry point for the runtime
│── wasm_programs/        # C programs compiled to WASM
│   ├── hello.c           # Sample C program for testing
│   ├── Makefile          # C to WASM build automation
│   ├── build/            # Compiled WASM binaries
│── docs/                 # Documentation
│   ├── design.md         # Design decisions and architecture
│   ├── research.md       # Notes on deterministic execution  
│── tests/                # Test cases for runtime validation
│── .gitignore            # Ignore compiled artifacts
│── README.md             # Project overview
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
```sh
cd runtime
cargo run
```
This executes `hello.wasm` inside the RepliCode runtime.

---

## **Development Roadmap**
### **Phase 1: Core Runtime (In Progress)**
✅ Execute a C program compiled to WASM  
✅ Build a minimal Rust-based WASM runtime  
✅ Capture and handle system calls  

### **Phase 2: Extended POSIX Support**  
🔲 Implement deterministic file I/O  
🔲 Integrate socket-based communication  
🔲 Develop a replicated system call layer  

### **Phase 3: Multi-Threading & Networking**  
🔲 Support for POSIX threads (`pthreads`)  
🔲 Network stack integration  
🔲 Performance optimizations  

---

## **References & Documentation**
- [Rust Programming Language](https://www.rust-lang.org/)  
- [Wasmtime WebAssembly Runtime](https://wasmtime.dev/)  
- [WebAssembly and WASI](https://webassembly.org/)  
- [POSIX Standard](https://pubs.opengroup.org/onlinepubs/9699919799/)  

---

## **Networking & Socket Implementation**

### **Network Architecture**
RepliCode implements a deterministic networking layer that ensures consistent behavior across all nodes. The system uses a NAT (Network Address Translation) table to manage connections and port mappings.

#### **Key Components**
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

### **NAT Table Implementation**
The NAT table (`NatTable`) manages:
- Port mappings between processes
- Pending accept operations
- Connection state tracking
- Network operation queuing

```rust
struct NatTable {
    port_mappings: HashMap<(u64, u16), u16>,  // (pid, src_port) -> consensus_port
    process_ports: HashMap<(u64, u16), u16>,  // (pid, src_port) -> consensus_port
    listeners: HashMap<(u64, u16), NatListener>,
    pending_accepts: HashMap<(u64, u16), bool>,
}
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
   - Runtime preallocates new FD and port for the connection
   - If accept succeeds:
     - New socket is created with preallocated port
     - Connection is established
   - If accept fails:
     - Preallocated FD is freed
     - Port counter is reverted
     - Returns EAGAIN for retry

4. **Sending Data**
   - Process calls `wasi_sock_send`
   - Runtime queues operation
   - Consensus processes and routes data

5. **Receiving Data**
   - Data arrives via consensus
   - Runtime routes to correct socket based on port mapping
   - Process reads via `wasi_sock_recv`

### **Port Management**
The system implements deterministic port allocation:
- Each process maintains its own port counter
- Ports are preallocated for accept operations
- Failed accepts trigger port counter reversion
- Port mappings ensure consistent routing across nodes

### **Message Batching**
The system uses a batching mechanism for network operations:

1. **Outgoing Messages**
   - Network operations are queued
   - Batched with other operations
   - Sent to consensus in batches

2. **Incoming Messages**
   - Consensus sends batched responses
   - Runtime processes each message
   - Updates appropriate socket buffers

### **Error Handling**
Common error codes:
- `EINVAL` (1): Invalid arguments
- `EAGAIN` (11): Resource temporarily unavailable
- `EMFILE` (76): Too many open files

### **Deterministic Execution**
The networking layer ensures determinism by:
- Consistent port allocation
- Ordered message processing
- Synchronized state updates
- Atomic operation handling

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

