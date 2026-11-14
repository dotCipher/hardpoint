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

  function make(tag, cls, text) {
    var node = document.createElement(tag);
    if (cls) { node.className = cls; }
    if (text) { node.textContent = text; }
    return node;
  }

  function ghost(node) {
    node.classList.add("ghost");
    node.setAttribute("aria-hidden", "true");
    return node;
  }

  // A box whose words change must not change size. Lay out every wording it
  // can show in the same place, unseen, and return the one that is seen.
  function reserve(box, texts) {
    box.textContent = "";
    box.classList.add("stack");
    var had = {};
    texts.forEach(function (text) {
      if (text && !had[text]) { had[text] = true; box.appendChild(ghost(make("span", "", text))); }
    });
    return box.appendChild(make("span", "live"));
  }

  // ---- the catalog: search, facets and order. Within a facet the choices
  // widen the list; across facets they narrow it. Each choice shows how many
  // devices it would give, and the whole state lives in the address so a
  // filtered list can be linked to.
  var catalog = document.getElementById("catalog");
  if (catalog) {
    var q = document.getElementById("q"), order = document.getElementById("sort"), panel = document.getElementById("facets");
    var grid = document.getElementById("cards"), found = document.getElementById("found");
    var clear = document.getElementById("clear"), none = document.getElementById("none");
    var tiles = Array.prototype.slice.call(grid.children).map(function (node) {
      return { node: node, facets: JSON.parse(node.dataset.facets), text: node.dataset.search, name: node.dataset.name.toLowerCase(),
               abilities: Number(node.dataset.abilities), rank: Number(node.dataset.rank) };
    });
    var groups = Array.prototype.slice.call(panel.querySelectorAll(".facet")).map(function (box) {
      return { key: box.dataset.facet, inputs: Array.prototype.slice.call(box.querySelectorAll("input")) };
    });
    var orders = {
      checked: function (a, b) { return a.rank - b.rank; },
      name: function (a, b) { return a.name.localeCompare(b.name); },
      abilities: function (a, b) { return b.abilities - a.abilities || a.rank - b.rank; }
    };

    var fits = function (tile, terms, picks, except) {
      if (!terms.every(function (term) { return tile.text.indexOf(term) >= 0; })) { return false; }
      return groups.every(function (group) {
        var want = picks[group.key];
        return group.key === except || !want.length || want.some(function (value) { return tile.facets[group.key].indexOf(value) >= 0; });
      });
    };

    var apply = function () {
      var terms = q.value.toLowerCase().split(/\s+/).filter(Boolean);
      var picks = {}, address = new URLSearchParams(), shown = 0;
      groups.forEach(function (group) {
        picks[group.key] = group.inputs.filter(function (input) { return input.checked; }).map(function (input) { return input.value; });
        picks[group.key].forEach(function (value) { address.append(group.key, value); });
      });
      tiles.slice().sort(orders[order.value] || orders.checked).forEach(function (tile) {
        var on = fits(tile, terms, picks);
        tile.node.hidden = !on;
        if (on) { shown += 1; }
        grid.appendChild(tile.node);
      });
      groups.forEach(function (group) {
        group.inputs.forEach(function (input) {
          var count = tiles.filter(function (tile) {
            return tile.facets[group.key].indexOf(input.value) >= 0 && fits(tile, terms, picks, group.key);
          }).length;
          var empty = !count && !input.checked;
          input.parentNode.querySelector("b").textContent = count;
          input.disabled = empty;
          input.parentNode.classList.toggle("off", empty);
        });
      });
      found.textContent = catalog.dataset.found.replace("{n}", shown).replace("{total}", tiles.length);
      clear.hidden = !terms.length && !address.toString();
      none.hidden = shown > 0;
      if (terms.length) { address.set("q", q.value.trim()); }
      if (order.selectedIndex > 0) { address.set("sort", order.value); }
      var query = address.toString();
      history.replaceState(null, "", location.pathname + (query ? "?" + query : ""));
    };

    var asked = new URLSearchParams(location.search);
    q.value = asked.get("q") || "";
    if (orders[asked.get("sort")]) { order.value = asked.get("sort"); }
    groups.forEach(function (group) {
      var want = asked.getAll(group.key);
      group.inputs.forEach(function (input) { input.checked = want.indexOf(input.value) >= 0; });
    });
    q.addEventListener("input", apply);
    order.addEventListener("change", apply);
    panel.addEventListener("change", apply);
    clear.addEventListener("click", function () {
      q.value = "";
      groups.forEach(function (group) { group.inputs.forEach(function (input) { input.checked = false; }); });
      apply();
    });
    // Beside the list on a wide screen; folded away above it on a narrow one.
    var wide = window.matchMedia("(min-width: 900px)");
    var place = function () { panel.open = wide.matches || groups.some(function (group) { return group.inputs.some(function (input) { return input.checked; }); }); };
    if (wide.addEventListener) { wide.addEventListener("change", place); }
    panel.hidden = false;
    place();
    apply();
  }

  // ---- copy the prompt
  var copy = document.getElementById("copy");
  var prompt = document.getElementById("prompt");
  if (copy && prompt) {
    var copyLabel = copy.textContent;
    var copyText = reserve(copy, [copyLabel, copy.dataset.done]);
    copyText.textContent = copyLabel;
    var copied = function () {
      copyText.textContent = copy.dataset.done;
      setTimeout(function () { copyText.textContent = copyLabel; }, 1600);
    };
    var bySelecting = function () {
      var range = document.createRange();
      range.selectNodeContents(prompt);
      var selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      try { if (document.execCommand("copy")) { copied(); } } catch (error) { /* the text stays selected to copy by hand */ }
    };
    copy.addEventListener("click", function () {
      if (navigator.clipboard && navigator.clipboard.writeText) {
        navigator.clipboard.writeText(prompt.textContent).then(copied, bySelecting);
      } else {
        bySelecting();
      }
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
    var el = function (id) { return document.getElementById(id); };
    var pillBox = el("scene-pills");
    var pills = Array.prototype.slice.call(pillBox.querySelectorAll(".pill"));
    var pictures = Array.prototype.slice.call(stage.querySelectorAll(".drawing-wrap"));
    var askEl = el("scene-ask"), verdict = el("scene-verdict"), chipEl = el("scene-chip");
    var readout = el("scene-readout"), confirm = el("scene-confirm"), statusEl = el("scene-status");
    var dotsEl = el("scene-dots"), toggle = el("scene-toggle");
    var words = scenes.words;
    var di = 0, ci = 0, playing = !still, queue = [], decide = null;

    // One line showing a value: a meter, a colour or a pulse, then the words.
    function row(cap, from, value, note) {
      var node = make("span", "row");
      if (cap.risk === "gated") { node.appendChild(make("span", "why", cap.harm)); return node; }
      if (cap.show === "meter") {
        var fill = make("i");
        fill.style.width = (parseInt(from || "0", 10) || 0) + "%";
        fill.dataset.to = (parseInt(value, 10) || 0) + "%";
        node.appendChild(make("span", "bar2")).appendChild(fill);
      } else if (cap.show === "swatch") {
        node.appendChild(make("span", "dotc")).style.background = value === cap.after ? cap.swatch : "transparent";
      } else if (cap.show === "pulse") {
        var ticks = node.appendChild(make("span", "ticks3"));
        for (var n = 0; n < 3; n++) { ticks.appendChild(make("i")); }
      }
      node.appendChild(make("span", "", !note && from && from !== value ? from + " → " + value : value));
      if (note) { node.appendChild(make("span", "was", note)); }
      return node;
    }

    var caps = [];
    scenes.devices.forEach(function (device) { caps = caps.concat(device.caps); });
    var askLive = reserve(askEl, caps.map(function (cap) { return cap.ask; }));
    var doesLive = reserve(el("scene-does"), caps.map(function (cap) { return cap.does; }));
    readout.classList.add("stack");
    var had = {};
    caps.forEach(function (cap) {
      var key = [cap.risk, cap.show, cap.before, cap.after, cap.harm].join("|");
      if (had[key]) { return; }
      had[key] = true;
      readout.appendChild(ghost(row(cap, cap.before, cap.after, "")));
      if (cap.risk === "guarded") {
        readout.appendChild(ghost(row(cap, cap.after, cap.after, words.kept)));
        readout.appendChild(ghost(row(cap, cap.after, cap.before, words.undone)));
      }
    });
    var valueLive = readout.appendChild(make("span", "live"));

    function wait(ms, fn) { queue.push(setTimeout(fn, still ? 0 : ms)); }
    function stopAll() { queue.forEach(clearTimeout); queue = []; decide = null; }
    function lampOf() { return pictures[di].querySelector(".lamp"); }

    function showValue(cap, from, value, note) {
      var node = row(cap, from, value, note);
      valueLive.textContent = "";
      valueLive.appendChild(node);
      var fill = node.querySelector(".bar2 i");
      if (fill) { requestAnimationFrame(function () { requestAnimationFrame(function () { fill.style.width = fill.dataset.to; }); }); }
      var lamp = lampOf();
      if (lamp && cap.risk !== "gated") { lamp.style.fill = cap.show === "swatch" && value === cap.after ? cap.swatch : "var(--c)"; }
      readout.classList.add("show");
    }

    function finish(hold) { if (playing) { wait(hold, next); } }

    function settle(cap, kept) {
      decide = null;
      confirm.classList.remove("on", "show", "counting");
      showValue(cap, cap.after, kept ? cap.after : cap.before, kept ? words.kept : words.undone);
      finish(2200);
    }

    function result(cap) {
      showValue(cap, cap.before, cap.after, "");
      if (cap.risk === "gated") { return finish(3600); }
      if (cap.risk !== "guarded") { return finish(2600); }
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
      var pill = pills[di];
      pillBox.scrollTo({ left: Math.max(0, pill.offsetLeft - (pillBox.clientWidth - pill.offsetWidth) / 2), behavior: still ? "auto" : "smooth" });
      pictures.forEach(function (pic, n) { pic.hidden = n !== di; pic.classList.remove("asking"); });
      var lamp = lampOf(); if (lamp) { lamp.style.fill = ""; }
      stage.style.setProperty("--c", "var(--" + cap.risk + ")");
      dotsEl.textContent = "";
      device.caps.forEach(function (_, n) { dotsEl.appendChild(make("i", n === ci ? "on" : "")); });
      [verdict, readout, statusEl].forEach(function (node) { node.classList.remove("show"); });
      confirm.classList.remove("on", "show", "counting");
      askEl.classList.remove("done");
      askLive.textContent = "";
      var typed = 0;
      (function type() {
        if (typed < cap.ask.length && !still) {
          askLive.textContent = cap.ask.slice(0, ++typed);
          queue.push(setTimeout(type, 20));
          return;
        }
        askLive.textContent = cap.ask;
        askEl.classList.add("done");
        wait(350, function () {
          chipEl.textContent = cap.risk;
          doesLive.textContent = cap.does;
          statusEl.textContent = cap.status;
          statusEl.className = "check show " + cap.status;
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
      toggle.textContent = playing ? words.pause : words.play;
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
    var good = document.getElementById("why-good");
    var bar = document.getElementById("why-bar");
    var whyChip = document.getElementById("why-chip");
    var harmLive = reserve(document.getElementById("why-harm"), whys.items.map(function (item) { return item.harm; }));
    var whyDoes = reserve(document.getElementById("why-does"), whys.items.map(function (item) { return item.does; }));
    var compare = function (i) {
      var item = whys.items[i];
      whyPills.forEach(function (pill, n) { pill.setAttribute("aria-pressed", n === i ? "true" : "false"); });
      harmLive.textContent = item.harm;
      whyChip.textContent = item.risk;
      whyDoes.textContent = item.does;
      good.style.setProperty("--c", "var(--" + item.risk + ")");
      bar.classList.remove("on", "counting");
      if (still || item.risk !== "guarded") { return; }
      bar.classList.add("on");
      bar.style.setProperty("--secs", "4s");
      requestAnimationFrame(function () { requestAnimationFrame(function () { bar.classList.add("counting"); }); });
    };
    whyPills.forEach(function (pill, i) { pill.addEventListener("click", function () { compare(i); }); });
    compare(0);
  }

  // ---- the board replay
  var demo = data("demo-data");
  var board = document.querySelector(".board.replay");
  if (demo && board) {
    var feed = document.getElementById("feed");
    var line = feed ? reserve(feed, demo.steps.map(function (step) { return step.say; }).concat([demo.pressed])) : null;
    var slots = demo.slots.map(function (slot) {
      return { key: slot.key, pressable: slot.pressable, el: board.querySelector('[data-key="' + slot.key + '"]'), session: null };
    });
    var sessions = {};
    var timers = [];
    var urgent = function (state) { return state === "needs-you" || state === "failed"; };
    var shown = function (name) { return name && sessions[name] && sessions[name] !== "idle"; };

    function later(fn, secs) { timers.push(setTimeout(fn, secs * 1000)); }
    function say(text) { if (line && text) { line.textContent = text; } }

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
      say(text);
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
      if (step.session) { set(step.session, step.state); say(step.say); }
      else if (step.press) { pressMostUrgent(step.say); }
      else if (step.release) {
        Object.keys(sessions).forEach(function (name) { sessions[name] = "idle"; });
        assign(); paint(); say(step.say);
      }
    }

    function reset() {
      timers.forEach(clearTimeout); timers = [];
      sessions = {};
      slots.forEach(function (slot) { slot.session = null; });
      if (line) { line.textContent = ""; }
      paint();
    }

    function replayAll() {
      reset();
      demo.steps.forEach(function (step) { later(function () { run(step); }, step.at); });
      later(replayAll, demo.loop);
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
    if (replay) { replay.addEventListener("click", replayAll); }

    if (still) {
      // No autoplay: show the moment a helper needs an answer, and wait.
      demo.steps.some(function (step) { run(step); return step.state === "needs-you"; });
    } else {
      replayAll();
    }
  }

  // ---- how a change travels
  var ladder = data("ladder-data");
  var travel = document.getElementById("travel");
  if (ladder && travel) {
    var items = ladder.items;
    var pick = function (field) { return items.map(function (item) { return item[field]; }); };
    var tabs = Array.prototype.slice.call(travel.querySelectorAll(".tab"));
    var stops = Array.prototype.slice.call(travel.querySelectorAll(".stop"));
    var token = travel.querySelector(".token");
    var timer = travel.querySelector(".timer");
    var keep = travel.querySelector("#keep");
    var value = travel.querySelector("#value");
    var target = function (item) { return item.device + " · " + item.capability; };
    var askLine = reserve(travel.querySelector("#ask"), pick("ask"));
    var targetLine = reserve(travel.querySelector("#target"), items.map(target));
    var gateLine = reserve(stops[2].querySelector("span"), pick("gate"));
    var reachLine = reserve(stops[3].querySelector("span"), pick("reach"));
    var out = reserve(travel.querySelector("#outcome"), pick("outcome").concat(pick("kept"), pick("reverted")));
    var pending = [];

    function clear() { pending.forEach(clearTimeout); pending = []; }
    function at(ms, fn) { if (still) { fn(); } else { pending.push(setTimeout(fn, ms)); } }

    function reach(i, cls) {
      var stop = stops[i];
      stop.classList.add(cls || "on");
      token.classList.add("show");
      token.style.transform = "translateX(" + (stop.offsetLeft + stop.offsetWidth / 2 - 7) + "px)";
    }

    function settle(text, shownValue) {
      out.textContent = text;
      value.textContent = "";
      if (shownValue) { value.appendChild(document.createTextNode("now ")); value.appendChild(make("b", "", shownValue)); }
    }

    function show(i) {
      var item = items[i];
      clear();
      tabs.forEach(function (tab, n) { tab.setAttribute("aria-selected", n === i ? "true" : "false"); });
      travel.style.setProperty("--c", "var(--" + item.risk + ")");
      askLine.textContent = item.ask;
      targetLine.textContent = target(item);
      stops.forEach(function (stop) { stop.className = "stop"; });
      gateLine.textContent = item.gate;
      reachLine.textContent = item.reach;
      token.classList.remove("show");
      timer.classList.remove("show", "run");
      settle("", item.before);

      at(150, function () { reach(0); });
      at(850, function () { reach(1); });
      at(1550, function () { reach(2, item.risk === "gated" ? "blocked" : "on"); });
      at(2250, function () {
        if (item.risk === "gated") { stops[3].classList.add("skip"); settle(item.outcome, ""); return; }
        reach(3);
        settle(item.outcome, item.after);
        if (item.risk !== "guarded") { return; }
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
        var item = items[tabs.findIndex(function (t) { return t.getAttribute("aria-selected") === "true"; })];
        clear();
        timer.classList.remove("show", "run");
        settle(item.kept, item.after);
      });
    }
    var started = false;
    var start = function () { if (!started) { started = true; show(0); } };
    if ("IntersectionObserver" in window && !still) {
      new IntersectionObserver(function (entries, watcher) {
        if (entries[0].isIntersecting) { start(); watcher.disconnect(); }
      }, { threshold: 0.3 }).observe(travel);
    } else {
      start();
    }
  }
})();
