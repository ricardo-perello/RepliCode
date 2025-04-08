// test_rmdir.c
#include <stdio.h>
#include <sys/stat.h>
#include <fcntl.h>
#include <unistd.h>
#include <string.h>

int main(void) {
    int rc = mkdir("subdir", 0777);
    if (rc != 0) {
        printf("mkdir failed! rc=%d\n", rc);
        return 1;
    }
    // Now create a file inside subdir.
    int fd = open("subdir/test_in_subdir.txt", O_WRONLY | O_CREAT, 0666);
    if (fd < 0) {
        printf("Failed to create subdir/test_in_subdir.txt\n");
        return 1;
    }
    const char *msg = "File inside subdir!\n";
    write(fd, msg, strlen(msg));
    close(fd);

    // Attempt to remove the directory
    // In POSIX, rmdir() only removes empty directories, so remove the file first:
    // or your runtime might do the same check. If your runtime automatically
    // does a remove_dir that won't remove non-empty directories, you might have to unlink first.
    rc = unlink("subdir/test_in_subdir.txt");
    if (rc != 0) {
        printf("Failed to unlink subdir/test_in_subdir.txt, rc=%d\n", rc);
        return 1;
    }

    rc = rmdir("subdir");
    if (rc != 0) {
        printf("rmdir failed! rc=%d\n", rc);
        return 1;
    }
    printf("Removed subdir successfully.\n");
    return 0;
}
