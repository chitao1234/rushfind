#include <regex.h>

int rushfind_regexec_match(const char *pattern, const char *reply) {
    regex_t regex;
    int status = regcomp(&regex, pattern, REG_EXTENDED | REG_NOSUB);
    if (status != 0) {
        return -1;
    }

    status = regexec(&regex, reply, 0, 0, 0);
    regfree(&regex);

    if (status == 0) {
        return 1;
    }
    if (status == REG_NOMATCH) {
        return 0;
    }
    return -1;
}
