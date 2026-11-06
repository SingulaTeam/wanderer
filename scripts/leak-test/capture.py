"""Counts what the client sends on one interface, for the leak test.

tcpdump cannot run here: in a user namespace it fails to drop to its own user
and writes nothing. This reads the interface with a packet socket instead,
until SIGTERM, then prints one line of counts:

    leaked=N allowed=N lan_dns=N lan=N

- allowed: TCP to an endpoint the guard admits (the node, the Control Plane)
- leaked:  anything else that left, apart from ARP and IPv6 neighbour
           discovery, which carry nothing of the user's
- lan_dns, lan: the same split for the local-network interface, where DNS is
           a leak and everything else is allowed or not by the LAN toggle

Usage: python3 -I capture.py IFACE ALLOWED_ENDPOINT...   (as ADDR:PORT)
"""

import signal
import socket
import struct
import sys

ETH_P_ALL = 0x0003
PACKET_OUTGOING = 4
ETH_ARP, ETH_IPV4, ETH_IPV6 = 0x0806, 0x0800, 0x86DD
NEIGHBOUR_DISCOVERY = {133, 134, 135, 136, 143}  # ICMPv6 RS, RA, NS, NA, MLDv2


def classify(frame, allowed):
    ethertype = struct.unpack("!H", frame[12:14])[0]
    ip = frame[14:]
    if ethertype == ETH_ARP:
        return None
    if ethertype == ETH_IPV6:
        # ICMPv6 without extension headers: next header 58, type at byte 40.
        if ip[6] == 58 and ip[40] in NEIGHBOUR_DISCOVERY:
            return None
        return "other"
    if ethertype != ETH_IPV4:
        return "other"
    proto = ip[9]
    header = (ip[0] & 0x0F) * 4
    dst = socket.inet_ntoa(ip[16:20])
    if proto in (6, 17):
        dport = struct.unpack("!H", ip[header + 2 : header + 4])[0]
        if dport == 53:
            return "dns"
        if proto == 6 and (dst, dport) in allowed:
            return "allowed"
    return "other"


def main():
    iface = sys.argv[1]
    allowed = set()
    for endpoint in sys.argv[2:]:
        addr, port = endpoint.rsplit(":", 1)
        allowed.add((addr, int(port)))

    s = socket.socket(socket.AF_PACKET, socket.SOCK_RAW, socket.htons(ETH_P_ALL))
    s.bind((iface, 0))
    counts = {"allowed": 0, "dns": 0, "other": 0}

    def done(*_):
        print(
            f"leaked={counts['other'] + counts['dns']} allowed={counts['allowed']} "
            f"lan_dns={counts['dns']} lan={counts['other']}",
            flush=True,
        )
        sys.exit(0)

    signal.signal(signal.SIGTERM, done)
    print("ready", flush=True)
    while True:
        frame, address = s.recvfrom(65535)
        if address[2] != PACKET_OUTGOING or len(frame) < 34:
            continue
        kind = classify(frame, allowed)
        if kind:
            counts[kind] += 1


if __name__ == "__main__":
    main()
