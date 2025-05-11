#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <fcntl.h>
#include <unistd.h>
#include <sys/stat.h>

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

#define BUF_SIZE 4096
#define MAX_KEY 128
#define MAX_VAL 1024

void handle_client(int client_fd);
int set_key(const char* key, const char* value);
int get_key(const char* key, char* value_out, int maxlen);
int del_key(const char* key);

int main() {
    int server_fd, client_fd, ret;
    ret = sock_open(2, 1, 0, &server_fd); // AF_INET=2, SOCK_STREAM=1
    if (ret != 0) {
        printf("Failed to open socket\n");
        return 1;
    }
    ret = sock_listen(server_fd, 5);
    if (ret != 0) {
        printf("Failed to listen on socket\n");
        return 1;
    }
    printf("KV server listening on port 7000\n");
    fflush(stdout);
    while (1) {
        ret = sock_accept(server_fd, 0, &client_fd);
        if (ret == 0) {
            handle_client(client_fd);
            sock_shutdown(client_fd, 3);
        }
    }
    return 0;
}

void handle_client(int client_fd) {
    char buf[BUF_SIZE];
    int received = 0;
    while (1) {
        // Read a line
        received = 0;
        while (received < BUF_SIZE - 1) {
            int n = 0;
            int ret = sock_recv(client_fd, buf + received, 1, 0, &n, NULL);
            if (ret != 0 || n <= 0) return;
            if (buf[received] == '\n') {
                buf[received] = 0;
                break;
            }
            received++;
        }
        if (strncmp(buf, "SET ", 4) == 0) {
            char key[MAX_KEY], value[MAX_VAL];
            if (sscanf(buf+4, "%127s %1023[^]", key, value) == 2) {
                if (set_key(key, value) == 0) {
                    char ok[] = "OK\n";
                    int sent;
                    sock_send(client_fd, ok, strlen(ok), 0, &sent);
                } else {
                    char err[] = "ERR\n";
                    int sent;
                    sock_send(client_fd, err, strlen(err), 0, &sent);
                }
            } else {
                char err[] = "ERR\n";
                int sent;
                sock_send(client_fd, err, strlen(err), 0, &sent);
            }
        } else if (strncmp(buf, "GET ", 4) == 0) {
            char key[MAX_KEY], value[MAX_VAL];
            if (sscanf(buf+4, "%127s", key) == 1) {
                if (get_key(key, value, MAX_VAL) == 0) {
                    char resp[MAX_VAL+8];
                    snprintf(resp, sizeof(resp), "VALUE %s\n", value);
                    int sent;
                    sock_send(client_fd, resp, strlen(resp), 0, &sent);
                } else {
                    char err[] = "ERR\n";
                    int sent;
                    sock_send(client_fd, err, strlen(err), 0, &sent);
                }
            } else {
                char err[] = "ERR\n";
                int sent;
                sock_send(client_fd, err, strlen(err), 0, &sent);
            }
        } else if (strncmp(buf, "DEL ", 4) == 0) {
            char key[MAX_KEY];
            if (sscanf(buf+4, "%127s", key) == 1) {
                if (del_key(key) == 0) {
                    char ok[] = "OK\n";
                    int sent;
                    sock_send(client_fd, ok, strlen(ok), 0, &sent);
                } else {
                    char err[] = "ERR\n";
                    int sent;
                    sock_send(client_fd, err, strlen(err), 0, &sent);
                }
            } else {
                char err[] = "ERR\n";
                int sent;
                sock_send(client_fd, err, strlen(err), 0, &sent);
            }
        } else {
            char err[] = "ERR\n";
            int sent;
            sock_send(client_fd, err, strlen(err), 0, &sent);
        }
    }
}

int set_key(const char* key, const char* value) {
    int fd = open(key, O_WRONLY | O_CREAT | O_TRUNC, 0666);
    if (fd < 0) return -1;
    ssize_t nwritten = write(fd, value, strlen(value));
    close(fd);
    return (nwritten == (ssize_t)strlen(value)) ? 0 : -1;
}

int get_key(const char* key, char* value_out, int maxlen) {
    int fd = open(key, O_RDONLY);
    if (fd < 0) return -1;
    ssize_t nread = read(fd, value_out, maxlen-1);
    close(fd);
    if (nread > 0) {
        value_out[nread] = 0;
        return 0;
    }
    return -1;
}

int del_key(const char* key) {
    return unlink(key);
} 