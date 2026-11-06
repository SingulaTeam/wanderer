"""The test's whole internet, on one address.

- http://ADDR:8080/ answers with the address the request came from, so a probe
  can tell a request that went through the tunnel (it comes from the node)
  from one that did not (it comes from the client).
- ADDR:8443 is a TLS 1.3 server for REALITY to borrow its handshake from, as
  a real node borrows a real site's.
- DNS_ADDR:53 answers every A query with ADDR, as the public resolver the
  daemon asks for a node's name.

Usage: python3 -I internet.py ADDR CERT KEY DNS_ADDR
"""

import http.server
import socket
import socketserver
import ssl
import sys
import threading


class Echo(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        body = self.client_address[0].encode() + b"\n"
        self.send_response(200)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True


def dns(listen, answer):
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.bind((listen, 53))
    while True:
        q, peer = s.recvfrom(512)
        if len(q) < 12:
            continue
        # The question ends at the first zero label after the header.
        end = q.index(b"\0", 12) + 5
        header = q[:2] + b"\x81\x80" + q[4:6] + b"\x00\x01\x00\x00\x00\x00"
        record = b"\xc0\x0c\x00\x01\x00\x01\x00\x00\x00\x3c\x00\x04" + socket.inet_aton(answer)
        s.sendto(header + q[12:end] + record, peer)


def main():
    addr, cert, key, dns_addr = sys.argv[1:5]
    threading.Thread(target=dns, args=(dns_addr, addr), daemon=True).start()
    plain = Server((addr, 8080), Echo)

    tls = Server((addr, 8443), Echo)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.minimum_version = ssl.TLSVersion.TLSv1_3
    context.set_alpn_protocols(["h2", "http/1.1"])
    context.load_cert_chain(cert, key)
    tls.socket = context.wrap_socket(tls.socket, server_side=True)

    threading.Thread(target=tls.serve_forever, daemon=True).start()
    plain.serve_forever()


if __name__ == "__main__":
    main()
