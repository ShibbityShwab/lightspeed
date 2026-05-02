#!/bin/bash
pkill -f echo28015.py 2>/dev/null
sleep 0.3
nohup python3 /tmp/echo28015.py >/tmp/echo28015.log 2>&1 &
EPID=$!
sleep 1
echo "Echo server PID: $EPID"
cat /tmp/echo28015.log
