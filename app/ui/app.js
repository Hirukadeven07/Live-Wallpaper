// The Live Wallpaper window. It talks to the app with chrome.webview.postMessage and gets JSON messages back:
//   state    the wallpapers, the screens and which wallpaper is on each, and the app's settings
//   props    one wallpaper's settings (from its LivelyProperties.json) and their current values
//   toast    a short message; busy  "adding files..." on or off; imported  a wallpaper that was just added
(() => {
  'use strict';
  const host = window.chrome && window.chrome.webview;
  const send = msg => host && host.postMessage(msg);
  const $ = id => document.getElementById(id);
  const el = (tag, attrs, ...kids) => {
    const e = document.createElement(tag);
    for (const [k, v] of Object.entries(attrs || {})) {
      if (v == null || v === false) continue;
      if (k === 'class') e.className = v;
      else if (k === 'text') e.textContent = v;
      else if (k.startsWith('on')) e.addEventListener(k.slice(2), v);
      else e.setAttribute(k, v === true ? '' : v);
    }
    for (const kid of kids.flat()) if (kid != null && kid !== false) e.append(kid.nodeType ? kid : document.createTextNode(kid));
    return e;
  };
  const icon = name => {
    const s = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    const u = document.createElementNS('http://www.w3.org/2000/svg', 'use');
    u.setAttribute('href', '#i-' + name);
    s.append(u);
    s.setAttribute('aria-hidden', 'true');
    return s;
  };

  const S = {
    state: null,
    selected: null,
    target: 'all',
    props: null,       // the selected wallpaper's settings, once they arrive
  };
  try { S.target = localStorage.getItem('target') || 'all'; } catch (e) {}

  // ---------- helpers ----------
  const wallpapers = () => (S.state ? S.state.wallpapers : []);
  const displays = () => (S.state ? S.state.displays : []);
  const byId = id => wallpapers().find(w => w.id === id);
  const multi = () => displays().length > 1;
  const screensOf = id => displays().filter(d => d.wallpaper === id);
  const targetName = () => {
    if (S.target === 'all') return multi() ? 'all screens' : 'your desktop';
    const d = displays().find(d => d.id === S.target);
    return d ? d.name : 'your desktop';
  };
  /** Is this wallpaper already on every screen the current choice covers? */
  const onTarget = id => {
    if (S.target === 'all') return displays().length > 0 && displays().every(d => d.wallpaper === id);
    const d = displays().find(d => d.id === S.target);
    return !!d && d.wallpaper === id;
  };
  const thumbOf = w => {
    if (w.thumb) return el('img', { src: w.thumb, alt: '', loading: 'lazy', draggable: 'false' });
    if (w.preview) {
      const v = el('video', { src: w.preview + '#t=0.5', muted: true, loop: true, playsinline: true, preload: 'metadata' });
      v.muted = true;
      return v;
    }
    return el('div', { class: 'placeholder', text: (w.title || '?').trim().charAt(0).toUpperCase() });
  };

  // ---------- header ----------
  function renderHeader() {
    const box = $('screens');
    box.replaceChildren();
    if (!multi()) {
      box.hidden = true;
      S.target = 'all';
    } else {
      box.hidden = false;
      if (S.target !== 'all' && !displays().some(d => d.id === S.target)) S.target = 'all';
      const opt = (id, label) => el('button', {
        type: 'button', role: 'radio', 'aria-checked': String(S.target === id),
        onclick: () => { S.target = id; try { localStorage.setItem('target', id); } catch (e) {} renderAll(); }
      }, label);
      box.append(opt('all', 'All screens'), ...displays().map(d => opt(d.id, d.name)));
    }
    const paused = S.state.settings.paused;
    const p = $('pause');
    p.replaceChildren(icon(paused ? 'play' : 'pause'), el('span', { text: paused ? 'Resume' : 'Pause' }));
    p.title = paused ? 'Start the wallpapers moving again' : 'Freeze the wallpapers where they are';
    $('paused-banner').hidden = !paused;
  }

  // ---------- the wallpaper grids ----------
  function card(w) {
    const where = screensOf(w.id);
    const thumb = el('div', { class: 'thumb' }, thumbOf(w));
    if (where.length) {
      const label = !multi() ? 'On' : where.length === displays().length ? 'On all screens' : 'On ' + where.map(d => d.name).join(', ');
      thumb.append(el('span', { class: 'on-pill' }, icon('check'), label));
    }
    const sub = w.kind === 'video' ? 'Video' : w.kind === 'picture' ? 'Picture' : w.website ? 'Website' : (w.author || (w.builtin ? 'Built in' : 'Web wallpaper'));
    const c = el('button', {
      class: 'card', type: 'button', 'aria-current': String(S.selected === w.id), title: w.desc || w.title,
      onclick: () => choose(w.id),
      onmouseenter: () => { const v = c.querySelector('video'); if (v) v.play().catch(() => {}); },
      onmouseleave: () => { const v = c.querySelector('video'); if (v) v.pause(); },
    }, thumb, el('p', { class: 'name', text: w.title }), el('p', { class: 'sub', text: sub }));
    return c;
  }
  function renderGrids() {
    const b = $('grid-builtin'), u = $('grid-user');
    b.replaceChildren(...wallpapers().filter(w => w.builtin).map(card));
    const add = el('button', { class: 'card add-card', type: 'button', onclick: () => send({ type: 'import', target: S.target }) },
      el('div', { class: 'thumb' }, el('div', {}, icon('plus'), 'Add wallpaper')),
      el('p', { class: 'name', text: 'Add your own' }),
      el('p', { class: 'sub', text: '.zip, web page, picture or video' }));
    u.replaceChildren(...wallpapers().filter(w => !w.builtin).map(card), add);
  }

  /** Clicking a wallpaper: put it on the chosen screen(s), unless it's already showing somewhere. */
  function choose(id) {
    const w = byId(id);
    if (!w) return;
    const showing = screensOf(id).length > 0;
    select(id);
    if (!showing) send({ type: 'apply', id, target: S.target });
  }
  function select(id) {
    if (S.selected !== id) { S.selected = id; S.props = null; }
    send({ type: 'getProps', id });
    renderGrids();
    renderDetail();
  }

  // ---------- the details panel ----------
  function renderDetail() {
    const box = $('detail');
    const w = S.selected && byId(S.selected);
    if (!w) { box.replaceChildren(welcome()); return; }
    const where = screensOf(w.id);
    const hero = el('div', { class: 'hero' }, thumbOf(w));
    const hv = hero.querySelector('video');
    if (hv) { hv.autoplay = true; hv.play().catch(() => {}); }

    const chips = el('div', { class: 'where' });
    if (where.length) {
      if (!multi()) chips.append(el('span', { class: 'chip' }, icon('check'), 'On your desktop'));
      else for (const d of where) chips.append(el('span', { class: 'chip' }, icon('check'), 'On ' + d.name));
    } else chips.append(el('span', { class: 'chip off', text: 'Not on your desktop' }));

    const actions = el('div', { class: 'actions' });
    if (!onTarget(w.id)) {
      actions.append(el('button', { class: 'btn primary', type: 'button', onclick: () => send({ type: 'apply', id: w.id, target: S.target }) },
        icon('check'), multi() ? 'Use on ' + targetName() : 'Use as wallpaper'));
    }
    if (where.length) {
      actions.append(el('button', {
        class: 'btn', type: 'button', title: 'Hold to hide this window and look at the desktop',
        onpointerdown: e => { if (e.button === 0) send({ type: 'peek' }); },
      }, icon('eye'), 'Peek'));
      const offTarget = S.target === 'all' ? 'all' : (where.some(d => d.id === S.target) ? S.target : where[0].id);
      actions.append(el('button', { class: 'btn ghost', type: 'button', onclick: () => send({ type: 'off', target: offTarget }) },
        icon('off'), 'Turn off'));
    }
    const more = el('button', { class: 'btn icon', type: 'button', 'aria-label': 'More', title: 'More',
      onclick: e => openMenu(e.currentTarget, moreItems(w)) }, icon('more'));
    actions.append(more);

    const notes = el('div', { class: 'notes' });
    const note = (ic, html) => { const n = el('div', { class: 'note' }, icon(ic)); const t = el('div'); t.innerHTML = html; n.append(t); notes.append(n); };
    if (w.helper === 'eyes') note('grid', '<b>Sees your desktop icons by itself.</b> No Clawd\'s Eyes needed. For the shelf to line up, right-click the desktop, open <b>View</b>, turn <b>Auto arrange icons</b> off and <b>Align icons to grid</b> on.');
    if (w.helper === 'ears') note('music', '<b>Listens to Spotify by itself.</b> No Clawd\'s Ears needed. Play something in the Spotify app and the player fills in.');
    if (w.usesMouse && S.state.settings.mouseInput) note('mouse', 'Reacts to your mouse while it\'s over the desktop. That works by itself here.');
    if (w.usesMouse && !S.state.settings.mouseInput) note('mouse', 'This one plays with the mouse, but <b>Let wallpapers react to the mouse</b> is off in the app settings.');
    if (w.audio) note('wave', 'Moves to the sound your PC is playing.');
    if (w.website) note('globe', 'This is a website, so it needs the internet.');

    const head = el('div', { class: 'settings-head' }, el('h3', { text: 'Settings' }));
    const form = el('div', { class: 'form' });
    if (S.props && S.props.id === w.id) {
      if (S.props.items.length) {
        head.append(el('button', { class: 'btn small ghost', type: 'button', onclick: () => send({ type: 'resetProps', id: w.id }) }, 'Reset'));
        for (const item of S.props.items) form.append(control(w.id, item));
      } else form.append(el('p', { class: 'empty-settings', text: 'This wallpaper has no settings.' }));
    }
    const children = [hero, el('h1', { text: w.title }), chips];
    if (w.desc) children.push(el('p', { class: 'desc', text: w.desc }));
    children.push(actions);
    if (notes.childElementCount) children.push(notes);
    children.push(head, form);
    box.replaceChildren(...children);
  }

  function welcome() {
    return el('div', { class: 'welcome' },
      el('h1', { text: 'Pick a wallpaper' }),
      el('p', { text: 'Click any wallpaper on the left and it goes straight onto your desktop. Its settings show up here, and every change shows on the desktop as you make it.' }),
      el('ol', {},
        el('li', { text: 'Wallpapers pause by themselves while an app covers the screen, so they don\'t waste power.' }),
        el('li', { text: 'Add your own: drop a .zip, a web page, a picture or a video into this window.' }),
        el('li', { text: 'Close this window whenever you like. Live Wallpaper keeps running in the tray, by the clock.' })));
  }

  function moreItems(w) {
    const dev = S.state.settings.developer;
    const items = [{ label: 'Open its folder', run: () => send({ type: 'openFolder', id: w.id }) }];
    if (dev || !w.builtin) items.push({ label: 'Reload', run: () => send({ type: 'reload', id: w.id }) });
    if (dev) items.push({ label: 'Developer tools', run: () => send({ type: 'devtools', id: w.id }) });
    if (!w.builtin) {
      items.push('-');
      items.push({ label: 'Remove…', danger: true, run: () => {
        if (confirm(`Remove "${w.title}"? Its folder is deleted.`)) send({ type: 'delete', id: w.id });
      } });
    }
    return items;
  }

  // ---------- settings controls (Lively's LivelyProperties.json types) ----------
  const sendLater = (() => {
    const timers = {};
    return (key, fn, ms) => { clearTimeout(timers[key]); timers[key] = setTimeout(fn, ms); };
  })();
  function setValue(id, key, value) {
    if (S.props && S.props.id === id) S.props.values[key] = value;
    send({ type: 'setProp', id, key, value });
  }

  function control(id, item) {
    const key = item.key, text = item.text || key;
    const value = S.props.values[key];
    const domId = 'p-' + key.replace(/[^\w-]/g, '_');
    switch (item.type) {
      case 'checkbox': {
        const input = el('input', { type: 'checkbox', id: domId, role: 'switch' });
        input.checked = !!value;
        input.addEventListener('change', () => setValue(id, key, input.checked));
        return el('div', { class: 'row inline' }, el('label', { class: 'label-text', for: domId, text }),
          el('span', { class: 'switch' }, input, el('span')));
      }
      case 'slider': {
        const min = +item.min || 0, max = item.max != null ? +item.max : 100, step = +item.step || 1;
        const input = el('input', { type: 'range', id: domId, min, max, step });
        input.value = value;
        // Optional extras Lively ignores: "unit" after the number, "ends" as words under the two ends.
        const unit = typeof item.unit === 'string' ? item.unit : '';
        const out = el('span', { class: 'val', text: value + unit });
        let last = 0;
        input.addEventListener('input', () => {
          out.textContent = input.value + unit;
          // Live while dragging, but not on every pixel.
          const now = performance.now();
          if (now - last > 60) { last = now; setValue(id, key, +input.value); }
          else sendLater(key, () => setValue(id, key, +input.value), 70);
        });
        input.addEventListener('change', () => setValue(id, key, +input.value));
        const ends = Array.isArray(item.ends) && item.ends.length === 2
          ? el('div', { class: 'ends' }, el('span', { text: String(item.ends[0]) }), el('span', { text: String(item.ends[1]) }))
          : null;
        return el('div', { class: 'row' }, el('div', { class: 'label-line' }, el('label', { for: domId, text }), out), input, ends);
      }
      case 'dropdown': {
        const items = item.items || [];
        const short = items.length > 1 && items.length <= 4 && items.every(s => String(s).length <= 14);
        if (short) {
          const seg = el('div', { class: 'seg', role: 'group', 'aria-label': text });
          items.forEach((label, i) => seg.append(el('button', {
            type: 'button', 'aria-pressed': String(value === i), text: label,
            onclick: () => { setValue(id, key, i); for (const b of seg.children) b.setAttribute('aria-pressed', String(b === seg.children[i])); },
          })));
          return el('div', { class: 'row' }, el('span', { class: 'label', text }), seg);
        }
        const sel = el('select', { id: domId });
        items.forEach((label, i) => sel.append(el('option', { value: i, text: label })));
        sel.value = String(value);
        sel.addEventListener('change', () => setValue(id, key, +sel.value));
        return el('div', { class: 'row' }, el('label', { for: domId, text }), sel);
      }
      case 'color': {
        const input = el('input', { type: 'color', id: domId });
        input.value = /^#[0-9a-f]{6}$/i.test(value) ? value : '#000000';
        const hex = el('span', { class: 'val', text: input.value.toUpperCase() });
        input.addEventListener('input', () => { hex.textContent = input.value.toUpperCase(); sendLater(key, () => setValue(id, key, input.value), 50); });
        return el('div', { class: 'row inline' }, el('label', { class: 'label-text', for: domId, text }), el('span', { class: 'color' }, hex, input));
      }
      case 'textbox': {
        const input = el('input', { type: 'text', id: domId, spellcheck: 'false' });
        input.value = value == null ? '' : value;
        input.addEventListener('input', () => sendLater(key, () => setValue(id, key, input.value), 350));
        return el('div', { class: 'row' }, el('label', { for: domId, text }), input);
      }
      case 'button':
        return el('div', { class: 'row inline' }, el('span', { class: 'label-text', text }),
          el('button', { class: 'btn small', type: 'button', onclick: () => send({ type: 'button', id, key }) }, item.value || 'Go'));
      case 'folderDropdown': {
        const files = (S.props.folders && S.props.folders[key]) || [];
        const pics = el('div', { class: 'pics' });
        for (const f of files) {
          pics.append(el('button', {
            class: 'pic', type: 'button', title: f.name, 'aria-pressed': String(f.name === value),
            onclick: () => { setValue(id, key, f.name); for (const b of pics.querySelectorAll('.pic:not(.add)')) b.setAttribute('aria-pressed', String(b.title === f.name)); },
          }, el('img', { src: f.url, alt: f.name, loading: 'lazy', draggable: 'false' })));
        }
        pics.append(el('button', { class: 'pic add', type: 'button', title: 'Add a picture', 'aria-label': 'Add a picture',
          onclick: () => send({ type: 'addPicture', id, key }) }, icon('plus')));
        return el('div', { class: 'row' }, el('span', { class: 'label', text }), pics);
      }
      case 'label':
        return el('div', { class: 'row text-only', text: item.value || text });
      default:
        return el('div', { class: 'row text-only', text: `${text} (a "${item.type}" setting, which this app doesn't show)` });
    }
  }

  // ---------- menu ----------
  function openMenu(anchor, items) {
    const m = $('menu');
    m.replaceChildren(...items.map(it => it === '-' ? el('hr') :
      el('button', { type: 'button', role: 'menuitem', class: it.danger ? 'danger' : null, onclick: () => { closeMenu(); it.run(); } }, it.label)));
    m.hidden = false;
    const r = anchor.getBoundingClientRect();
    const mw = m.offsetWidth, mh = m.offsetHeight;
    m.style.left = Math.max(8, Math.min(innerWidth - mw - 8, r.right - mw)) + 'px';
    m.style.top = (r.bottom + mh + 8 > innerHeight ? r.top - mh - 6 : r.bottom + 6) + 'px';
    const first = m.querySelector('button');
    if (first) first.focus();
  }
  function closeMenu() { $('menu').hidden = true; }
  document.addEventListener('pointerdown', e => { if (!$('menu').hidden && !$('menu').contains(e.target)) closeMenu(); });
  document.addEventListener('keydown', e => { if (e.key === 'Escape') closeMenu(); });

  // ---------- app settings ----------
  function renderPrefs() {
    const st = S.state.settings;
    const toggle = (key, label, help) => {
      const input = el('input', { type: 'checkbox', id: 'g-' + key, role: 'switch' });
      input.checked = !!st[key];
      input.addEventListener('change', () => send({ type: 'setSetting', key, value: input.checked }));
      return el('div', { class: 'row inline' },
        el('div', { class: 'label-text' }, el('label', { for: 'g-' + key, text: label }), help ? el('div', { class: 'help', text: help }) : null),
        el('span', { class: 'switch' }, input, el('span')));
    };
    const body = $('prefs-body');
    const groups = [
      el('div', { class: 'group' }, el('h3', { text: 'General' }),
        toggle('autostart', 'Start with Windows', 'Your wallpaper comes back after a restart.'),
        toggle('mouseInput', 'Let wallpapers react to the mouse', 'Clicks and movement over the bare desktop reach the wallpaper.')),
      el('div', { class: 'group' }, el('h3', { text: 'Saving power' }),
        toggle('pauseWhenCovered', 'Pause when an app covers the screen', 'While a maximised or full-screen window hides the desktop, the wallpaper stops, and after a few seconds it also gives memory back.'),
        toggle('pauseOnBattery', 'Pause on battery power', 'On a laptop, wallpapers stand still while it isn\'t plugged in.')),
      el('div', { class: 'group' }, el('h3', { text: 'Making your own' }),
        toggle('developer', 'Developer mode', 'Adds Reload and Developer tools to each wallpaper\'s menu.'),
        el('div', { class: 'buttons' },
          el('button', { class: 'btn small', type: 'button', onclick: () => send({ type: 'openFolder', id: 'user' }) }, 'Open my wallpapers folder'),
          el('button', { class: 'btn small', type: 'button', onclick: () => { $('prefs').close(); send({ type: 'newWallpaper', target: S.target }); } }, 'Start a new wallpaper'),
          el('button', { class: 'btn small', type: 'button', onclick: () => send({ type: 'import', target: S.target, folder: true }) }, 'Add a folder…'))),
      el('div', { class: 'group' }, el('h3', { text: 'App' }),
        el('div', { class: 'buttons' },
          el('button', { class: 'btn small', type: 'button', onclick: () => send({ type: 'openFolder', id: 'data' }) }, 'Open app data folder'),
          el('button', { class: 'btn small danger', type: 'button', onclick: () => send({ type: 'quit' }) }, 'Quit Live Wallpaper'))),
    ];
    const about = el('div', { class: 'about' });
    about.innerHTML = `Live Wallpaper ${S.state.version}. An unofficial fan project, not made or endorsed by Anthropic. ` +
      `<a href="#" id="repo-link">Project page on GitHub</a>.`;
    body.replaceChildren(...groups, about);
    $('repo-link').addEventListener('click', e => { e.preventDefault(); send({ type: 'openLink', url: 'https://github.com/Hirukadeven07/Live-Wallpaper' }); });
  }

  // ---------- toasts and busy ----------
  function toast(text, kind) {
    const t = el('div', { class: 'toast ' + (kind || ''), text });
    $('toasts').append(t);
    setTimeout(() => t.remove(), kind === 'error' ? 9000 : 4000);
  }

  // ---------- drag and drop ----------
  let dragDepth = 0;
  const hasFiles = e => e.dataTransfer && Array.from(e.dataTransfer.types || []).includes('Files');
  addEventListener('dragenter', e => { if (!hasFiles(e)) return; e.preventDefault(); dragDepth++; $('drop').hidden = false; });
  addEventListener('dragleave', e => { if (!hasFiles(e)) return; dragDepth = Math.max(0, dragDepth - 1); if (!dragDepth) $('drop').hidden = true; });
  addEventListener('dragover', e => { e.preventDefault(); if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy'; });
  addEventListener('drop', e => {
    e.preventDefault();
    dragDepth = 0;
    $('drop').hidden = true;
    const files = e.dataTransfer ? Array.from(e.dataTransfer.files || []) : [];
    if (!files.length) return;
    if (host && host.postMessageWithAdditionalObjects) host.postMessageWithAdditionalObjects({ type: 'drop', target: S.target }, files);
    else toast('Dropping files needs a newer WebView2. Use Add wallpaper instead.', 'error');
  });

  // ---------- wiring ----------
  $('add').addEventListener('click', () => send({ type: 'import', target: S.target }));
  $('pause').addEventListener('click', () => send({ type: 'setSetting', key: 'paused', value: !S.state.settings.paused }));
  $('resume').addEventListener('click', () => send({ type: 'setSetting', key: 'paused', value: false }));
  $('gear').addEventListener('click', () => { renderPrefs(); $('prefs').showModal(); });

  function renderAll() {
    if (!S.state) return;
    renderHeader();
    renderGrids();
    renderDetail();
    if ($('prefs').open) renderPrefs();
  }

  function onState(st) {
    S.state = st;
    if (S.selected && !byId(S.selected)) { S.selected = null; S.props = null; }
    if (!S.selected) {
      // Start on what's showing on the chosen screen.
      const d = S.target === 'all' ? displays().find(d => d.wallpaper) : displays().find(d => d.id === S.target);
      if (d && d.wallpaper) { S.selected = d.wallpaper; send({ type: 'getProps', id: d.wallpaper }); }
    } else {
      send({ type: 'getProps', id: S.selected });
    }
    renderAll();
  }

  if (host) {
    host.addEventListener('message', e => {
      const m = e.data;
      if (!m || typeof m !== 'object') return;
      switch (m.type) {
        case 'state': onState(m); break;
        case 'props':
          if (m.id === S.selected) {
            // Keep focus where it was if the panel is redrawn while someone is using it.
            const focused = document.activeElement && document.activeElement.id;
            S.props = m;
            renderDetail();
            if (focused && $(focused)) $(focused).focus();
          }
          break;
        case 'toast': toast(m.text, m.kind); break;
        case 'busy': $('busy').hidden = !m.on; if (m.text) $('busy-text').textContent = m.text; break;
        case 'imported': select(m.id); break;
      }
    });
    send({ type: 'hello' });
  } else {
    document.body.prepend(el('p', { style: 'padding:20px', text: 'Open this page from the Live Wallpaper app.' }));
  }
})();
