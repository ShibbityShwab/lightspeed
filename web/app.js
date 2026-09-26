// LightSpeed Landing Page - app.js
// Vanilla JS: mobile nav, scroll effects, live stats, trends, download detection.

(function () {
  'use strict';

  var prefersReduced = !!(window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches);

  // --- Mobile Nav Toggle ---
  var toggle = document.getElementById('nav-toggle');
  var navLinks = document.getElementById('nav-links');

  function setNavOpen(open) {
    if (!toggle || !navLinks) return;
    navLinks.classList.toggle('open', open);
    toggle.setAttribute('aria-expanded', open ? 'true' : 'false');
    toggle.setAttribute('aria-label', open ? 'Close menu' : 'Open menu');
  }

  function isNavOpen() {
    return !!(navLinks && navLinks.classList.contains('open'));
  }

  if (toggle && navLinks) {
    toggle.addEventListener('click', function () {
      setNavOpen(!isNavOpen());
    });
    navLinks.querySelectorAll('a').forEach(function (link) {
      link.addEventListener('click', function () { setNavOpen(false); });
    });
    document.addEventListener('keydown', function (e) {
      if (e.key === 'Escape' && isNavOpen()) {
        setNavOpen(false);
        toggle.focus();
      }
    });
    var mobileNav = window.matchMedia ? window.matchMedia('(max-width: 860px)') : null;
    if (mobileNav && mobileNav.addEventListener) {
      mobileNav.addEventListener('change', function (e) {
        if (!e.matches) setNavOpen(false);
      });
    }
  }

  // --- Scroll: nav solidifies past the fold (rAF-throttled, class-based) ---
  var nav = document.getElementById('nav');
  if (nav) {
    var navTicking = false;
    var syncNav = function () {
      nav.classList.toggle('is-scrolled', window.scrollY > 80);
      navTicking = false;
    };
    window.addEventListener('scroll', function () {
      if (navTicking) return;
      navTicking = true;
      window.requestAnimationFrame(syncNav);
    }, { passive: true });
    syncNav();
  }

  // --- Scroll: fade-in elements (skipped entirely under reduced motion) ---
  // Relay cards are rendered later from network-stats.json, so they are
  // observed by observeFade() when they are created rather than queried here.
  var fadeObserver = null;
  function observeFade(el) {
    if (!fadeObserver) return;
    el.classList.add('fade-in');
    fadeObserver.observe(el);
  }

  var faders = document.querySelectorAll('.step, .game-card, .bench-card, .compare-card, .download-card, .faq-item');
  if (!prefersReduced && faders.length && 'IntersectionObserver' in window) {
    fadeObserver = new IntersectionObserver(function (entries) {
      entries.forEach(function (entry) {
        if (entry.isIntersecting) {
          entry.target.classList.add('visible');
          fadeObserver.unobserve(entry.target);
        }
      });
    }, { threshold: 0.15, rootMargin: '0px 0px -40px 0px' });
    faders.forEach(function (el) { observeFade(el); });
  }

  // --- Animate benchmark bars on scroll (instant under reduced motion) ---
  var benchBars = document.querySelectorAll('.bench-bar-fill:not(.bench-bar-live)');
  if (benchBars.length && 'IntersectionObserver' in window && !prefersReduced) {
    benchBars.forEach(function (bar) {
      var targetWidth = bar.style.width;
      bar.style.width = '0%';
      var barObserver = new IntersectionObserver(function (entries) {
        entries.forEach(function (entry) {
          if (entry.isIntersecting) {
            setTimeout(function () { bar.style.width = targetWidth; }, 200);
            barObserver.unobserve(entry.target);
          }
        });
      }, { threshold: 0.5 });
      barObserver.observe(bar);
    });
  }

  // --- Smooth scroll for anchor links ---
  document.querySelectorAll('a[href^="#"]').forEach(function (anchor) {
    anchor.addEventListener('click', function (e) {
      var href = this.getAttribute('href');
      if (!href || href === '#') return;
      var target = null;
      try {
        target = document.querySelector(href);
      } catch (err) {
        target = null;
      }
      if (!target) return;
      e.preventDefault();
      target.scrollIntoView({ behavior: prefersReduced ? 'auto' : 'smooth', block: 'start' });
      if (window.history && window.history.pushState) {
        window.history.pushState(null, '', href);
      }
    });
  });

  // --- Live network stats from network-stats.json ---
  function formatUptime(secs) {
    if (typeof secs !== 'number' || !isFinite(secs) || secs < 0) return '--';
    if (secs < 60) return '<1m';
    var days = Math.floor(secs / 86400);
    var hours = Math.floor((secs % 86400) / 3600);
    var mins = Math.floor((secs % 3600) / 60);
    if (days > 0) return days + 'd ' + hours + 'h';
    if (hours > 0) return hours + 'h ' + mins + 'm';
    return mins + 'm';
  }

  function formatTimestamp(epochSecs, style) {
    if (style === 'relative') {
      var diff = Math.max(0, Math.floor(Date.now() / 1000) - epochSecs);
      if (diff < 60) return 'just now';
      if (diff < 3600) return Math.floor(diff / 60) + 'm ago';
      if (diff < 86400) return Math.floor(diff / 3600) + 'h ago';
      return Math.floor(diff / 86400) + 'd ago';
    }
    return new Date(epochSecs * 1000).toLocaleString(undefined, {
      year: 'numeric', month: 'short', day: 'numeric',
      hour: '2-digit', minute: '2-digit'
    });
  }

  function setStat(key, value) {
    document.querySelectorAll('[data-stat="' + key + '"]').forEach(function (el) {
      el.textContent = value;
    });
  }

  function updateSnapshotStatus(message) {
    var el = document.getElementById('snapshot-status');
    if (el) el.textContent = message;
  }

  // Relay node ids encode airport-style codes: relay-lax-1 -> LAX, relay-fra -> FRA.
  function nodeCode(nodeId) {
    var parts = String(nodeId || '').split('-').filter(Boolean);
    for (var i = 0; i < parts.length; i++) {
      if (parts[i] === 'relay') continue;
      if (/^\d+$/.test(parts[i])) continue;
      return parts[i].toUpperCase().slice(0, 3);
    }
    return 'NODE';
  }

  function relayStatus(status) {
    var value = String(status || '').toLowerCase();
    if (value === 'healthy' || value === 'online' || value === 'up') {
      return { label: 'Online', state: 'healthy', cardClass: 'relay-status' };
    }
    if (value === 'degraded') {
      return { label: 'Degraded', state: 'degraded', cardClass: 'relay-status relay-status-degraded' };
    }
    return { label: 'Offline', state: 'down', cardClass: 'relay-status relay-status-offline' };
  }

  function renderLastChecked(epochSecs) {
    var absolute = formatTimestamp(epochSecs, 'absolute');
    var iso = new Date(epochSecs * 1000).toISOString();
    document.querySelectorAll('[data-last-checked]').forEach(function (el) {
      el.textContent = formatTimestamp(epochSecs, 'relative');
      el.setAttribute('datetime', iso);
      el.setAttribute('title', absolute);
    });
  }

  function formatCount(value) {
    if (typeof value !== 'number' || !isFinite(value) || value < 0) return '--';
    if (value >= 1e9) return (value / 1e9).toFixed(1).replace(/\.0$/, '') + 'B';
    if (value >= 1e6) return (value / 1e6).toFixed(1).replace(/\.0$/, '') + 'M';
    if (value >= 1e3) return (value / 1e3).toFixed(1).replace(/\.0$/, '') + 'k';
    return String(Math.floor(value));
  }

  // The relay grid is built entirely from network-stats.json relays[], so a
  // newly published relay appears without a markup change. Cards are keyed by
  // data-node-id and updated in place; stale cards are removed.
  function fillRelayCard(card, relay) {
    var status = relayStatus(relay.status);
    card.textContent = '';

    var head = document.createElement('div');
    head.className = 'relay-head';

    var chip = document.createElement('span');
    chip.className = 'node-chip';
    chip.setAttribute('aria-hidden', 'true');
    chip.textContent = nodeCode(relay.node_id);

    var info = document.createElement('div');
    var title = document.createElement('h3');
    if (relay.flag) {
      var flag = document.createElement('span');
      flag.className = 'relay-flag';
      flag.setAttribute('aria-hidden', 'true');
      flag.textContent = relay.flag;
      title.appendChild(flag);
      title.appendChild(document.createTextNode(' '));
    }
    title.appendChild(document.createTextNode(relay.location || relay.node_id));
    var region = document.createElement('div');
    region.className = 'relay-region';
    region.textContent = relay.area || relay.region || 'Relay';
    info.appendChild(title);
    info.appendChild(region);
    head.appendChild(chip);
    head.appendChild(info);

    var statusEl = document.createElement('div');
    statusEl.className = status.cardClass;
    var dot = document.createElement('span');
    dot.className = 'status-dot';
    dot.setAttribute('aria-hidden', 'true');
    var statusLabel = document.createElement('span');
    statusLabel.setAttribute('data-relay-status', '');
    statusLabel.textContent = status.label;
    statusEl.appendChild(dot);
    statusEl.appendChild(statusLabel);

    var meta = document.createElement('div');
    meta.className = 'relay-meta';
    meta.textContent = 'v' + (relay.version || '--') + ' \u00b7 up ' + formatUptime(relay.uptime_secs);

    var packets = document.createElement('div');
    packets.className = 'relay-meta relay-meta-packets';
    packets.textContent = formatCount(relay.packets_relayed) + ' relayed \u00b7 ' +
      formatCount(relay.packets_dropped) + ' filtered \u00b7 ' +
      formatCount(relay.sessions_created) + ' sessions';

    card.appendChild(head);
    card.appendChild(statusEl);
    card.appendChild(meta);
    card.appendChild(packets);
    return card;
  }

  function relayCardEl(relay) {
    var card = document.createElement('article');
    card.className = 'relay-card';
    card.setAttribute('data-node-id', relay.node_id);
    return fillRelayCard(card, relay);
  }

  function sortRelays(relays) {
    return relays.slice().sort(function (a, b) {
      var areaA = String(a.area || a.region || '');
      var areaB = String(b.area || b.region || '');
      if (areaA !== areaB) return areaA < areaB ? -1 : 1;
      var locA = String(a.location || a.node_id);
      var locB = String(b.location || b.node_id);
      return locA < locB ? -1 : locA > locB ? 1 : 0;
    });
  }

  function renderRelayCards(relays) {
    var grid = document.querySelector('[data-relay-grid]');
    if (!grid) return;

    var ordered = sortRelays(relays.filter(function (relay) {
      return relay && relay.node_id;
    }));

    var note = grid.querySelector('.relay-grid-note');
    if (!ordered.length) {
      if (!note) {
        note = document.createElement('p');
        note.className = 'relay-grid-note';
        grid.appendChild(note);
      }
      note.textContent = 'The latest snapshot lists no relays. The signed registry is still published.';
      grid.setAttribute('aria-busy', 'false');
      return;
    }
    if (note) note.remove();

    var existing = {};
    grid.querySelectorAll('[data-node-id]').forEach(function (card) {
      existing[card.getAttribute('data-node-id')] = card;
    });

    ordered.forEach(function (relay, index) {
      var card = existing[relay.node_id];
      delete existing[relay.node_id];
      if (card) {
        fillRelayCard(card, relay);
      } else {
        card = relayCardEl(relay);
        observeFade(card);
      }
      var at = grid.children[index];
      if (at !== card) grid.insertBefore(card, at || null);
    });

    Object.keys(existing).forEach(function (id) {
      existing[id].remove();
    });

    grid.setAttribute('aria-busy', 'false');
  }

  function renderRelayHealthList(relays) {
    var list = document.getElementById('relay-health-list');
    if (!list || !relays.length) return;
    list.textContent = '';
    relays.forEach(function (relay) {
      var status = relayStatus(relay.status);

      var row = document.createElement('li');
      row.className = 'relay-health-row';

      var chip = document.createElement('span');
      chip.className = 'node-chip node-chip-sm';
      chip.textContent = nodeCode(relay.node_id);

      var name = document.createElement('span');
      name.className = 'relay-health-name';
      name.textContent = relay.location || relay.node_id;

      var state = document.createElement('span');
      state.className = 'relay-health-status is-' + status.state;
      state.textContent = status.label;

      var uptime = document.createElement('span');
      uptime.className = 'relay-health-uptime';
      uptime.textContent = formatUptime(relay.uptime_secs);

      var packets = document.createElement('span');
      packets.className = 'relay-health-packets';
      packets.textContent = formatCount(relay.packets_relayed) + ' relayed';
      packets.title = formatCount(relay.packets_relayed) + ' packets relayed, ' +
        formatCount(relay.packets_dropped) + ' filtered, ' +
        formatCount(relay.sessions_created) + ' sessions';

      row.appendChild(chip);
      row.appendChild(name);
      row.appendChild(state);
      row.appendChild(uptime);
      row.appendChild(packets);
      list.appendChild(row);
    });
  }

  // /health counters are process-lifetime and reset when the relay rolls over
  // in place, so the cards and headline read the accumulator the collector
  // carries across restarts instead. Falls back to /health until history loads.
  var lifetimeByRelay = null;
  var lastRelays = null;

  function applyRelayStats(relays) {
    var merged = relays.map(function (relay) {
      var lt = lifetimeByRelay && lifetimeByRelay[relay.node_id];
      if (!lt) return relay;
      var out = {};
      Object.keys(relay).forEach(function (k) { out[k] = relay[k]; });
      if (typeof lt.packets_relayed === 'number') out.packets_relayed = lt.packets_relayed;
      if (typeof lt.sessions_created === 'number') out.sessions_created = lt.sessions_created;
      return out;
    });
    renderRelayCards(merged);
    renderRelayHealthList(relays);

    var totalRelayed = 0;
    var totalDropped = 0;
    var totalUpstreamLoss = 0;
    var totalSessions = 0;
    merged.forEach(function (relay) {
      if (typeof relay.packets_relayed === 'number') totalRelayed += relay.packets_relayed;
      if (typeof relay.packets_dropped === 'number') totalDropped += relay.packets_dropped;
      if (typeof relay.drops_relay_send_errors === 'number') totalUpstreamLoss += relay.drops_relay_send_errors;
      if (typeof relay.sessions_created === 'number') totalSessions += relay.sessions_created;
    });
    setStat('packets_relayed', formatCount(totalRelayed));
    setStat('packets_dropped', formatCount(totalDropped));
    setStat('drops_relay_send_errors', formatCount(totalUpstreamLoss));
    setStat('sessions_created', formatCount(totalSessions));
  }

  function applyLifetime(snapshots) {
    if (!snapshots.length) return;
    var per = snapshots[snapshots.length - 1].per_relay || {};
    var byRelay = {};
    Object.keys(per).forEach(function (id) {
      var lt = per[id] && per[id].lifetime;
      if (lt && typeof lt === 'object') byRelay[id] = lt;
    });
    if (!Object.keys(byRelay).length) return;
    lifetimeByRelay = byRelay;
    if (lastRelays) applyRelayStats(lastRelays);
  }

  function renderNetworkStats(stats) {
    if (!stats || typeof stats !== 'object') return;

    var relayCount = typeof stats.relay_count === 'number' ? stats.relay_count : null;
    var healthyCount = typeof stats.healthy_count === 'number' ? stats.healthy_count : null;

    if (relayCount !== null) setStat('relay_count', String(relayCount));
    if (typeof stats.area_count === 'number') setStat('area_count', String(stats.area_count));
    if (relayCount !== null && healthyCount !== null) {
      setStat('relays_online', healthyCount + '/' + relayCount);
      updateSnapshotStatus('Relay snapshot updated: ' + healthyCount + ' of ' + relayCount + ' relays online.');
    } else {
      updateSnapshotStatus('Relay snapshot updated.');
    }

    var versions = Array.isArray(stats.software_versions) ? stats.software_versions : [];
    if (versions.length) {
      setStat('software_versions', versions.map(function (v) { return 'v' + v; }).join(', '));
    }

    if (typeof stats.generated_at === 'number') renderLastChecked(stats.generated_at);

    var relays = Array.isArray(stats.relays) ? stats.relays : [];
    if (relays.length) {
      lastRelays = relays;
      applyRelayStats(relays);
    } else {
      renderRelayCards([]);
    }

    var healthBar = document.getElementById('network-health-bar');
    if (healthBar && relayCount && healthyCount !== null) {
      var pct = Math.max(0, Math.min(100, Math.round((healthyCount / relayCount) * 100)));
      healthBar.style.width = pct + '%';
    }
  }

  function showRelayGridUnavailable() {
    var grid = document.querySelector('[data-relay-grid]');
    if (!grid || grid.querySelector('[data-node-id]')) return;
    var note = grid.querySelector('.relay-grid-note');
    if (!note) {
      note = document.createElement('p');
      note.className = 'relay-grid-note';
      grid.appendChild(note);
    }
    note.textContent = 'The live relay snapshot could not be loaded right now. The signed node list is still published in the registry.';
    grid.setAttribute('aria-busy', 'false');
  }

  function loadNetworkStats() {
    if (!window.fetch) {
      showRelayGridUnavailable();
      return;
    }
    fetch('network-stats.json', { cache: 'no-cache' })
      .then(function (response) {
        if (!response.ok) throw new Error('network-stats.json: HTTP ' + response.status);
        return response.json();
      })
      .then(renderNetworkStats)
      .catch(function () {
        // Snapshot unavailable: keep the neutral placeholders.
        updateSnapshotStatus('Live relay snapshot is unavailable right now. Placeholders are shown.');
        showRelayGridUnavailable();
      });
  }

  // --- Network history trends (bounded snapshot history) ---
  var SVG_NS = 'http://www.w3.org/2000/svg';
  var TREND_COLORS = ['#a29bfe', '#00cec9', '#00d68f', '#fdcb6e', '#ff6b6b', '#7d6ff0', '#8a8aa8', '#f2f2fa'];
  var MAX_TREND_RELAYS = 8;
  var trendUid = 0;

  function finiteNumber(value) {
    return typeof value === 'number' && isFinite(value);
  }

  function latestFinite(values) {
    for (var i = values.length - 1; i >= 0; i--) {
      if (finiteNumber(values[i])) return values[i];
    }
    return null;
  }

  function normalizeSnapshots(doc) {
    if (!doc || typeof doc !== 'object' || !Array.isArray(doc.snapshots)) return [];
    return doc.snapshots.filter(function (snap) {
      return snap && typeof snap === 'object' && finiteNumber(snap.t);
    }).sort(function (a, b) { return a.t - b.t; });
  }

  function cumulativeDelta(current, previous, reset) {
    if (!finiteNumber(current)) return null;
    if (reset || !finiteNumber(previous) || current < previous) return current;
    return current - previous;
  }

  function perRelayDeltas(snap, prevSnap) {
    var out = {};
    var perRelay = snap && snap.per_relay;
    if (!perRelay || typeof perRelay !== 'object') return out;
    Object.keys(perRelay).forEach(function (id) {
      var current = perRelay[id];
      if (!current || typeof current !== 'object') return;
      if (current.delta && typeof current.delta === 'object') {
        out[id] = {
          relayed: finiteNumber(current.delta.packets_relayed) ? current.delta.packets_relayed : null,
          dropped: finiteNumber(current.delta.packets_dropped) ? current.delta.packets_dropped : null
        };
        return;
      }
      var prev = prevSnap && prevSnap.per_relay ? prevSnap.per_relay[id] : null;
      out[id] = {
        relayed: cumulativeDelta(current.packets_relayed, prev && prev.packets_relayed, current.reset === true),
        dropped: cumulativeDelta(current.packets_dropped, prev && prev.packets_dropped, current.reset === true)
      };
    });
    return out;
  }

  function buildTrendModel(snapshots) {
    var totals = {};
    var points = snapshots.map(function (snap, index) {
      var deltas = perRelayDeltas(snap, snapshots[index - 1]);
      Object.keys(deltas).forEach(function (id) {
        var entry = deltas[id];
        totals[id] = (totals[id] || 0) + (entry.relayed || 0) + (entry.dropped || 0);
      });

      var interval = snap.interval && typeof snap.interval === 'object' ? snap.interval : {};
      var latencyMs = null;
      if (finiteNumber(interval.relay_latency_us_sum) &&
          finiteNumber(interval.relay_latency_us_count) &&
          interval.relay_latency_us_count > 0) {
        latencyMs = interval.relay_latency_us_sum / interval.relay_latency_us_count / 1000;
      }

      var fecRatio = null;
      if (finiteNumber(interval.fec_recoveries) && finiteNumber(interval.fec_losses) &&
          interval.fec_recoveries + interval.fec_losses > 0) {
        fecRatio = (interval.fec_recoveries / (interval.fec_recoveries + interval.fec_losses)) * 100;
      }

      var directAppMs = null;
      if (finiteNumber(interval.direct_app_ms_sum) && finiteNumber(interval.direct_app_ms_count) &&
          interval.direct_app_ms_count > 0) {
        directAppMs = interval.direct_app_ms_sum / interval.direct_app_ms_count;
      }

      var icmpSavedMs = null;
      if (finiteNumber(interval.saved_ms_sum) && finiteNumber(interval.saved_ms_count) &&
          interval.saved_ms_count > 0) {
        icmpSavedMs = interval.saved_ms_sum / interval.saved_ms_count;
      }

      var savedMs = finiteNumber(directAppMs) ? directAppMs : icmpSavedMs;

      var negativeShare = null;
      if (finiteNumber(interval.saved_app_ms_count) && interval.saved_app_ms_count > 0 &&
          finiteNumber(interval.saved_app_ms_negative_count)) {
        negativeShare = (interval.saved_app_ms_negative_count / interval.saved_app_ms_count) * 100;
      }

      return {
        t: snap.t, relays: deltas, latencyMs: latencyMs, fecRatio: fecRatio,
        savedMs: savedMs, savedIsDirectApp: finiteNumber(directAppMs),
        negativeShare: negativeShare
      };
    });

    var relayIds = Object.keys(totals).sort(function (a, b) {
      return totals[b] - totals[a];
    }).slice(0, MAX_TREND_RELAYS);

    return {
      points: points,
      relayIds: relayIds,
      hasLatency: points.some(function (point) { return finiteNumber(point.latencyMs); }),
      hasFec: points.some(function (point) { return finiteNumber(point.fecRatio); }),
      hasSaved: points.some(function (point) { return finiteNumber(point.savedMs); }),
      hasDirectApp: points.some(function (point) { return point.savedIsDirectApp; }),
      hasNegativeShare: points.some(function (point) { return finiteNumber(point.negativeShare); }),
      hasIcmpSavedFallback: points.some(function (point) {
        return finiteNumber(point.savedMs) && !point.savedIsDirectApp;
      })
    };
  }

  function formatMs(value) {
    if (!finiteNumber(value)) return '--';
    return (value >= 10 ? value.toFixed(0) : value.toFixed(1)) + ' ms';
  }

  function formatPercent(value) {
    if (!finiteNumber(value)) return '--';
    return value.toFixed(1) + '%';
  }

  function formatMsAxis(value) {
    if (!finiteNumber(value)) return '--';
    return (value >= 10 ? value.toFixed(0) : value.toFixed(1)) + ' ms';
  }

  function formatPercentAxis(value) {
    if (!finiteNumber(value)) return '--';
    return Math.round(value) + '%';
  }

  function formatCountAxis(value) {
    return formatCount(Math.round(value));
  }

  function formatShortTime(epochSecs) {
    return new Date(epochSecs * 1000).toLocaleString(undefined, {
      month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit'
    });
  }

  function svgEl(name, attrs) {
    var el = document.createElementNS(SVG_NS, name);
    Object.keys(attrs || {}).forEach(function (key) { el.setAttribute(key, attrs[key]); });
    return el;
  }

  function niceCeil(value) {
    if (!(value > 0)) return 1;
    if (value <= 10) return Math.ceil(value);
    var exponent = Math.floor(Math.log10(value));
    var base = Math.pow(10, exponent);
    var scaled = value / base;
    var step = scaled <= 1 ? 1 : scaled <= 2 ? 2 : scaled <= 5 ? 5 : 10;
    return step * base;
  }

  function axisFractions(max) {
    if (max <= 1) return [0, 1];
    if (max <= 4) return [0, 0.5, 1];
    return [0, 0.25, 0.5, 0.75, 1];
  }

  function buildLineChartSvg(spec) {
    var W = 520;
    var H = 220;
    var padL = 70;
    var padR = 16;
    var padT = 16;
    var padB = 34;
    var uid = 'trend-' + (++trendUid);
    var svg = svgEl('svg', {
      viewBox: '0 0 ' + W + ' ' + H,
      class: 'trend-svg',
      role: 'img',
      'aria-labelledby': uid + '-title ' + uid + '-desc'
    });

    var titleEl = svgEl('title', { id: uid + '-title' });
    titleEl.textContent = spec.title + ' trend';
    var descEl = svgEl('desc', { id: uid + '-desc' });
    descEl.textContent = spec.description;
    svg.appendChild(titleEl);
    svg.appendChild(descEl);

    var n = spec.points.length;
    function xFor(index) {
      if (n <= 1) return padL + (W - padL - padR) / 2;
      return padL + (index / (n - 1)) * (W - padL - padR);
    }

    var values = [];
    spec.series.forEach(function (series) {
      series.values.forEach(function (value) { if (finiteNumber(value)) values.push(value); });
    });
    var max = niceCeil(Math.max.apply(null, values));
    function yFor(value) {
      return H - padB - (Math.max(0, value) / max) * (H - padT - padB);
    }

    axisFractions(max).forEach(function (fraction) {
      var y = yFor(max * fraction);
      svg.appendChild(svgEl('line', {
        x1: padL, y1: y, x2: W - padR, y2: y, class: 'trend-grid-line'
      }));
      var label = svgEl('text', {
        x: padL - 8, y: y, class: 'trend-axis-label', 'text-anchor': 'end', 'dominant-baseline': 'middle'
      });
      label.textContent = spec.axisFormat(max * fraction);
      svg.appendChild(label);
    });

    spec.series.forEach(function (series) {
      var path = '';
      var penDown = false;
      series.values.forEach(function (value, index) {
        if (!finiteNumber(value)) { penDown = false; return; }
        path += (penDown ? ' L ' : ' M ') + xFor(index).toFixed(1) + ' ' + yFor(value).toFixed(1);
        penDown = true;
      });
      if (path) {
        svg.appendChild(svgEl('path', { d: path, class: 'trend-series', stroke: series.color }));
      }
      if (n <= 60) {
        series.values.forEach(function (value, index) {
          if (!finiteNumber(value)) return;
          svg.appendChild(svgEl('circle', {
            cx: xFor(index), cy: yFor(value), r: 2.4, fill: series.color, class: 'trend-dot'
          }));
        });
      }
    });

    var tickIndexes = n <= 1 ? [0] : [0, Math.floor((n - 1) / 2), n - 1];
    if (n > 2 && window.innerWidth <= 480) tickIndexes = [0, n - 1];
    var uniqueTicks = tickIndexes.filter(function (value, index, arr) { return arr.indexOf(value) === index; });
    uniqueTicks.forEach(function (index, position) {
      var anchor = uniqueTicks.length === 1 ? 'middle'
        : position === 0 ? 'start'
        : position === uniqueTicks.length - 1 ? 'end' : 'middle';
      var label = svgEl('text', {
        x: xFor(index), y: H - 10, class: 'trend-axis-label', 'text-anchor': anchor
      });
      label.textContent = formatShortTime(spec.points[index].t);
      svg.appendChild(label);
    });

    return svg;
  }

  function renderTrendCard(spec) {
    var card = document.createElement('figure');
    card.className = 'trend-card';

    var title = document.createElement('h4');
    title.className = 'trend-card-title';
    title.textContent = spec.title;
    card.appendChild(title);

    if (spec.subtitle) {
      var subtitle = document.createElement('p');
      subtitle.className = 'trend-card-subtitle';
      subtitle.textContent = spec.subtitle;
      card.appendChild(subtitle);
    }

    var hasData = spec.series.some(function (series) {
      return series.values.some(finiteNumber);
    });

    if (!hasData) {
      var noData = document.createElement('p');
      noData.className = 'trend-no-data';
      noData.textContent = spec.emptyMessage;
      card.appendChild(noData);
      return card;
    }

    card.appendChild(buildLineChartSvg(spec));

    if (spec.series.length > 1) {
      var legend = document.createElement('div');
      legend.className = 'trend-legend';
      spec.series.forEach(function (series) {
        var item = document.createElement('span');
        item.className = 'trend-legend-item';
        var swatch = document.createElement('span');
        swatch.className = 'trend-legend-swatch';
        swatch.style.background = series.color;
        var name = document.createElement('span');
        name.textContent = series.label;
        item.appendChild(swatch);
        item.appendChild(name);
        legend.appendChild(item);
      });
      card.appendChild(legend);
    }

    var summary = document.createElement('p');
    summary.className = 'trend-card-summary';
    summary.textContent = spec.summary;
    card.appendChild(summary);

    if (spec.note) {
      var note = document.createElement('p');
      note.className = 'trend-note';
      note.textContent = spec.note;
      card.appendChild(note);
    }

    if (spec.caveat) {
      var caveat = document.createElement('p');
      caveat.className = 'trend-caveat';
      caveat.textContent = spec.caveat;
      card.appendChild(caveat);
    }

    return card;
  }

  function relaySeries(model, key) {
    return model.relayIds.map(function (id, index) {
      return {
        label: id,
        color: TREND_COLORS[index % TREND_COLORS.length],
        values: model.points.map(function (point) {
          var entry = point.relays[id];
          return entry ? entry[key] : null;
        })
      };
    });
  }

  function latestRelaySum(model, key) {
    var last = model.points[model.points.length - 1];
    if (!last) return null;
    var total = 0;
    var found = false;
    model.relayIds.forEach(function (id) {
      var entry = last.relays[id];
      if (entry && finiteNumber(entry[key])) { total += entry[key]; found = true; }
    });
    return found ? total : null;
  }

  function windowSummary(model) {
    var first = model.points[0];
    var last = model.points[model.points.length - 1];
    return model.points.length + ' snapshot' + (model.points.length === 1 ? '' : 's') +
      ' · ' + formatShortTime(first.t) + ' to ' + formatShortTime(last.t);
  }

  function chartDescription(title, series, format) {
    var parts = series.map(function (s) {
      return s.label + ' latest ' + format(latestFinite(s.values));
    });
    return 'Line chart of ' + title.toLowerCase() + '. ' + parts.join('; ') + '.';
  }

  function showTrendsEmpty(message) {
    var grid = document.getElementById('trends-grid');
    var empty = document.getElementById('trends-empty');
    if (grid) grid.textContent = '';
    if (!empty) return;
    var text = empty.querySelector('[data-trends-empty-message]');
    if (text) text.textContent = message;
    empty.hidden = false;
  }

  // --- Per-source measured quality (latest interval, per relay) ---
  // The relay withholds a client-locale cell until at least three distinct
  // sources back it, so a row is shown only once the reconstructed interval
  // holds that many paired reports. Thinner cells are counted, never
  // estimated, and no country, address, or identifier is rendered.
  var MIN_SOURCE_REPORTS = 3;

  function sourceDelta(current, previous, reset) {
    var c = finiteNumber(current) ? current : 0;
    if (reset || !finiteNumber(previous) || c < previous) return c;
    return c - previous;
  }

  function buildSourceRows(snapshots) {
    if (snapshots.length < 2) return { rows: [], suppressed: 0 };
    var latest = snapshots[snapshots.length - 1];
    var previous = snapshots[snapshots.length - 2];
    var perRelay = latest && latest.per_relay;
    if (!perRelay || typeof perRelay !== 'object') return { rows: [], suppressed: 0 };

    var rows = [];
    var suppressed = 0;

    Object.keys(perRelay).sort().forEach(function (id) {
      var relay = perRelay[id];
      if (!relay || typeof relay !== 'object' || relay.reachable === false) return;
      var sources = relay.sources;
      if (!sources || typeof sources !== 'object') return;
      var prevRelay = previous && previous.per_relay ? previous.per_relay[id] : null;
      var prevSources = prevRelay && prevRelay.sources && typeof prevRelay.sources === 'object'
        ? prevRelay.sources : {};
      var reset = relay.reset === true;

      Object.keys(sources).forEach(function (region) {
        var cur = sources[region];
        if (!cur || typeof cur !== 'object') return;
        var prev = prevSources[region] || {};
        var samples = sourceDelta(cur.saved_app_ms_count, prev.saved_app_ms_count, reset);
        var sum = sourceDelta(cur.saved_app_ms_sum, prev.saved_app_ms_sum, reset);
        var negative = sourceDelta(cur.saved_app_ms_negative_count, prev.saved_app_ms_negative_count, reset);
        if (!(samples > 0)) return;
        if (samples < MIN_SOURCE_REPORTS) { suppressed += 1; return; }
        rows.push({
          relay: id,
          region: region,
          samples: samples,
          mean: sum / samples,
          worseShare: (negative / samples) * 100
        });
      });
    });

    rows.sort(function (a, b) {
      if (b.worseShare !== a.worseShare) return b.worseShare - a.worseShare;
      return a.mean - b.mean;
    });
    return { rows: rows, suppressed: suppressed };
  }

  function syncTableScrollHint() {
    var wrap = document.getElementById('source-quality-table-wrap');
    var hint = document.querySelector('[data-table-scroll-hint]');
    if (!wrap || !hint) return;
    var scrollable = wrap.scrollWidth > wrap.clientWidth + 2;
    wrap.classList.toggle('is-scrollable', scrollable);
    hint.hidden = !scrollable;
  }

  function renderSourceQuality(snapshots) {
    var section = document.getElementById('source-quality');
    if (!section) return;
    var body = section.querySelector('[data-source-quality-body]');
    var note = section.querySelector('[data-source-quality-note]');
    if (!body || !note) return;

    var result = buildSourceRows(snapshots || []);
    body.textContent = '';

    if (!result.rows.length) {
      section.hidden = true;
      return;
    }

    result.rows.forEach(function (row) {
      var tr = document.createElement('tr');
      var cells = [
        { label: 'Relay', text: row.relay },
        { label: 'Client region', text: row.region.toUpperCase() },
        { label: 'Reports', text: formatCount(row.samples), numeric: true },
        { label: 'Saved', text: formatMs(row.mean), numeric: true },
        { label: 'Worse', text: formatPercent(row.worseShare), numeric: true }
      ];
      cells.forEach(function (cell) {
        var td = document.createElement('td');
        var label = document.createElement('span');
        label.className = 'source-quality-label';
        label.textContent = cell.label;
        var value = document.createElement('span');
        value.className = 'source-quality-value';
        value.textContent = cell.text;
        td.appendChild(label);
        td.appendChild(value);
        if (cell.numeric) td.className = 'source-quality-num';
        tr.appendChild(td);
      });
      body.appendChild(tr);
    });

    var noteText = 'Positive saved means the relay was faster than the direct path for that client region; a higher worse share means it made things worse. Each row needs at least ' +
      MIN_SOURCE_REPORTS + ' paired client reports.';
    if (result.suppressed) {
      noteText += ' ' + result.suppressed + ' thinner cell(s) are withheld below the floor.';
    }
    note.textContent = noteText;
    section.hidden = false;
    syncTableScrollHint();
  }

  var tableHintTicking = false;
  window.addEventListener('resize', function () {
    if (tableHintTicking) return;
    tableHintTicking = true;
    window.requestAnimationFrame(function () {
      tableHintTicking = false;
      syncTableScrollHint();
    });
  }, { passive: true });

  function renderTrends(doc) {
    var grid = document.getElementById('trends-grid');
    var empty = document.getElementById('trends-empty');
    if (!grid || !empty) return;

    var snapshots = normalizeSnapshots(doc);
    applyLifetime(snapshots);
    if (!snapshots.length) {
      renderSourceQuality([]);
      showTrendsEmpty('No network history has been published yet. Trends appear after the first scheduled snapshot is recorded.');
      return;
    }

    var model = buildTrendModel(snapshots);
    if (!model.relayIds.length && !model.hasLatency && !model.hasFec && !model.hasSaved) {
      renderSourceQuality(snapshots);
      showTrendsEmpty('The published history has ' + snapshots.length + ' snapshot' +
        (snapshots.length === 1 ? '' : 's') + ' but no plottable relay metrics yet.');
      return;
    }

    grid.textContent = '';
    empty.hidden = true;
    renderSourceQuality(snapshots);

    var window = windowSummary(model);

    var heroSaved = document.getElementById('hero-ping-saved');
    if (heroSaved) {
      var savedSeries = model.points.map(function (point) { return point.savedMs; }).filter(finiteNumber);
      heroSaved.textContent = savedSeries.length ? formatMs(savedSeries[savedSeries.length - 1]) : 'collecting';
    }

    var heroNegative = document.getElementById('hero-negative-share');
    if (heroNegative) {
      var negativeSeries = model.points.map(function (point) { return point.negativeShare; }).filter(finiteNumber);
      heroNegative.textContent = negativeSeries.length ? formatPercent(negativeSeries[negativeSeries.length - 1]) : 'collecting';
    }

    if (model.relayIds.length) {
      var relayedSeries = relaySeries(model, 'relayed');
      grid.appendChild(renderTrendCard({
        title: 'Packets relayed per interval',
        subtitle: 'Packets each relay forwarded since the previous snapshot, per relay.',
        series: relayedSeries,
        points: model.points,
        axisFormat: formatCountAxis,
        description: chartDescription('Packets relayed per interval', relayedSeries, formatCount),
        summary: 'Latest: ' + formatCount(latestRelaySum(model, 'relayed')) + ' relayed · ' + window,
        emptyMessage: 'This history does not carry per-relay packet counters.'
      }));

      var droppedSeries = relaySeries(model, 'dropped');
      grid.appendChild(renderTrendCard({
        title: 'Packets filtered per interval',
        subtitle: 'Packets each relay dropped or filtered since the previous snapshot (includes unauthenticated scans).',
        series: droppedSeries,
        points: model.points,
        axisFormat: formatCountAxis,
        description: chartDescription('Packets filtered per interval', droppedSeries, formatCount),
        summary: 'Latest: ' + formatCount(latestRelaySum(model, 'dropped')) + ' filtered · ' + window,
        emptyMessage: 'This history does not carry per-relay drop counters.'
      }));
    }

    var latencyValues = model.points.map(function (point) { return point.latencyMs; });
    grid.appendChild(renderTrendCard({
      title: 'Mean upstream response lag',
      subtitle: 'Relay-observed upstream response lag per snapshot (sum ÷ count of latency samples).',
      series: [{ label: 'Network mean', color: TREND_COLORS[0], values: latencyValues }],
      points: model.points,
      axisFormat: formatMsAxis,
      description: chartDescription('Mean upstream response lag', [{ label: 'network mean', values: latencyValues }], formatMs),
      summary: 'Latest: ' + formatMs(latestFinite(latencyValues)) + ' · ' + window,
      emptyMessage: 'This history does not carry latency sum/count counters, so there is no mean to plot.',
      caveat: 'Mean of proxy-observed lag, not client RTT. It measures how long the relay waited for upstream responses, not the ping you would see in-game.'
    }));

    var savedValues = model.points.map(function (point) { return point.savedMs; });
    var likeForLike = model.hasDirectApp && !model.hasIcmpSavedFallback;
    var savedSubtitle = likeForLike
      ? 'Direct application RTT, from a small sample of the game\'s own packets on the un-relayed path, minus the same traffic through the relay, averaged over client reports (telemetry is on by default, opt-out). Positive means the tunnelled path was faster.'
      : 'ICMP round trip to the game server minus the tunnelled game-traffic round trip, averaged over client reports (telemetry is on by default, opt-out). Positive means the tunnelled path was faster.';
    var savedCaveat;
    if (likeForLike) {
      savedCaveat = 'Both numbers are game-packet round trips: direct samples the game\'s own packets on the un-relayed path, tunnelled is the same traffic through the relay. Like-for-like, so this is a direct comparison of the two paths.';
    } else if (model.hasDirectApp) {
      savedCaveat = 'Some reports compare the game\'s own packets directly (like-for-like); older reports use an ICMP echo for the direct side. They are mixed here, so treat this as an estimate.';
    } else {
      savedCaveat = 'Direct is the client\'s ICMP echo to the game server. Tunnelled is the round trip of the game traffic itself through the relay, which includes the server\'s own processing time. They are different instruments, so treat this as an estimate, not a measurement of in-game ping.';
    }
    var negativeShareValues = model.points.map(function (point) { return point.negativeShare; });
    var latestNegativeShare = latestFinite(negativeShareValues);
    var negativeShareNote = finiteNumber(latestNegativeShare)
      ? 'In the latest interval, ' + formatPercent(latestNegativeShare) + ' of paired client reports found the relayed app RTT worse than the direct app RTT. LightSpeed does not help every connection, and this is the share where it made things worse.'
      : 'No paired client reports yet, so the share of reports where the relayed path was worse cannot be shown.';
    grid.appendChild(renderTrendCard({
      title: 'RTT saved by LightSpeed',
      subtitle: savedSubtitle,
      series: [{ label: 'Saved', color: TREND_COLORS[2], values: savedValues }],
      points: model.points,
      axisFormat: formatMsAxis,
      description: chartDescription('RTT saved by LightSpeed', [{ label: 'saved', values: savedValues }], formatMs),
      summary: 'Latest: ' + formatMs(latestFinite(savedValues)) + ' · ' + window,
      emptyMessage: 'No client latency reports yet, so there is no direct-versus-tunnelled comparison to plot.',
      caveat: savedCaveat,
      note: negativeShareNote
    }));

    var fecValues = model.points.map(function (point) { return point.fecRatio; });
    grid.appendChild(renderTrendCard({
      title: 'FEC recovery ratio',
      subtitle: 'Share of recorded FEC events that recovered data: recoveries ÷ (recoveries + losses).',
      series: [{ label: 'Recovery ratio', color: TREND_COLORS[1], values: fecValues }],
      points: model.points,
      axisFormat: formatPercentAxis,
      description: chartDescription('FEC recovery ratio', [{ label: 'recovery ratio', values: fecValues }], formatPercent),
      summary: 'Latest: ' + formatPercent(latestFinite(fecValues)) + ' · ' + window,
      emptyMessage: 'This history does not carry FEC recovery/loss counters, so there is no ratio to plot.'
    }));
  }

  function loadNetworkHistory() {
    if (!window.fetch) return;
    fetch('network-history.json', { cache: 'no-cache' })
      .then(function (response) {
        if (!response.ok) throw new Error('network-history.json: HTTP ' + response.status);
        return response.json();
      })
      .then(renderTrends)
      .catch(function () {
        renderSourceQuality([]);
        showTrendsEmpty('Network history is unavailable or unreadable right now. Trends appear once the collector publishes a readable snapshot.');
      });
  }

  // --- Download: platform detection, copy-to-clipboard, latest release tag ---
  var RELEASE_DOWNLOAD_BASE = 'https://github.com/ShibbityShwab/lightspeed/releases/latest/download/';
  var RELEASE_LATEST_PAGE = 'https://github.com/ShibbityShwab/lightspeed/releases/latest';
  var RELEASE_TAG_KEY = 'lightspeed:release-tag';
  var RELEASE_TAG_TTL_MS = 60 * 60 * 1000;

  // Best artifact for each detected platform/arch pair. Windows ARM64 falls
  // back to the x64 MSI (runs under emulation) and Linux ARM64 has no GUI
  // build yet, so both resolve to the closest real asset.
  function downloadTarget(platform, arch) {
    if (platform === 'windows') {
      return { asset: 'lightspeed-gui-x86_64-pc-windows-msvc.msi', label: 'Windows (x64)' };
    }
    if (platform === 'macos') {
      if (arch === 'aarch64') {
        return { asset: 'lightspeed-client-aarch64-apple-darwin.tar.xz', label: 'macOS (Apple Silicon)' };
      }
      return { asset: 'lightspeed-client-x86_64-apple-darwin.tar.xz', label: 'macOS (Intel)' };
    }
    if (platform === 'linux') {
      if (arch === 'aarch64') {
        return { asset: 'lightspeed-client-aarch64-unknown-linux-gnu.tar.xz', label: 'Linux (ARM64)' };
      }
      return { asset: 'lightspeed-gui-x86_64-unknown-linux-gnu.tar.xz', label: 'Linux (x64)' };
    }
    return null;
  }

  function normalizePlatform(value) {
    var v = String(value || '').toLowerCase();
    if (v.indexOf('iphone') !== -1 || v.indexOf('ipad') !== -1 || v.indexOf('ipod') !== -1) return '';
    if (v.indexOf('android') !== -1) return '';
    if (v.indexOf('win') !== -1) return 'windows';
    if (v.indexOf('mac') !== -1 || v.indexOf('darwin') !== -1) return 'macos';
    if (v.indexOf('linux') !== -1 || v.indexOf('x11') !== -1 || v.indexOf('ubuntu') !== -1 || v.indexOf('fedora') !== -1) return 'linux';
    return '';
  }

  function normalizeArch(value) {
    var v = String(value || '').toLowerCase();
    if (v === 'arm' || v.indexOf('arm64') !== -1 || v.indexOf('aarch64') !== -1 || v.indexOf('armv8') !== -1) return 'aarch64';
    if (v.indexOf('x86') !== -1 || v.indexOf('x64') !== -1 || v.indexOf('amd64') !== -1 || v.indexOf('win64') !== -1 || v.indexOf('wow64') !== -1 || v.indexOf('intel') !== -1) return 'x86_64';
    return '';
  }

  function detectPlatform() {
    var nav = navigator;
    var hints;
    if (nav.userAgentData && typeof nav.userAgentData.getHighEntropyValues === 'function') {
      hints = nav.userAgentData.getHighEntropyValues(['architecture', 'bitness'])
        .then(function (values) {
          return { platform: nav.userAgentData.platform, arch: values.architecture };
        })
        .catch(function () {
          return { platform: nav.userAgentData.platform, arch: '' };
        });
    } else {
      hints = Promise.resolve({ platform: nav.platform || '', arch: '' });
    }
    return hints.then(function (values) {
      var platform = normalizePlatform(values.platform || nav.platform || '') ||
        normalizePlatform(nav.userAgent || '');
      var arch = normalizeArch(values.arch || '') || normalizeArch(nav.userAgent || '');
      return { platform: platform, arch: arch };
    });
  }

  function initDownload() {
    var button = document.getElementById('primary-download');
    var label = document.getElementById('primary-download-label');
    var note = document.getElementById('primary-download-note');
    if (!button) return;

    function showFallback() {
      button.href = RELEASE_LATEST_PAGE;
      if (label) label.textContent = 'Download the latest release';
      if (note) note.textContent = 'Pick your platform on GitHub Releases.';
    }

    detectPlatform().then(function (target) {
      var best = downloadTarget(target.platform, target.arch);
      if (!best) {
        showFallback();
        return;
      }
      button.href = RELEASE_DOWNLOAD_BASE + best.asset;
      button.setAttribute('data-asset', best.asset);
      if (label) label.textContent = 'Download for ' + best.label;
      if (note) note.textContent = best.asset;
    }).catch(showFallback);
  }

  function copyText(text) {
    if (navigator.clipboard && navigator.clipboard.writeText) {
      return navigator.clipboard.writeText(text);
    }
    return new Promise(function (resolve, reject) {
      var area = document.createElement('textarea');
      area.value = text;
      area.setAttribute('readonly', '');
      area.style.position = 'fixed';
      area.style.opacity = '0';
      document.body.appendChild(area);
      area.select();
      var ok = false;
      try { ok = document.execCommand('copy'); } catch (err) { ok = false; }
      document.body.removeChild(area);
      if (ok) resolve(); else reject(new Error('copy failed'));
    });
  }

  function initCopyButtons() {
    var statusEl = document.getElementById('copy-status');
    document.querySelectorAll('.copy-btn').forEach(function (btn) {
      btn.addEventListener('click', function () {
        var wrapper = btn.closest('.code-copy');
        var code = wrapper ? wrapper.querySelector('code') : null;
        if (!code) return;
        var original = btn.textContent;
        copyText(code.textContent.trim()).then(function () {
          btn.textContent = 'Copied!';
          btn.classList.add('copied');
          if (statusEl) statusEl.textContent = 'Copied: ' + code.textContent.trim();
        }).catch(function () {
          btn.textContent = 'Select + copy';
          if (statusEl) statusEl.textContent = 'Copy failed. Select the command and copy it manually.';
        }).then(function () {
          setTimeout(function () {
            btn.textContent = original;
            btn.classList.remove('copied');
          }, 1600);
        });
      });
    });
  }

  function loadLatestReleaseTag() {
    var slots = document.querySelectorAll('[data-release-tag]');
    if (!slots.length) return;

    function applyTag(tag) {
      slots.forEach(function (el) { el.textContent = tag; });
    }

    function readCache() {
      try {
        var raw = sessionStorage.getItem(RELEASE_TAG_KEY);
        if (!raw) return null;
        var parsed = JSON.parse(raw);
        if (parsed && typeof parsed.tag === 'string' && typeof parsed.at === 'number' &&
            (Date.now() - parsed.at) < RELEASE_TAG_TTL_MS) {
          return parsed.tag;
        }
      } catch (err) {
        // Storage unavailable: fall through to the network fetch.
      }
      return null;
    }

    function writeCache(tag) {
      try {
        sessionStorage.setItem(RELEASE_TAG_KEY, JSON.stringify({ tag: tag, at: Date.now() }));
      } catch (err) {
        // Storage unavailable: nothing to do.
      }
    }

    var cached = readCache();
    if (cached) {
      applyTag(cached);
      return;
    }

    if (!window.fetch) return;
    fetch('https://api.github.com/repos/ShibbityShwab/lightspeed/releases/latest', {
      headers: { Accept: 'application/vnd.github+json' }
    })
      .then(function (response) {
        if (!response.ok) throw new Error('releases/latest: HTTP ' + response.status);
        return response.json();
      })
      .then(function (release) {
        var tag = release && typeof release.tag_name === 'string' ? release.tag_name : '';
        if (!tag) return;
        applyTag(tag);
        writeCache(tag);
      })
      .catch(function () {
        // Keep the versionless fallback copy that is already in the markup.
      });
  }

  initDownload();
  initCopyButtons();
  loadLatestReleaseTag();

  loadNetworkStats();
  loadNetworkHistory();

})();
