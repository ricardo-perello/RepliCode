#include <stdio.h>
#include <fcntl.h>
#include <unistd.h>
#include <string.h>

// Writes 4096 bytes total in chunks of 128 characters. 
// If your `max_write_buffer` is smaller than 128, it will block multiple times.
int main(void) {
    // Open or create "block_test.txt"
    int fd = open("block_test.txt", O_WRONLY | O_CREAT | O_TRUNC, 0666);
    if (fd < 0) {
        printf("Failed to create block_test.txt\n");
        return 1;
    }

    // We'll create a 128-byte chunk. We'll write 32 times => 4096 bytes total.
    char chunk[128];
    memset(chunk, 'A', sizeof(chunk)); // fill with 'A'
    // We'll do 32 writes => 32 * 128 = 4096 bytes
    for (int i = 0; i < 32; i++) {
        ssize_t written = write(fd, chunk, 128);
        if (written < 0) {
            printf("Write failed on iteration %d\n", i);
            close(fd);
            return 1;
        }
        printf("Wrote chunk %d, %zd bytes\n", i, written);
    }

    close(fd);
    printf("Done writing 4096 bytes.\n");
    return 0;
}
