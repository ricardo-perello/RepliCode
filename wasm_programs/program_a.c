#include <stdio.h>
#include <unistd.h>
#include <errno.h>

int main() {
    printf("Program B: Starting and sleeping for 1 second...\n");
    sleep(10000);
    printf("Program B: Woke up!\n");
    return 0;
}