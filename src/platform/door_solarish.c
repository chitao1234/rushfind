#include <door.h>
#include <fcntl.h>
#include <stropts.h>
#include <sys/stat.h>
#include <unistd.h>

static void rushfind_test_door_server(void *cookie, char *argp, size_t arg_size,
                                      door_desc_t *dp, uint_t n_desc) {
    (void)cookie;
    (void)argp;
    (void)arg_size;
    (void)dp;
    (void)n_desc;
    door_return(NULL, 0, NULL, 0);
    _exit(127);
}

int rushfind_test_create_door(const char *path) {
    int placeholder = open(path, O_CREAT | O_TRUNC | O_RDWR, 0600);
    if (placeholder < 0) return -1;
    close(placeholder);
    int fd = door_create(rushfind_test_door_server, NULL, 0);
    if (fd < 0) return -1;
    if (fattach(fd, path) != 0) {
        close(fd);
        return -1;
    }
    return fd;
}

int rushfind_test_destroy_door(int fd, const char *path) {
    int detach = fdetach(path);
    int revoke = door_revoke(fd);
    close(fd);
    return detach == 0 && revoke == 0 ? 0 : -1;
}
