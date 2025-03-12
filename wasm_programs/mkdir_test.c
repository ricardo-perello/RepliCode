#include <stdio.h>
#include <sys/stat.h>  // for mkdir
#include <sys/types.h>
#include <dirent.h>    // for opendir, readdir, closedir
#include <fcntl.h>     // for open
#include <unistd.h>    // for close, rmdir, unlink
#include <string.h>    // for strlen

int main(void) {
    printf("started");
    // 1) Create a new directory
    int rc = mkdir("example_dir", 0777);
    if (rc != 0) {
        perror("mkdir failed");
        return 1;
    }
    printf("Directory 'example_dir' created successfully.\n");

    // 2) Open the directory just to confirm it exists
    DIR* dirp = opendir("example_dir");
    if (!dirp) {
        perror("opendir failed");
        return 1;
    }
    printf("Opened 'example_dir' successfully.\n");
    closedir(dirp);

    // 3) Create a file inside that directory and write some text
    int fd = open("example_dir/testfile.txt", O_WRONLY | O_CREAT, 0666);
    if (fd < 0) {
        perror("open failed");
        return 1;
    }
    const char* msg = "Hello from inside example_dir!\n";
    ssize_t written = write(fd, msg, strlen(msg));
    if (written < 0) {
        perror("write failed");
        close(fd);
        return 1;
    }
    close(fd);
    printf("Wrote a test file inside 'example_dir'.\n");

    // 4) Remove the test file
    rc = unlink("example_dir/testfile.txt");
    if (rc != 0) {
        perror("unlink failed");
        return 1;
    }
    printf("Removed 'testfile.txt'.\n");

    // 5) Finally, remove the directory
    rc = rmdir("example_dir");
    if (rc != 0) {
        perror("rmdir failed");
        return 1;
    }
    printf("'example_dir' was removed.\n");

    printf("All directory tests finished successfully.\n");
    return 0;
}
