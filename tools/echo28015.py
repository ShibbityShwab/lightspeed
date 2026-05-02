#!/usr/bin/env python3
"""UDP echo server on port 28015 for LightSpeed capture/inject E2E test."""
import socket, time

s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(('0.0.0.0', 28015))
print('UDP echo 0.0.0.0:28015 ready', flush=True)
while True:
    d, a = s.recvfrom(2048)
    msg = time.strftime('%H:%M:%S') + ' echo ' + str(len(d)) + 'B from ' + str(a)
    print(msg, flush=True)
    s.sendto(d, a)
