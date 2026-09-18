// LightSpeed Landing Page - app.js
// Minimal JS: mobile nav, scroll animations, nav background

(function () {
  'use strict';

  // --- Mobile Nav Toggle ---
  const toggle = document.getElementById('nav-toggle');
  const navLinks = document.getElementById('nav-links');
  if (toggle && navLinks) {
    toggle.addEventListener('click', () => {
      navLinks.classList.toggle('open');
      toggle.classList.toggle('active');
    });
    // Close on link click
    navLinks.querySelectorAll('a').forEach(link => {
      link.addEventListener('click', () => {
        navLinks.classList.remove('open');
        toggle.classList.remove('active');
      });
    });
  }

  // --- Scroll: Nav background solidify ---
  const nav = document.getElementById('nav');
  if (nav) {
    window.addEventListener('scroll', () => {
      if (window.scrollY > 80) {
        nav.style.background = 'rgba(10, 10, 26, 0.97)';
      } else {
        nav.style.background = 'rgba(10, 10, 26, 0.85)';
      }
    }, { passive: true });
  }

  // --- Scroll: Fade-in elements ---
  const faders = document.querySelectorAll('.step, .game-card, .bench-card, .compare-card, .download-card, .faq-item, .relay-card');
  if (faders.length && 'IntersectionObserver' in window) {
    faders.forEach(el => el.classList.add('fade-in'));
    const observer = new IntersectionObserver((entries) => {
      entries.forEach(entry => {
        if (entry.isIntersecting) {
          entry.target.classList.add('visible');
          observer.unobserve(entry.target);
        }
      });
    }, { threshold: 0.15, rootMargin: '0px 0px -40px 0px' });
    faders.forEach(el => observer.observe(el));
  }

  // --- Animate benchmark bars on scroll ---
  const benchBars = document.querySelectorAll('.bench-bar-fill:not(.bench-bar-live)');
  if (benchBars.length && 'IntersectionObserver' in window) {
    benchBars.forEach(bar => {
      const targetWidth = bar.style.width;
      bar.style.width = '0%';
      const barObserver = new IntersectionObserver((entries) => {
        entries.forEach(entry => {
          if (entry.isIntersecting) {
            setTimeout(() => { bar.style.width = targetWidth; }, 200);
            barObserver.unobserve(entry.target);
          }
        });
      }, { threshold: 0.5 });
      barObserver.observe(bar);
    });
  }

  // --- Smooth scroll for anchor links (fallback) ---
  document.querySelectorAll('a[href^="#"]').forEach(anchor => {
    anchor.addEventListener('click', function (e) {
      const target = document.querySelector(this.getAttribute('href'));
      if (target) {
        e.preventDefault();
        target.scrollIntoView({ behavior: 'smooth', block: 'start' });
      }
    });
  });

  // --- Live network stats from network-stats.json ---
  function formatUptime(secs) {
    if (typeof secs !== 'number' || !isFinite(secs) || secs < 0) return '--';
    if (secs < 60) return '<1m';
    const days = Math.floor(secs / 86400);
    const hours = Math.floor((secs % 86400) / 3600);
    const mins = Math.floor((secs % 3600) / 60);
    if (days > 0) return days + 'd ' + hours + 'h';
    if (hours > 0) return hours + 'h ' + mins + 'm';
    return mins + 'm';
  }

  function formatTimestamp(epochSecs, style) {
    if (style === 'relative') {
      const diff = Math.max(0, Math.floor(Date.now() / 1000) - epochSecs);
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

  function relayStatus(status) {
    const value = String(status || '').toLowerCase();
    if (value === 'healthy' || value === 'online' || value === 'up') {
      return { label: 'Online', state: 'healthy', cardClass: 'relay-status' };
    }
    if (value === 'degraded') {
      return { label: 'Degraded', state: 'degraded', cardClass: 'relay-status relay-status-degraded' };
    }
    return { label: 'Offline', state: 'down', cardClass: 'relay-status relay-status-offline' };
  }

  function renderLastChecked(epochSecs) {
    const absolute = formatTimestamp(epochSecs, 'absolute');
    const iso = new Date(epochSecs * 1000).toISOString();
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

  function renderRelayCards(relays) {
    const byId = {};
    relays.forEach(function (relay) {
      if (relay && relay.node_id) byId[relay.node_id] = relay;
    });
    document.querySelectorAll('[data-node-id]').forEach(function (card) {
      const relay = byId[card.getAttribute('data-node-id')];
      if (!relay) return;
      const status = relayStatus(relay.status);

      const statusEl = card.querySelector('.relay-status');
      if (statusEl) {
        statusEl.className = status.cardClass;
        const label = statusEl.querySelector('[data-relay-status]');
        if (label) label.textContent = status.label;
      }

      const versionEl = card.querySelector('[data-relay-version]');
      if (versionEl && relay.version) versionEl.textContent = relay.version;

      const uptimeEl = card.querySelector('[data-relay-uptime]');
      if (uptimeEl) uptimeEl.textContent = formatUptime(relay.uptime_secs);

      const relayedEl = card.querySelector('[data-relay-packets-relayed]');
      if (relayedEl) relayedEl.textContent = formatCount(relay.packets_relayed);

      const droppedEl = card.querySelector('[data-relay-packets-dropped]');
      if (droppedEl) droppedEl.textContent = formatCount(relay.packets_dropped);

      const sessionsEl = card.querySelector('[data-relay-sessions]');
      if (sessionsEl) sessionsEl.textContent = formatCount(relay.sessions_created);
    });
  }

  function renderRelayHealthList(relays) {
    const list = document.getElementById('relay-health-list');
    if (!list || !relays.length) return;
    list.textContent = '';
    relays.forEach(function (relay) {
      const status = relayStatus(relay.status);

      const row = document.createElement('li');
      row.className = 'relay-health-row';

      const name = document.createElement('span');
      name.className = 'relay-health-name';
      name.textContent = (relay.flag ? relay.flag + ' ' : '') + (relay.location || relay.node_id);

      const state = document.createElement('span');
      state.className = 'relay-health-status is-' + status.state;
      state.textContent = status.label;

      const uptime = document.createElement('span');
      uptime.className = 'relay-health-uptime';
      uptime.textContent = formatUptime(relay.uptime_secs);

      const packets = document.createElement('span');
      packets.className = 'relay-health-packets';
      packets.textContent = formatCount(relay.packets_relayed) + ' relayed';
      packets.title = formatCount(relay.packets_relayed) + ' packets relayed, ' +
        formatCount(relay.packets_dropped) + ' filtered, ' +
        formatCount(relay.sessions_created) + ' sessions';

      row.appendChild(name);
      row.appendChild(state);
      row.appendChild(uptime);
      row.appendChild(packets);
      list.appendChild(row);
    });
  }

  function renderNetworkStats(stats) {
    if (!stats || typeof stats !== 'object') return;

    const relayCount = typeof stats.relay_count === 'number' ? stats.relay_count : null;
    const healthyCount = typeof stats.healthy_count === 'number' ? stats.healthy_count : null;

    if (relayCount !== null) setStat('relay_count', String(relayCount));
    if (typeof stats.area_count === 'number') setStat('area_count', String(stats.area_count));
    if (relayCount !== null && healthyCount !== null) {
      setStat('relays_online', healthyCount + '/' + relayCount);
    }

    const versions = Array.isArray(stats.software_versions) ? stats.software_versions : [];
    if (versions.length) {
      setStat('software_versions', versions.map(function (v) { return 'v' + v; }).join(', '));
    }

    if (typeof stats.generated_at === 'number') renderLastChecked(stats.generated_at);

    const relays = Array.isArray(stats.relays) ? stats.relays : [];
    if (relays.length) {
      renderRelayCards(relays);
      renderRelayHealthList(relays);

      let totalRelayed = 0;
      let totalDropped = 0;
      let totalUpstreamLoss = 0;
      let totalSessions = 0;
      relays.forEach(function (relay) {
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

    const healthBar = document.getElementById('network-health-bar');
    if (healthBar && relayCount && healthyCount !== null) {
      const pct = Math.max(0, Math.min(100, Math.round((healthyCount / relayCount) * 100)));
      healthBar.style.width = pct + '%';
    }
  }

  function loadNetworkStats() {
    if (!window.fetch) return;
    fetch('network-stats.json', { cache: 'no-cache' })
      .then(function (response) {
        if (!response.ok) throw new Error('network-stats.json: HTTP ' + response.status);
        return response.json();
      })
      .then(renderNetworkStats)
      .catch(function () {
        // Snapshot unavailable: keep the neutral placeholders.
      });
  }

  loadNetworkStats();

})();
