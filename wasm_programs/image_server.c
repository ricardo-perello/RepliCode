#include <stdio.h>
#include <string.h>
#include <netinet/in.h>
#include <fcntl.h>
#include <unistd.h>
#include <sys/stat.h>
#include <errno.h>

// WASI socket functions
__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("sock_open")))
int sock_open(int domain, int socktype, int protocol, int* sock_fd_out);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("sock_listen")))
int sock_listen(int sock_fd, int backlog);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("sock_accept")))
int sock_accept(int sock_fd, int flags, int* sock_fd_out);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("sock_recv")))
int sock_recv(int sock_fd, void* ri_data, int ri_data_len, int ri_flags, int* ro_datalen, int* ro_flags);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("sock_send")))
int sock_send(int sock_fd, const void* si_data, int si_data_len, int si_flags, int* ret_data_len);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("sock_shutdown")))
int sock_shutdown(int sock_fd, int how);

__attribute__((import_module("wasi_snapshot_preview1")))
__attribute__((import_name("sock_close")))
int sock_close(int sock_fd);

#define BUF_SIZE 4096
#define MAX_FILENAME 256
#define MAX_CMD_SIZE 1024

// Helper function to trim whitespace and newlines from the end of a string
void trim_end(char *str) {
    int len = strlen(str);
    while (len > 0 && (str[len-1] == ' ' || str[len-1] == '\n' || str[len-1] == '\r')) {
        str[len-1] = '\0';
        len--;
    }
}

// Helper function to convert network byte order to host byte order
uint32_t ntohl(uint32_t netlong) {
    uint8_t* bytes = (uint8_t*)&netlong;
    return ((uint32_t)bytes[0] << 24) |
           ((uint32_t)bytes[1] << 16) |
           ((uint32_t)bytes[2] << 8) |
           ((uint32_t)bytes[3]);
}

void handle_client(int client_fd) {
    char cmd_buf[MAX_CMD_SIZE];
    int bytes_received;
    int bytes_sent;
    int ret;
    
    printf("[SERVER] New client connection on fd %d\n", client_fd);
    fflush(stdout);
    
    // Receive command and filename
    int total_received = 0;
    while (total_received < MAX_CMD_SIZE - 1) {
        ret = sock_recv(client_fd, cmd_buf + total_received, 1, 0, &bytes_received, NULL);
        if (ret != 0 || bytes_received == 0) {
            printf("[SERVER] Failed to receive command or client disconnected (ret=%d, bytes=%d)\n", ret, bytes_received);
            fflush(stdout);
            return;
        }
        total_received += bytes_received;
        // Stop at newline or if we've received enough data
        if (cmd_buf[total_received - 1] == '\n' || total_received >= MAX_CMD_SIZE - 1) {
            break;
        }
    }
    
    // Null terminate the command
    cmd_buf[total_received] = '\0';
    printf("[SERVER] Received command (%d bytes): %s", total_received, cmd_buf);
    fflush(stdout);

    // Parse command
    char* cmd = strtok(cmd_buf, " ");
    if (cmd == NULL) {
        printf("[SERVER] Invalid command format - no command found\n");
        fflush(stdout);
        return;
    }
    
    if (strcmp(cmd, "SEND") == 0) {
        // Handle SEND command
        char* filename = strtok(NULL, " ");
        if (filename == NULL) {
            printf("[SERVER] Missing filename for SEND command\n");
            fflush(stdout);
            return;
        }
        // Trim any whitespace or newlines from the filename
        trim_end(filename);
        printf("[SERVER] Processing SEND request for file: %s\n", filename);
        fflush(stdout);
        
        // Receive file size
        uint32_t file_size;
        ret = sock_recv(client_fd, &file_size, sizeof(file_size), 0, &bytes_received, NULL);
        if (ret != 0 || bytes_received != sizeof(file_size)) {
            printf("[SERVER] Failed to receive file size (ret=%d, bytes=%d)\n", ret, bytes_received);
            fflush(stdout);
            return;
        }
        // Convert from network byte order to host byte order
        file_size = ntohl(file_size);
        printf("[SERVER] Expecting to receive %u bytes for file %s\n", file_size, filename);
        fflush(stdout);
        
        // Create file
        int fd = open(filename, O_WRONLY | O_CREAT | O_TRUNC, 0666);
        if (fd < 0) {
            printf("[SERVER] Failed to create file %s (errno=%d)\n", filename, errno);
            fflush(stdout);
            return;
        }
        printf("[SERVER] Opened file %s for writing\n", filename);
        fflush(stdout);
        
        // Receive and write file data
        char buffer[BUF_SIZE];
        uint32_t remaining = file_size;
        uint32_t total_written = 0;
        while (remaining > 0) {
            int to_read = remaining > BUF_SIZE ? BUF_SIZE : remaining;
            ret = sock_recv(client_fd, buffer, to_read, 0, &bytes_received, NULL);
            if (ret != 0 || bytes_received <= 0) {
                printf("[SERVER] Error or disconnect while receiving file data (ret=%d, bytes=%d)\n", ret, bytes_received);
                fflush(stdout);
                close(fd);
                return;
            }
            int written = write(fd, buffer, bytes_received);
            if (written != bytes_received) {
                printf("[SERVER] Failed to write all data to file (written=%d, expected=%d)\n", written, bytes_received);
                fflush(stdout);
                close(fd);
                return;
            }
            remaining -= bytes_received;
            total_written += bytes_received;
            printf("[SERVER] Received %d bytes, %u bytes remaining (total written: %u)\n", 
                   bytes_received, remaining, total_written);
            fflush(stdout);
        }
        close(fd);
        printf("[SERVER] Finished writing file %s (%u bytes total)\n", filename, total_written);
        fflush(stdout);
        
        // Send success response
        const char* response = "OK\n";  // Just send OK
        char response_buf[4];  // Buffer for response
        strcpy(response_buf, response);  // Copy response to separate buffer
        ret = sock_send(client_fd, response_buf, strlen(response_buf), 0, &bytes_sent);
        if (ret != 0 || bytes_sent != strlen(response_buf)) {
            printf("[SERVER] Failed to send response (ret=%d, bytes=%d)\n", ret, bytes_sent);
            fflush(stdout);
            return;
        }
        printf("[SERVER] Sent response: %s", response);
        fflush(stdout);
        
        // Shutdown write side and close socket
        printf("[SERVER] Shutting down write side of socket\n");
        fflush(stdout);
        ret = sock_shutdown(client_fd, 1);  // SHUT_WR
        if (ret != 0) {
            printf("[SERVER] Failed to shutdown socket (ret=%d)\n", ret);
            fflush(stdout);
        }
        printf("[SERVER] Closing socket\n");
        fflush(stdout);
        ret = sock_close(client_fd);
        if (ret != 0) {
            printf("[SERVER] Failed to close socket (ret=%d)\n", ret);
            fflush(stdout);
        }
        return;
        
    } else if (strcmp(cmd, "GET") == 0) {
        // Handle GET command
        char* filename = strtok(NULL, " ");
        if (filename == NULL) {
            printf("[SERVER] Missing filename for GET command\n");
            fflush(stdout);
            return;
        }
        printf("[SERVER] Processing GET request for file: %s\n", filename);
        fflush(stdout);
        
        // Open file
        int fd = open(filename, O_RDONLY);
        if (fd < 0) {
            printf("[SERVER] File not found: %s (errno=%d)\n", filename, errno);
            fflush(stdout);
            const char* response = "ERROR: File not found\n";
            sock_send(client_fd, response, strlen(response), 0, &bytes_sent);
            return;
        }
        
        // Get file size
        struct stat st;
        fstat(fd, &st);
        uint32_t file_size = st.st_size;
        
        printf("[SERVER] Sending file %s of size %u bytes\n", filename, file_size);
        fflush(stdout);
        
        // Send file size in big-endian order
        uint8_t size_bytes[4];
        size_bytes[0] = (file_size >> 24) & 0xFF;  // Most significant byte
        size_bytes[1] = (file_size >> 16) & 0xFF;
        size_bytes[2] = (file_size >> 8) & 0xFF;
        size_bytes[3] = file_size & 0xFF;          // Least significant byte
        
        printf("[SERVER] Raw size bytes being sent: %02x %02x %02x %02x\n",
               size_bytes[0], size_bytes[1], size_bytes[2], size_bytes[3]);
        fflush(stdout);
        
        ret = sock_send(client_fd, size_bytes, sizeof(size_bytes), 0, &bytes_sent);
        if (ret != 0 || bytes_sent != sizeof(size_bytes)) {
            printf("[SERVER] Failed to send file size (ret=%d, bytes=%d)\n", ret, bytes_sent);
            fflush(stdout);
            close(fd);
            return;
        }
        
        // Send file data
        char buffer[BUF_SIZE];
        uint32_t remaining = file_size;
        uint32_t total_sent = 0;
        while (remaining > 0) {
            // only ever try to read as much as we know is left
            size_t to_read = remaining < BUF_SIZE ? remaining : BUF_SIZE;
            ssize_t nread = read(fd, buffer, to_read);
            if (nread < 0) {
                printf("[SERVER] read error (errno=%d)\n", errno);
                fflush(stdout);
                break;
            }
            if (nread == 0) {
                // unexpected EOF, but bail out
                printf("[SERVER] unexpected EOF after %u/%u bytes\n", total_sent, file_size);
                fflush(stdout);
                break;
            }

            int bytes_sent;
            int ret = sock_send(client_fd, buffer, nread, 0, &bytes_sent);
            if (ret != 0 || bytes_sent != nread) {
                printf("[SERVER] sock_send failed (ret=%d, sent=%d/%zd)\n", ret, bytes_sent, nread);
                fflush(stdout);
                break;
            }

            remaining -= bytes_sent;
            total_sent += bytes_sent;
            printf("[SERVER] Sent %d bytes, total %u/%u\n", bytes_sent, total_sent, file_size);
            fflush(stdout);
        }
        close(fd);
        printf("[SERVER] Finished sending file %s (%u bytes total)\n", filename, total_sent);
        fflush(stdout);
        
        // Shutdown write side and close socket after successful send
        printf("[SERVER] Shutting down write side of socket\n");
        fflush(stdout);
        ret = sock_shutdown(client_fd, 1);  // SHUT_WR
        if (ret != 0) {
            printf("[SERVER] Failed to shutdown socket (ret=%d)\n", ret);
            fflush(stdout);
        }
        printf("[SERVER] Closing socket\n");
        fflush(stdout);
        ret = sock_close(client_fd);
        if (ret != 0) {
            printf("[SERVER] Failed to close socket (ret=%d)\n", ret);
            fflush(stdout);
        }
        return;
        
    } else {
        printf("[SERVER] Unknown command: %s\n", cmd);
        fflush(stdout);
        const char* response = "ERROR: Unknown command\n";
        sock_send(client_fd, response, strlen(response), 0, &bytes_sent);
        printf("[SERVER] Shutting down write side of socket\n");
        fflush(stdout);
        ret = sock_shutdown(client_fd, 1);  // SHUT_WR
        if (ret != 0) {
            printf("[SERVER] Failed to shutdown socket (ret=%d)\n", ret);
            fflush(stdout);
        }
        printf("[SERVER] Closing socket\n");
        fflush(stdout);
        ret = sock_close(client_fd);
        if (ret != 0) {
            printf("[SERVER] Failed to close socket (ret=%d)\n", ret);
            fflush(stdout);
        }
        return;
    }
}

int main() {
    int server_fd;
    int client_fd;
    int ret;
    
    printf("[SERVER] Starting image server...\n");
    fflush(stdout);
    
    // Open a socket
    ret = sock_open(2, 1, 0, &server_fd); // AF_INET=2, SOCK_STREAM=1
    if (ret != 0) {
        printf("[SERVER] Failed to open socket (ret=%d)\n", ret);
        return 1;
    }
    printf("[SERVER] Server socket opened with fd: %d\n", server_fd);
    fflush(stdout);

    // Listen on port 7000
    ret = sock_listen(server_fd, 5); // backlog of 5
    if (ret != 0) {
        printf("[SERVER] Failed to listen on socket (ret=%d)\n", ret);
        return 1;
    }
    printf("[SERVER] Server listening on port 7000\n");
    fflush(stdout);

    // Main server loop
    while (1) {
        // Accept a connection
        ret = sock_accept(server_fd, 0, &client_fd);
        if (ret != 0) {
            printf("[SERVER] Failed to accept connection (error: %d)\n", ret);
            continue;
        }
        printf("[SERVER] Accepted connection with client fd: %d\n", client_fd);
        fflush(stdout);
        
        // Handle client
        handle_client(client_fd);
        
        // Note: No need for additional shutdown/close here since handle_client now handles it
        printf("[SERVER] Client connection closed\n");
        fflush(stdout);
    }

    return 0;
} 