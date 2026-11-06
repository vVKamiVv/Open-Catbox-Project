// Background: night sky over the sea, a moon, and golden butterflies. Everything is drawn here, no images.
(() => {
  "use strict";
  const canvas = document.getElementById("sky");
  const ctx = canvas.getContext("2d");
  const css = getComputedStyle(document.documentElement);
  const color = (name) => css.getPropertyValue(name).trim();
  const still = matchMedia("(prefers-reduced-motion: reduce)").matches;

  // one wing pair (right side) around the body at (512, 520), same drawing as the app icon
  const WING = new Path2D(
    "M520 470C560 330 700 200 850 210C930 216 900 330 840 400C790 460 660 500 530 500Z" +
    "M528 510C640 520 760 570 770 680C778 760 690 790 630 730C580 680 545 600 522 530Z");
  const HORIZON = 0.7;
  const rand = (a, b) => a + Math.random() * (b - a);

  let w = 0, h = 0, stars = [], flies = [], dust = [], wingFill;

  function resize() {
    const dpr = window.devicePixelRatio || 1;
    w = innerWidth; h = innerHeight;
    canvas.width = Math.round(w * dpr); canvas.height = Math.round(h * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    stars = Array.from({ length: 170 }, () => ({
      x: Math.random() * w, y: Math.pow(Math.random(), 1.6) * h * HORIZON,
      r: rand(0.3, 1.3), phase: rand(0, 6.3), speed: rand(0.4, 1.6),
    }));
    if (!flies.length) flies = Array.from({ length: 11 }, () => spawn(true));
    wingFill = ctx.createLinearGradient(520, 200, 840, 780);
    wingFill.addColorStop(0, color("--gold-light"));
    wingFill.addColorStop(0.5, color("--gold"));
    wingFill.addColorStop(1, color("--gold-dark"));
  }

  function spawn(anywhere) {
    return {
      x: anywhere ? rand(0, w) : rand(-40, w * 0.6),
      y: anywhere ? rand(h * 0.1, h * 0.95) : h + rand(20, 80),
      size: rand(0.035, 0.075), seed: rand(0, 100),
      flap: rand(0, 6.3), flapSpeed: rand(7, 11), drift: rand(14, 34),
    };
  }

  function drawSky(t) {
    const sky = ctx.createLinearGradient(0, 0, 0, h * HORIZON);
    sky.addColorStop(0, color("--night-0"));
    sky.addColorStop(0.65, color("--night-1"));
    sky.addColorStop(1, color("--night-2"));
    ctx.fillStyle = sky;
    ctx.fillRect(0, 0, w, h * HORIZON);

    // a faint crimson band where sky meets sea
    const band = ctx.createRadialGradient(w * 0.55, h * HORIZON, 0, w * 0.55, h * HORIZON, w * 0.7);
    band.addColorStop(0, "rgba(142, 27, 43, 0.38)");
    band.addColorStop(1, "rgba(142, 27, 43, 0)");
    ctx.fillStyle = band;
    ctx.fillRect(0, 0, w, h * HORIZON);

    for (const s of stars) {
      const a = 0.35 + 0.65 * Math.abs(Math.sin(s.phase + t * s.speed));
      ctx.fillStyle = `rgba(247, 236, 210, ${a})`;
      ctx.beginPath(); ctx.arc(s.x, s.y, s.r, 0, 6.283); ctx.fill();
    }

    const mx = w * 0.78, my = h * 0.22, mr = Math.min(w, h) * 0.06;
    const halo = ctx.createRadialGradient(mx, my, mr * 0.8, mx, my, mr * 5);
    halo.addColorStop(0, "rgba(247, 227, 166, 0.22)");
    halo.addColorStop(1, "rgba(247, 227, 166, 0)");
    ctx.fillStyle = halo;
    ctx.fillRect(mx - mr * 5, my - mr * 5, mr * 10, mr * 10);
    const moon = ctx.createRadialGradient(mx - mr * 0.3, my - mr * 0.3, mr * 0.1, mx, my, mr);
    moon.addColorStop(0, "#fffaf0");
    moon.addColorStop(1, "#e9d9ad");
    ctx.fillStyle = moon;
    ctx.beginPath(); ctx.arc(mx, my, mr, 0, 6.283); ctx.fill();

    // sea with the moon's broken reflection
    const top = h * HORIZON;
    const sea = ctx.createLinearGradient(0, top, 0, h);
    sea.addColorStop(0, "#0d1638");
    sea.addColorStop(1, color("--night-0"));
    ctx.fillStyle = sea;
    ctx.fillRect(0, top, w, h - top);
    for (let y = top + 2, i = 0; y < h; y += 5, i++) {
      const depth = (y - top) / (h - top);
      const span = mr * (0.8 + depth * 2.6) * (0.6 + 0.4 * Math.sin(i * 1.7 + t * 1.3));
      const off = Math.sin(i * 0.9 + t * 0.8) * mr * 0.6 * depth;
      ctx.fillStyle = `rgba(247, 227, 166, ${0.32 * (1 - depth * 0.7)})`;
      ctx.fillRect(mx - span / 2 + off, y, span, 1.4);
      ctx.fillStyle = `rgba(120, 140, 200, ${0.05 * (1 - depth)})`;
      ctx.fillRect(0, y + 2, w, 1);
    }
  }

  function drawFly(f) {
    const flap = 0.2 + 0.8 * Math.abs(Math.cos(f.flap));
    ctx.save();
    ctx.translate(f.x, f.y);
    ctx.rotate(Math.sin(f.seed + f.flap * 0.1) * 0.25);
    ctx.scale(f.size, f.size);
    ctx.fillStyle = wingFill;
    for (const side of [1, -1]) {
      ctx.save();
      ctx.scale(side * flap, 1);
      ctx.translate(-512, -520);
      ctx.fill(WING);
      ctx.restore();
    }
    ctx.beginPath(); ctx.ellipse(0, 0, 15, 110, 0, 0, 6.283); ctx.fill();
    ctx.restore();
  }

  function step(dt, t) {
    for (let i = 0; i < flies.length; i++) {
      const f = flies[i];
      f.flap += dt * f.flapSpeed;
      f.x += (f.drift * 0.6 + Math.sin(t * 0.6 + f.seed) * 22) * dt;
      f.y += (-f.drift + Math.cos(t * 1.1 + f.seed) * 16) * dt;
      if (f.y < -60 || f.x > w + 60) flies[i] = spawn(false);
      if (Math.random() < dt * 3) dust.push({ x: f.x, y: f.y, life: 1, vx: rand(-6, 6), vy: rand(4, 14) });
    }
    dust = dust.filter((d) => (d.life -= dt * 0.5) > 0);
    for (const d of dust) { d.x += d.vx * dt; d.y += d.vy * dt; }
  }

  function draw(t) {
    drawSky(t);
    for (const d of dust) {
      ctx.fillStyle = `rgba(247, 227, 166, ${d.life * 0.7})`;
      ctx.fillRect(d.x, d.y, 1.6, 1.6);
    }
    ctx.shadowColor = "rgba(226, 182, 90, 0.8)";
    ctx.shadowBlur = 14;
    flies.forEach(drawFly);
    ctx.shadowBlur = 0;
  }

  let last = performance.now();
  function frame(now) {
    const dt = Math.min(0.05, (now - last) / 1000);
    last = now;
    step(dt, now / 1000);
    draw(now / 1000);
    requestAnimationFrame(frame);
  }

  addEventListener("resize", () => { resize(); if (still) draw(0); });
  resize();
  if (still) draw(0); else requestAnimationFrame(frame);
})();
