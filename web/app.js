// LightSpeed Landing Page — app.js
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
  const faders = document.querySelectorAll('.step, .game-card, .bench-card, .compare-card, .download-card, .faq-item');
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
  const benchBars = document.querySelectorAll('.bench-bar-fill');
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

})();
