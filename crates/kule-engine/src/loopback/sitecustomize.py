# Kule: a server that asks for every address listens on this computer only,
# so nothing on the network reaches what runs in a task. Loaded into every
# Python process of a task through PYTHONPATH. A socket given an address of
# its own is left as it is.
import socket as _socket

_ANY4 = {"", "0.0.0.0"}
_ANY6 = {"", "::", "::0", "0:0:0:0:0:0:0:0"}
_bind = _socket.socket.bind


def _kule_bind(self, address):
    try:
        if isinstance(address, tuple) and address:
            if self.family == _socket.AF_INET and address[0] in _ANY4:
                address = ("127.0.0.1",) + tuple(address[1:])
            elif self.family == _socket.AF_INET6 and address[0] in _ANY6:
                # A dual-stack socket keeps taking IPv4, on loopback.
                only6 = self.getsockopt(_socket.IPPROTO_IPV6, _socket.IPV6_V6ONLY)
                address = ("::1" if only6 else "::ffff:127.0.0.1",) + tuple(address[1:])
    except OSError:
        pass
    return _bind(self, address)


_socket.socket.bind = _kule_bind


def _kule_chain():
    # A sitecustomize of the project's own, which this one shadows, still runs.
    import importlib.util
    import os
    import sys

    here = os.path.dirname(os.path.abspath(__file__))
    for entry in sys.path:
        folder = os.path.abspath(entry or ".")
        if folder == here:
            continue
        found = os.path.join(folder, "sitecustomize.py")
        if os.path.isfile(found):
            spec = importlib.util.spec_from_file_location("_kule_shadowed_sitecustomize", found)
            if spec and spec.loader:
                spec.loader.exec_module(importlib.util.module_from_spec(spec))
            return


try:
    _kule_chain()
except Exception:
    pass
