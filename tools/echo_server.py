#!/usr/bin/env python3
"""Simple UDP echo server for E2E testing."""
import socket
import time
import sys

port = int(sys.argv[1]) if len(sys.argv) > 1 else 9999
sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
sock.bind(('0.0.0.0', port))
print(f'UDP Echo server listening on 0.0.0.0:{port}', flush=True)

while True:
    data, addr = sock.recvfrom(2048)
    print(f'[{time.strftime("%H:%M:%S")}] Echo {len(data)} bytes from {addr}', flush=True)
    sock.sendto(data, addr)
