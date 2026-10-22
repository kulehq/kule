// macOS: the TCP sockets a process listens on, read from the kernel with
// libproc, as lsof reads them. In C for the layout of socket_fdinfo, which
// only the system's own headers give reliably.

#include <libproc.h>
#include <netinet/in.h>
#include <stdlib.h>
#include <string.h>
#include <sys/proc_info.h>

struct kule_listener {
    int family;              // 4 or 6
    unsigned short port;
    unsigned char addr[16];  // 4 bytes for IPv4
};

// Fills `out` (room for `max`) with the listening TCP sockets of `pid`;
// returns how many, or -1 when the process cannot be read.
int kule_listeners(int pid, struct kule_listener *out, int max) {
    int size = proc_pidinfo(pid, PROC_PIDLISTFDS, 0, NULL, 0);
    if (size <= 0) {
        return -1;
    }
    struct proc_fdinfo *fds = malloc((size_t)size);
    if (fds == NULL) {
        return -1;
    }
    size = proc_pidinfo(pid, PROC_PIDLISTFDS, 0, fds, size);
    if (size <= 0) {
        free(fds);
        return -1;
    }
    int count = size / (int)sizeof(struct proc_fdinfo);
    int found = 0;
    for (int i = 0; i < count && found < max; i++) {
        if (fds[i].proc_fdtype != PROX_FDTYPE_SOCKET) {
            continue;
        }
        struct socket_fdinfo info;
        int got = proc_pidfdinfo(pid, fds[i].proc_fd, PROC_PIDFDSOCKETINFO, &info, sizeof info);
        if (got != (int)sizeof info || info.psi.soi_kind != SOCKINFO_TCP) {
            continue;
        }
        struct tcp_sockinfo *tcp = &info.psi.soi_proto.pri_tcp;
        if (tcp->tcpsi_state != TSI_S_LISTEN) {
            continue;
        }
        struct in_sockinfo *in = &tcp->tcpsi_ini;
        struct kule_listener *l = &out[found++];
        l->port = ntohs((unsigned short)in->insi_lport);
        memset(l->addr, 0, sizeof l->addr);
        if (in->insi_vflag & INI_IPV6) {
            l->family = 6;
            memcpy(l->addr, &in->insi_laddr.ina_6, 16);
        } else {
            l->family = 4;
            memcpy(l->addr, &in->insi_laddr.ina_46.i46a_addr4, 4);
        }
    }
    free(fds);
    return found;
}

struct kule_connection {
    int family;              // 4 or 6
    unsigned short lport;    // this end's port
    unsigned char raddr[16]; // the other end's address, 4 bytes for IPv4
};

// Fills `out` (room for `max`) with the established TCP connections of
// `pid`: this end's port and the other end's address; returns how many,
// or -1 when the process cannot be read.
int kule_connections(int pid, struct kule_connection *out, int max) {
    int size = proc_pidinfo(pid, PROC_PIDLISTFDS, 0, NULL, 0);
    if (size <= 0) {
        return -1;
    }
    struct proc_fdinfo *fds = malloc((size_t)size);
    if (fds == NULL) {
        return -1;
    }
    size = proc_pidinfo(pid, PROC_PIDLISTFDS, 0, fds, size);
    if (size <= 0) {
        free(fds);
        return -1;
    }
    int count = size / (int)sizeof(struct proc_fdinfo);
    int found = 0;
    for (int i = 0; i < count && found < max; i++) {
        if (fds[i].proc_fdtype != PROX_FDTYPE_SOCKET) {
            continue;
        }
        struct socket_fdinfo info;
        int got = proc_pidfdinfo(pid, fds[i].proc_fd, PROC_PIDFDSOCKETINFO, &info, sizeof info);
        if (got != (int)sizeof info || info.psi.soi_kind != SOCKINFO_TCP) {
            continue;
        }
        struct tcp_sockinfo *tcp = &info.psi.soi_proto.pri_tcp;
        if (tcp->tcpsi_state == TSI_S_LISTEN || tcp->tcpsi_state == TSI_S_CLOSED) {
            continue;
        }
        struct in_sockinfo *in = &tcp->tcpsi_ini;
        struct kule_connection *c = &out[found++];
        c->lport = ntohs((unsigned short)in->insi_lport);
        memset(c->raddr, 0, sizeof c->raddr);
        if (in->insi_vflag & INI_IPV6) {
            c->family = 6;
            memcpy(c->raddr, &in->insi_faddr.ina_6, 16);
        } else {
            c->family = 4;
            memcpy(c->raddr, &in->insi_faddr.ina_46.i46a_addr4, 4);
        }
    }
    free(fds);
    return found;
}
