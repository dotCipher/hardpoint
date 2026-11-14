// Hardpoint site. No dependencies. Everything here is an enhancement: the
// pages read correctly with scripts off, and nothing moves on its own when
// the visitor has asked for reduced motion.
(function () {
  "use strict";
  var still = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  function data(id) {
    var el = document.getElementById(id);
    return el ? JSON.parse(el.textContent) : null;
  }

  // ---- sections arrive as they scroll into view
  var seen = "IntersectionObserver" in window && !still
    ? new IntersectionObserver(function (entries) {
        entries.forEach(function (entry) {
          if (entry.isIntersecting) { entry.target.classList.add("in"); seen.unobserve(entry.target); }
        });
      }, { threshold: 0.12 })
    : null;
  document.querySelectorAll(".reveal").forEach(function (el) {
    if (seen) { seen.observe(el); } else { el.classList.add("in"); }
  });

  // ---- filter the device list
  var q = document.getElementById("q");
  if (q) {
    var rows = Array.prototype.slice.call(document.querySelectorAll("[data-search]"));
    q.addEventListener("input", function () {
      var term = q.value.trim().toLowerCase();
      rows.forEach(function (row) { row.style.display = row.dataset.search.indexOf(term) >= 0 ? "" : "none"; });
    });
  }

  // ---- the key map on a device page: point at a key to read its numbers
  var info = document.getElementById("keyinfo");
  if (info) {
    var idle = info.innerHTML;
    document.querySelectorAll(".keymap .key").forEach(function (key) {
      function show() {
        document.querySelectorAll(".keymap .key.hot").forEach(function (k) { k.classList.remove("hot"); });
        key.classList.add("hot");
        info.innerHTML = "key <b>" + key.dataset.key + "</b> · switch <b>" + key.dataset.row + "," + key.dataset.col +
          "</b> · light <b>" + key.dataset.led + "</b>";
      }
      key.addEventListener("pointerenter", show);
      key.addEventListener("focus", show);
      key.addEventListener("pointerleave", function () { key.classList.remove("hot"); info.innerHTML = idle; });
    });
  }

  // ---- device scenes: one per device in the catalog, built from its label
  var scenes = data("scene-data");
  var stage = document.getElementById("scenes");
  if (scenes && stage) {
    var pills = Array.prototype.slice.call(stage.querySelectorAll("#scene-pills .pill"));
    var pictures = Array.prototype.slice.call(stage.querySelectorAll(".drawing-wrap"));
    var el = function (id) { return document.getElementById(id); };
    var askEl = el("scene-ask"), verdict = el("scene-verdict"), chipEl = el("scene-chip"), doesEl = el("scene-does");
    var readout = el("scene-readout"), confirm = el("scene-confirm"), statusEl = el("scene-status");
    var deviceEl = el("scene-device"), dotsEl = el("scene-dots"), toggle = el("scene-toggle");
    var di = 0, ci = 0, playing = !still, queue = [], decide = null;

    function wait(ms, fn) { queue.push(setTimeout(fn, still ? 0 : ms)); }
    function stopAll() { queue.forEach(clearTimeout); queue = []; decide = null; }
    function lampOf() { return pictures[di].querySelector(".lamp"); }

    function showValue(cap, value, before) {
      readout.innerHTML = "";
      var add = function (tag, cls, text) {
        var node = document.createElement(tag); node.className = cls || ""; if (text) { node.textContent = text; }
        readout.appendChild(node); return node;
      };
      if (cap.show === "meter") {
        var bar = add("span", "bar2"); var fill = document.createElement("i"); bar.appendChild(fill);
        fill.style.width = (parseInt(before || "0", 10) || 0) + "%";
        requestAnimationFrame(function () { requestAnimationFrame(function () { fill.style.width = (parseInt(value, 10) || 0) + "%"; }); });
      } else if (cap.show === "swatch") {
        add("span", "dotc").style.background = value === cap.after ? cap.swatch : "transparent";
      } else if (cap.show === "pulse") {
        var ticks = add("span", "ticks3"); for (var n = 0; n < 3; n++) { ticks.appendChild(document.createElement("i")); }
      }
      add("span", "", value);
      if (before && before !== value) { add("span", "was", "was " + before); }
      var lamp = lampOf();
      if (lamp) { lamp.style.fill = cap.show === "swatch" && value === cap.after ? cap.swatch : "var(--c)"; }
      readout.classList.add("show");
    }

    function settle(cap, kept) {
      decide = null;
      confirm.classList.remove("on", "show", "counting");
      showValue(cap, kept ? cap.after : cap.before, kept ? cap.before : "");
      var note = document.createElement("span");
      note.className = "was";
      note.textContent = kept ? scenes.words.kept : scenes.words.undone;
      readout.appendChild(note);
      finish(cap, 2200);
    }

    function finish(cap, hold) {
      statusEl.textContent = cap.checked;
      statusEl.className = "checked show " + cap.status;
      if (playing) { wait(hold, next); }
    }

    function result(cap) {
      if (cap.risk === "gated") {
        readout.innerHTML = "";
        var why = document.createElement("span"); why.className = "why"; why.textContent = cap.harm; readout.appendChild(why);
        readout.classList.add("show");
        return finish(cap, 3600);
      }
      showValue(cap, cap.after, cap.before);
      if (cap.risk !== "guarded") { return finish(cap, 2600); }
      // The change is made, then held open: keep it, or it goes back by itself.
      confirm.classList.add("on");
      confirm.style.setProperty("--secs", "5s");
      requestAnimationFrame(function () { requestAnimationFrame(function () { confirm.classList.add("show", "counting"); }); });
      decide = function (kept) { queue.forEach(clearTimeout); queue = []; settle(cap, kept); };
      if (playing) { queue.push(setTimeout(function () { if (decide) { decide(false); } }, 5000)); }
    }

    function play() {
      stopAll();
      var device = scenes.devices[di], cap = device.caps[ci];
      pills.forEach(function (pill, n) { pill.setAttribute("aria-selected", n === di ? "true" : "false"); });
      pictures.forEach(function (pic, n) { pic.hidden = n !== di; pic.classList.remove("asking"); });
      var lamp = lampOf(); if (lamp) { lamp.style.fill = ""; }
      stage.style.setProperty("--c", "var(--" + cap.risk + ")");
      deviceEl.textContent = device.name;
      dotsEl.innerHTML = "";
      device.caps.forEach(function (_, n) { var dot = document.createElement("i"); if (n === ci) { dot.className = "on"; } dotsEl.appendChild(dot); });
      [verdict, readout, statusEl].forEach(function (node) { node.classList.remove("show"); });
      confirm.classList.remove("on", "show", "counting");
      askEl.classList.remove("done");
      askEl.textContent = "";
      var typed = 0, speed = still ? 0 : 20;
      (function type() {
        if (typed < cap.ask.length && !still) {
          askEl.textContent = cap.ask.slice(0, ++typed);
          queue.push(setTimeout(type, speed));
          return;
        }
        askEl.textContent = cap.ask;
        askEl.classList.add("done");
        wait(350, function () {
          chipEl.textContent = cap.plain;
          doesEl.textContent = cap.does;
          verdict.classList.add("show");
          pictures[di].classList.add("asking");
          wait(900, function () { result(cap); });
        });
      })();
    }

    function next() {
      ci += 1;
      if (ci >= scenes.devices[di].caps.length) { ci = 0; di = (di + 1) % scenes.devices.length; }
      play();
    }

    function setPlaying(on) {
      playing = on && !still;
      toggle.textContent = playing ? scenes.words.pause : scenes.words.play;
      if (playing) { play(); } else { queue.forEach(clearTimeout); queue = []; }
    }

    pills.forEach(function (pill, n) { pill.addEventListener("click", function () { di = n; ci = 0; play(); }); });
    el("scene-next").addEventListener("click", next);
    el("scene-keep").addEventListener("click", function () { if (decide) { decide(true); } });
    el("scene-undo").addEventListener("click", function () { if (decide) { decide(false); } });
    toggle.addEventListener("click", function () { setPlaying(!playing); });
    if (still) { toggle.hidden = true; }
    document.addEventListener("visibilitychange", function () { if (document.hidden && playing) { setPlaying(false); } });
    play();
  }

  // ---- the same request, with and without a label
  var whys = data("why-data");
  var whyPills = Array.prototype.slice.call(document.querySelectorAll("#why-pills .pill"));
  if (whys && whyPills.length) {
    var bad = document.getElementById("why-bad"), good = document.getElementById("why-good");
    var bar = document.getElementById("why-bar");
    function compare(i) {
      var item = whys.items[i];
      whyPills.forEach(function (pill, n) { pill.setAttribute("aria-pressed", n === i ? "true" : "false"); });
      document.getElementById("why-harm").textContent = item.harm;
      document.getElementById("why-chip").textContent = item.plain;
      document.getElementById("why-does").textContent = item.does;
      good.style.setProperty("--c", "var(--" + item.risk + ")");
      bad.classList.remove("jolt"); bar.classList.remove("on", "counting");
      if (still) { return; }
      void bad.offsetWidth;
      bad.classList.add("jolt");
      if (item.risk === "guarded") {
        bar.classList.add("on");
        bar.style.setProperty("--secs", "4s");
        requestAnimationFrame(function () { requestAnimationFrame(function () { bar.classList.add("counting"); }); });
      }
    }
    whyPills.forEach(function (pill, i) { pill.addEventListener("click", function () { compare(i); }); });
    compare(0);
  }

  // ---- the board replay
  var demo = data("demo-data");
  var board = document.querySelector(".board.replay");
  if (demo && board) {
    var feed = document.getElementById("feed");
    var slots = demo.slots.map(function (slot) {
      return { key: slot.key, pressable: slot.pressable, el: board.querySelector('[data-key="' + slot.key + '"]'), session: null };
    });
    var sessions = {};
    var timers = [];
    var urgent = function (state) { return state === "needs-you" || state === "failed"; };
    var shown = function (name) { return name && sessions[name] && sessions[name] !== "idle"; };

    function later(fn, secs) { timers.push(setTimeout(fn, secs * 1000)); }

    function say(text, kind) {
      if (!feed || !text) { return; }
      var li = document.createElement("li");
      var dot = document.createElement("span");
      dot.className = "dot " + (kind || "");
      li.appendChild(dot);
      li.appendChild(document.createTextNode(text));
      feed.insertBefore(li, feed.firstChild);
      while (feed.children.length > 4) { feed.removeChild(feed.lastChild); }
    }

    // The same rules the daemon uses: a session keeps its key, and one that
    // wants a response trades places onto a key that can give one.
    function assign() {
      slots.forEach(function (slot) { if (!shown(slot.session)) { slot.session = null; } });
      Object.keys(sessions).forEach(function (name) {
        if (!shown(name) || slots.some(function (s) { return s.session === name; })) { return; }
        var free = slots.filter(function (s) { return !s.session; })[0];
        if (free) { free.session = name; }
      });
      slots.forEach(function (slot) {
        if (slot.pressable || !urgent(sessions[slot.session])) { return; }
        var swap = slots.filter(function (s) { return s.pressable && !urgent(sessions[s.session]); })[0];
        if (swap) { var moved = swap.session; swap.session = slot.session; slot.session = moved; }
      });
    }

    function paint() {
      slots.forEach(function (slot) {
        if (!slot.el) { return; }
        var state = shown(slot.session) ? sessions[slot.session] : null;
        if (state) { slot.el.dataset.state = state; } else { delete slot.el.dataset.state; }
        var label = slot.el.querySelector("title");
        if (label) { label.textContent = state ? slot.session + ": " + state.replace("-", " ") : "empty slot"; }
      });
    }

    function set(name, state) {
      sessions[name] = state;
      assign();
      paint();
      if (state === "done") {
        later(function () { if (sessions[name] === "done") { set(name, "idle"); } }, 3.2);
      }
    }

    function press(slot, text) {
      if (!slot || !slot.pressable || !shown(slot.session)) { return false; }
      var name = slot.session, state = sessions[name];
      slot.el.classList.add("flash");
      setTimeout(function () { slot.el.classList.remove("flash"); }, 360);
      if (state === "needs-you") { set(name, "running"); } else if (state === "failed" || state === "done") { set(name, "idle"); }
      say(text, "press");
      return true;
    }

    function pressMostUrgent(text) {
      var order = ["needs-you", "failed", "done", "running"];
      var target = slots.filter(function (s) { return s.pressable && shown(s.session); }).sort(function (a, b) {
        return order.indexOf(sessions[a.session]) - order.indexOf(sessions[b.session]);
      })[0];
      press(target, text);
    }

    function run(step) {
      if (step.session) { set(step.session, step.state); say(step.say, step.state); }
      else if (step.press) { pressMostUrgent(step.say); }
      else if (step.level !== undefined) { board.style.setProperty("--level", step.level); say(step.say, ""); }
      else if (step.release) {
        Object.keys(sessions).forEach(function (name) { sessions[name] = "idle"; });
        assign(); paint(); board.style.setProperty("--level", 1); say(step.say, "");
      }
    }

    function reset() {
      timers.forEach(clearTimeout); timers = [];
      sessions = {};
      slots.forEach(function (slot) { slot.session = null; });
      board.style.setProperty("--level", 1);
      if (feed) { feed.innerHTML = ""; }
      paint();
    }

    function play() {
      reset();
      demo.steps.forEach(function (step) { later(function () { run(step); }, step.at); });
      later(play, demo.loop);
    }

    slots.forEach(function (slot) {
      if (!slot.el || !slot.pressable) { return; }
      function act(event) {
        if (event.type === "keydown" && event.key !== "Enter" && event.key !== " ") { return; }
        event.preventDefault();
        press(slot, demo.pressed);
      }
      slot.el.addEventListener("click", act);
      slot.el.addEventListener("keydown", act);
    });
    var replay = document.getElementById("replay");
    if (replay) { replay.addEventListener("click", play); }

    if (still) {
      // No autoplay: show the moment an agent needs an answer, and wait.
      demo.steps.some(function (step) { run(step); return step.state === "needs-you"; });
    } else {
      play();
    }
  }

  // ---- how a change travels
  var ladder = data("ladder-data");
  var travel = document.getElementById("travel");
  if (ladder && travel) {
    var tabs = Array.prototype.slice.call(travel.querySelectorAll(".tab"));
    var stops = Array.prototype.slice.call(travel.querySelectorAll(".stop"));
    var token = travel.querySelector(".token");
    var timer = travel.querySelector(".timer");
    var keep = travel.querySelector("#keep");
    var out = travel.querySelector("#outcome");
    var value = travel.querySelector("#value");
    var pending = [];

    function clear() { pending.forEach(clearTimeout); pending = []; }
    function at(ms, fn) { if (still) { fn(); } else { pending.push(setTimeout(fn, ms)); } }

    function reach(i, cls) {
      var stop = stops[i];
      stop.classList.add(cls || "on");
      token.classList.add("show");
      token.style.left = (stop.offsetLeft + stop.offsetWidth / 2 - 7) + "px";
    }

    function settle(text, shownValue) {
      out.textContent = text;
      value.innerHTML = shownValue ? "now <b>" + shownValue + "</b>" : "";
    }

    function show(i) {
      var item = ladder.items[i];
      clear();
      tabs.forEach(function (tab, n) { tab.setAttribute("aria-selected", n === i ? "true" : "false"); });
      travel.style.setProperty("--c", "var(--" + item.risk + ")");
      travel.querySelector("#ask").textContent = item.ask;
      travel.querySelector("#target").textContent = item.device + " · " + item.capability + " · " + item.risk;
      stops.forEach(function (stop) { stop.className = "stop"; });
      stops[2].querySelector("span").textContent = item.gate;
      stops[3].querySelector("span").textContent = item.reach;
      token.classList.remove("show");
      timer.classList.remove("show", "run");
      settle("", item.before);

      at(150, function () { reach(0); });
      at(850, function () { reach(1); });
      at(1550, function () { reach(2, item.risk === "gated" ? "blocked" : "on"); });
      at(2250, function () {
        if (item.risk === "gated") { stops[3].classList.add("skip"); settle(item.outcome, ""); return; }
        reach(3);
        if (item.risk !== "guarded") { settle(item.outcome, item.after); return; }
        settle(item.outcome, item.after);
        timer.classList.add("show");
        // Start the countdown on the next frame so the bar animates from full.
        requestAnimationFrame(function () { requestAnimationFrame(function () { timer.classList.add("run"); }); });
        pending.push(setTimeout(function () {
          timer.classList.remove("show", "run");
          settle(item.reverted, item.before);
        }, 5000));
      });
    }

    tabs.forEach(function (tab, i) { tab.addEventListener("click", function () { show(i); }); });
    if (keep) {
      keep.addEventListener("click", function () {
        var item = ladder.items[tabs.findIndex(function (t) { return t.getAttribute("aria-selected") === "true"; })];
        clear();
        timer.classList.remove("show", "run");
        settle(item.kept, item.after);
      });
    }
    var started = false;
    function start() { if (!started) { started = true; show(0); } }
    if ("IntersectionObserver" in window && !still) {
      new IntersectionObserver(function (entries, watcher) {
        if (entries[0].isIntersecting) { start(); watcher.disconnect(); }
      }, { threshold: 0.3 }).observe(travel);
    } else {
      start();
    }
  }
})();
