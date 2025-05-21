// test_disk_limit.c
#include <stdio.h>
#include <fcntl.h>
#include <unistd.h>
#include <string.h>

int main(void) {
    // Create or open a file for writing.
    int fd = open("bigfile.txt", O_WRONLY | O_CREAT, 0666);
    if (fd < 0) {
        printf("Failed to create bigfile.txt\n");
        return 1;
    }

    printf("Starting to write one large file...\n");
    
    // Write data in a loop until we exceed the disk limit.
    const char *buf = "Hello, writing more than the disk limit...\n";
    size_t len = strlen(buf);
    size_t total_bytes = 0;

    for (int i = 0; i < 100000; i++) {
        ssize_t written = write(fd, buf, len);
        if (written < 0) {
            printf("Write failed at iteration %d after writing %zu bytes\n", i, total_bytes);
            close(fd);
            return 1;
        }
        
        total_bytes += written;
        
        // Print progress once in a while.
        if (i % 1000 == 0) {
            printf("Wrote %zu bytes so far (%d iterations)...\n", total_bytes, i);
        }
    }

    printf("Finished writing %zu bytes without hitting limit?\n", total_bytes);
    close(fd);
    return 0;
}
