# Bufferbloat: The Latency You Can Fix Yourself

> A short, honest guide to bufferbloat, why it often matters more than your
> route, and what LightSpeed can and cannot do about it.

If your ping is fine when nothing else is happening but spikes the moment
someone starts a download or a video call, the problem is probably not your
route to the game server. It is probably bufferbloat, and the fix usually lives
on your own router, not in any proxy.

This guide explains what bufferbloat is, how to tell whether it is your problem,
and what LightSpeed does and does not do about it. It is deliberately not a
sales pitch: for many players, the router fix below will help more than any
tunnel.

---

## What bufferbloat is

Every router and modem has buffers: small queues that hold packets when a link
is briefly busy. Buffers are good in small amounts. They absorb bursts so the
link stays full. The problem is when the buffers are large and the link is
saturated.

When your connection is running at capacity (a big download, a backup, a 4K
stream, a game update), packets queue up in those buffers. A game packet that
would normally cross the link in a few milliseconds now waits behind hundreds of
milliseconds of other people's data. Your ping jumps from 20 ms to 200 ms or
more, and it stays there until the download finishes. That is bufferbloat: high
latency caused by full buffers under load, not by a bad route.

It is a queueing problem, not a bandwidth problem. You can have a fast
connection and still have terrible loaded latency.

---

## How to tell if it is your problem

The test is simple: measure latency while idle, then measure it again while the
connection is saturated.

1. Run a latency test with nothing else using the network. Note the number.
2. Start a large download or a speed test that uploads and downloads at full
   speed.
3. While that is running, run the same latency test again.

If the loaded number is much higher than the idle number, you have bufferbloat.
A rough scale:

| Idle to loaded increase | What it means |
|---|---|
| Under 30 ms | Fine. Bufferbloat is not your main problem. |
| 30 to 100 ms | Noticeable. Worth fixing if you care about latency. |
| Over 100 ms | Severe. This is very likely hurting your game more than your route. |

The Waveform bufferbloat test (`https://www.waveform.com/tools/bufferbloat`) and
the `flent` tool both measure this directly. A plain speed test does not: it
reports throughput, not latency under load.

You can also just watch your in-game ping while a large download runs. If the
ping climbs and stays high, that is the signal.

---

## The fix: SQM on your router

The fix is called SQM (Smart Queue Management), and it runs on your router. It
replaces the deep, dumb buffers with a shallow, fair queue that keeps latency low
even when the link is full. The two algorithms you will see are:

- **fq_codel**: fair queueing with CoDel. The default on most modern Linux-based
  routers. Good, widely available.
- **CAKE**: Common Applications Kept Enhanced. Newer, better at shaping, and
  handles upload and download separately. Preferred if your router supports it.
- **QWAVE**: a newer standard for Wi-Fi latency, often paired with CAKE on the
  wired side. Useful when the bottleneck is your Wi-Fi link.

You do not need to understand the algorithms. You need a router that can run
them and a few numbers.

### What you need

1. A router that supports SQM. OpenWrt, DD-WRT, and many Asus, Netgear, and
   Ubiquiti routers do. Some ISP-provided routers do not; if yours does not, a
   cheap supported router in front of it can still help.
2. Your real upload and download speeds, measured when the network is otherwise
   idle. Use a wired connection for the measurement if you can.
3. About ten minutes.

### What to do

1. Enable SQM in your router's settings. On OpenWrt it is under Network, then
   SQM. On other firmware look for "SQM", "Smart Queue", "QoS", or "CAKE".
2. Set the download and upload limits to about **85 to 95 percent** of your
   measured speeds. The exact number matters: set it too high and the queue
   still fills; set it too low and you waste bandwidth. Start at 90 percent and
   adjust.
3. Choose CAKE if it is offered, otherwise fq_codel.
4. Save and re-run the latency-under-load test.

The loaded latency should drop sharply, often to within a few milliseconds of
idle. If it does not, your bottleneck may be the modem, the Wi-Fi link, or the
ISP's own equipment, and SQM on the router alone will not fully fix it.

### If your router cannot do it

Options, roughly in order of effort:

- **Replace the router** with one that runs OpenWrt or ships SQM. This is the
  most reliable fix.
- **Put a supported router in front** of the ISP router, in bridge or modem
  mode if possible.
- **Shape on a single machine** with `tc` and CAKE, if only that machine matters
  and you can run Linux on it. This helps that machine but not the whole home.
- **Reduce the load**: pause background downloads and cloud backups while you
  play. Crude, but it works.

---

## What LightSpeed can and cannot do

LightSpeed is a routing optimizer. It changes the path your game packets take
between you and the game server. It does not manage the queue on your own link.

**What it can do:**

- Route around a congested or badly peered path between your ISP and the game
  server, which lowers the base latency when the problem is the route.
- Stabilize jitter that comes from a bad path, by sending traffic over a
  better-peered backbone.
- Recover some lost packets with FEC, which helps on lossy links.

**What it cannot do:**

- It cannot fix bufferbloat on your own connection. If your router is queueing
  your game packets behind a download, the packets are already delayed before
  they reach the tunnel. LightSpeed cannot un-delay them.
- It cannot make a saturated link faster. It is not a bandwidth booster.
- It cannot fix Wi-Fi problems. If the bottleneck is your wireless link, the
  fix is on your side.
- It cannot help if the game server itself is the bottleneck.

The honest summary: if your loaded latency is bad, fix that first with SQM. If
your idle latency is bad, or your route is jittery, that is where LightSpeed can
help. The two fixes are complementary, not competing.

---

## A quick decision guide

- **Idle ping high, loaded ping similar:** the route is the problem. LightSpeed
  can help.
- **Idle ping fine, loaded ping high:** bufferbloat. Fix it with SQM on your
  router.
- **Both high:** fix bufferbloat first, then see what is left. LightSpeed can
  address the remainder.
- **Ping fine but you get packet loss:** could be the route or the link. FEC and
  a better route may help; a bad local link will not be fixed by either.

Measure before and after each change. If a change does not move the numbers,
revert it and try the next one. Latency work is mostly measurement.
