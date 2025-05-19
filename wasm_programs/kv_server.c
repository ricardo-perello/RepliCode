#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
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
#define MAX_KEY 128
#define MAX_VAL 1024
#define MAX_CMD_SIZE 1024
#define KV_DIR "/tmp/"

// Simple in-memory key-value store (fixed size)
#define MAX_ENTRIES 100
char keys[MAX_ENTRIES][MAX_KEY];
char values[MAX_ENTRIES][MAX_VAL];
int num_entries = 0;

void handle_client(int client_fd);

// In-memory implementation
int set_key(const char* key, const char* value) {
    printf("[SERVER] Setting key '%s' to value '%s'\n", key, value);
    fflush(stdout);
    
    // Check if key already exists
    for (int i = 0; i < num_entries; i++) {
        if (strcmp(keys[i], key) == 0) {
            // Update existing value
            strncpy(values[i], value, MAX_VAL-1);
            values[i][MAX_VAL-1] = '\0';
            printf("[SERVER] Updated existing key '%s'\n", key);
            fflush(stdout);
            return 0;
        }
    }
    
    // Add new key if space available
    if (num_entries < MAX_ENTRIES) {
        strncpy(keys[num_entries], key, MAX_KEY-1);
        keys[num_entries][MAX_KEY-1] = '\0';
        
        strncpy(values[num_entries], value, MAX_VAL-1);
        values[num_entries][MAX_VAL-1] = '\0';
        
        num_entries++;
        printf("[SERVER] Added new key '%s', total entries: %d\n", key, num_entries);
        fflush(stdout);
        return 0;
    }
    
    printf("[SERVER] No space for new key (limit: %d entries)\n", MAX_ENTRIES);
    fflush(stdout);
    return -1;
}

int get_key(const char* key, char* value_out, int maxlen) {
    printf("[SERVER] Getting value for key '%s'\n", key);
    fflush(stdout);
    
    for (int i = 0; i < num_entries; i++) {
        if (strcmp(keys[i], key) == 0) {
            strncpy(value_out, values[i], maxlen-1);
            value_out[maxlen-1] = '\0';
            printf("[SERVER] Found key '%s', value: '%s'\n", key, value_out);
            fflush(stdout);
            return 0;
        }
    }
    
    printf("[SERVER] Key '%s' not found\n", key);
    fflush(stdout);
    return -1;
}

int del_key(const char* key) {
    printf("[SERVER] Deleting key '%s'\n", key);
    fflush(stdout);
    
    for (int i = 0; i < num_entries; i++) {
        if (strcmp(keys[i], key) == 0) {
            // Move last entry to this position (if not the last one)
            if (i < num_entries - 1) {
                strcpy(keys[i], keys[num_entries-1]);
                strcpy(values[i], values[num_entries-1]);
            }
            num_entries--;
            printf("[SERVER] Deleted key '%s', remaining entries: %d\n", key, num_entries);
            fflush(stdout);
            return 0;
        }
    }
    
    printf("[SERVER] Key '%s' not found for deletion\n", key);
    fflush(stdout);
    return -1;
}

// Helper function to trim whitespace and newlines from the end of a string
void trim_end(char *str) {
    int len = strlen(str);
    while (len > 0 && (str[len-1] == ' ' || str[len-1] == '\n' || str[len-1] == '\r')) {
        str[len-1] = '\0';
        len--;
    }
}

void handle_client(int client_fd) {
    char cmd_buf[MAX_CMD_SIZE];
    int bytes_received;
    int bytes_sent;
    int ret;
    
    printf("[SERVER] New client connection on fd %d\n", client_fd);
    fflush(stdout);
    
    // Receive command
    int total_received = 0;
    while (total_received < MAX_CMD_SIZE - 1) {
        ret = sock_recv(client_fd, cmd_buf + total_received, 1, 0, &bytes_received, NULL);
        if (ret != 0 || bytes_received == 0) {
            printf("[SERVER] Failed to receive command or client disconnected (ret=%d, bytes=%d)\n", ret, bytes_received);
            fflush(stdout);
            sock_close(client_fd);
            return;
        }
        
        total_received += bytes_received;
        
        // Stop at newline
        if (cmd_buf[total_received - 1] == '\n' || total_received >= MAX_CMD_SIZE - 1) {
            break;
        }
    }
    
    // Null terminate the command
    cmd_buf[total_received] = '\0';
    printf("[SERVER] Received command (%d bytes): %s", total_received, cmd_buf);
    fflush(stdout);

    // Process the command
    if (strncmp(cmd_buf, "SET ", 4) == 0) {
        char* key_start = cmd_buf + 4;
        char* value_start = strchr(key_start, ' ');
        
        if (value_start != NULL) {
            *value_start = '\0';  // Split string at space
            value_start++;        // Move to start of value
            
            trim_end(key_start);  // Remove any trailing whitespace
            trim_end(value_start);
            
            printf("[SERVER] Processing SET request for key: '%s' value: '%s'\n", key_start, value_start);
            fflush(stdout);
            
            int result = set_key(key_start, value_start);
            printf("[SERVER] set_key() returned %d\n", result);
            fflush(stdout);
            
            if (result == 0) {
                const char* response = "OK\n";
                ret = sock_send(client_fd, response, strlen(response), 0, &bytes_sent);
                if (ret != 0 || bytes_sent != strlen(response)) {
                    printf("[SERVER] Failed to send response (ret=%d, bytes=%d)\n", ret, bytes_sent);
                    fflush(stdout);
                }
            } else {
                const char* response = "ERR 1\n";
                ret = sock_send(client_fd, response, strlen(response), 0, &bytes_sent);
                if (ret != 0 || bytes_sent != strlen(response)) {
                    printf("[SERVER] Failed to send response (ret=%d, bytes=%d)\n", ret, bytes_sent);
                    fflush(stdout);
                }
            }
        } else {
            const char* response = "ERR: Invalid SET format\n";
            ret = sock_send(client_fd, response, strlen(response), 0, &bytes_sent);
            if (ret != 0 || bytes_sent != strlen(response)) {
                printf("[SERVER] Failed to send response (ret=%d, bytes=%d)\n", ret, bytes_sent);
                fflush(stdout);
            }
        }
    } else if (strncmp(cmd_buf, "GET ", 4) == 0) {
        char* key = cmd_buf + 4;
        trim_end(key);
        char value[MAX_VAL];
        
        printf("[SERVER] Processing GET request for key: '%s'\n", key);
        fflush(stdout);
        
        if (get_key(key, value, MAX_VAL) == 0) {
            char response[MAX_VAL + 8];
            snprintf(response, sizeof(response), "VALUE %s\n", value);
            ret = sock_send(client_fd, response, strlen(response), 0, &bytes_sent);
            if (ret != 0 || bytes_sent != strlen(response)) {
                printf("[SERVER] Failed to send response (ret=%d, bytes=%d)\n", ret, bytes_sent);
                fflush(stdout);
            }
        } else {
            const char* response = "ERR: Key not found\n";
            ret = sock_send(client_fd, response, strlen(response), 0, &bytes_sent);
            if (ret != 0 || bytes_sent != strlen(response)) {
                printf("[SERVER] Failed to send response (ret=%d, bytes=%d)\n", ret, bytes_sent);
                fflush(stdout);
            }
        }
    } else if (strncmp(cmd_buf, "DEL ", 4) == 0) {
        char* key = cmd_buf + 4;
        trim_end(key);
        
        printf("[SERVER] Processing DEL request for key: '%s'\n", key);
        fflush(stdout);
        
        if (del_key(key) == 0) {
            const char* response = "OK\n";
            ret = sock_send(client_fd, response, strlen(response), 0, &bytes_sent);
            if (ret != 0 || bytes_sent != strlen(response)) {
                printf("[SERVER] Failed to send response (ret=%d, bytes=%d)\n", ret, bytes_sent);
                fflush(stdout);
            }
        } else {
            const char* response = "ERR: Failed to delete\n";
            ret = sock_send(client_fd, response, strlen(response), 0, &bytes_sent);
            if (ret != 0 || bytes_sent != strlen(response)) {
                printf("[SERVER] Failed to send response (ret=%d, bytes=%d)\n", ret, bytes_sent);
                fflush(stdout);
            }
        }
    } else if (strncmp(cmd_buf, "QUIT", 4) == 0) {
        const char* response = "BYE\n";
        ret = sock_send(client_fd, response, strlen(response), 0, &bytes_sent);
        if (ret != 0 || bytes_sent != strlen(response)) {
            printf("[SERVER] Failed to send response (ret=%d, bytes=%d)\n", ret, bytes_sent);
            fflush(stdout);
        }
    } else {
        const char* response = "ERR: Unknown command\n";
        ret = sock_send(client_fd, response, strlen(response), 0, &bytes_sent);
        if (ret != 0 || bytes_sent != strlen(response)) {
            printf("[SERVER] Failed to send response (ret=%d, bytes=%d)\n", ret, bytes_sent);
            fflush(stdout);
        }
    }
    
    // Shutdown write side and close socket after sending response
    printf("[SERVER] Shutting down and closing client connection\n");
    fflush(stdout);
    
    ret = sock_shutdown(client_fd, 1);  // SHUT_WR
    if (ret != 0) {
        printf("[SERVER] Failed to shutdown socket write side (ret=%d)\n", ret);
        fflush(stdout);
    }
    
    ret = sock_close(client_fd);
    if (ret != 0) {
        printf("[SERVER] Failed to close socket (ret=%d)\n", ret);
        fflush(stdout);
    }
}

int main() {
    int server_fd, client_fd, ret;
    
    printf("[SERVER] Starting KV server (in-memory version)...\n");
    fflush(stdout);
    
    // Initialize with some test data
    set_key("test1", "value1");
    set_key("test2", "value2");
    printf("[SERVER] Initialized with %d test entries\n", num_entries);
    fflush(stdout);
    
    ret = sock_open(2, 1, 0, &server_fd); // AF_INET=2, SOCK_STREAM=1
    if (ret != 0) {
        printf("[SERVER] Failed to open socket (ret=%d)\n", ret);
        return 1;
    }
    printf("[SERVER] Server socket opened with fd: %d\n", server_fd);
    fflush(stdout);
    
    ret = sock_listen(server_fd, 5);
    if (ret != 0) {
        printf("[SERVER] Failed to listen on socket (ret=%d)\n", ret);
        return 1;
    }
    printf("[SERVER] KV server listening on port 7000\n");
    fflush(stdout);
    
    while (1) {
        ret = sock_accept(server_fd, 0, &client_fd);
        if (ret != 0) {
            printf("[SERVER] Failed to accept connection (error: %d)\n", ret);
            continue;
        }
        printf("[SERVER] Accepted connection with client fd: %d\n", client_fd);
        fflush(stdout);
        
        handle_client(client_fd);
        
        printf("[SERVER] Client connection handled\n");
        fflush(stdout);
    }
    return 0;
} 